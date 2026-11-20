//! Explicit registry mutations on an admitted build. Download and compilation happen in
//! the reserved private candidate; the approval object has no request it could run again.
use super::{
    authoritative_index, dependencies, fits_game, index, installed, registry_candidates, txn,
    Install,
};
use crate::{backend, i18n::tr, native_mods, worlds};
use std::{collections::BTreeMap, sync::Arc};

const MAX_TRIALS: usize = 512;
type Choice<'a> = (&'a index::Project, &'a index::Version);

pub struct RegistryInstall {
    ready: Box<txn::ReadyInstall>,
    evidence: String,
    entries: Vec<installed::Entry>,
}

impl RegistryInstall {
    pub fn entries(&self) -> &[installed::Entry] {
        &self.entries
    }

    pub fn question(&self) -> &worlds::SwitchQuestion {
        self.ready.question()
    }

    /// This never re-runs a build. A different catalogue or changed live input refuses at
    /// cutoff, even after the player agreed to the world question.
    pub fn commit(self, approved: bool) -> Result<Vec<installed::Entry>, String> {
        self.ready.commit(approved, || {
            let (_, current) = authoritative_index()?;
            if current != self.evidence {
                return Err(tr("The mod registry changed while the install was being prepared. Nothing was installed.").into());
            }
            Ok(())
        })?;
        Ok(self.entries)
    }
}

pub fn prepare_registry_install(
    owner: Arc<txn::Lease>,
    instance: &crate::instances::Instance,
    ids: &[String],
    progress: &dyn Fn(&str, f32),
    registry_consent: &dyn Fn(&native_mods::Consent) -> bool,
) -> Result<RegistryInstall, installed::InstallError> {
    let (index, evidence) = authoritative_index()?;
    let waiting = txn::waiting_packages(owner.home())?;
    let first_plan = txn::StagedInstall::for_instance(owner.clone(), instance, progress)?;
    let game =
        backend::game_of_build(first_plan.base()).ok_or_else(backend::no_version_installed)?;
    let mut first = Some(first_plan);
    let native_allowed = backend::Settings::native_grant()?.is_some();
    let options = requested_choices(&index, ids, &game, &waiting)?;
    let mut refused = None;
    let (ready, entries) = ranked(&options, |asked| {
        let plan = match first.take() {
            Some(plan) => plan,
            None => txn::StagedInstall::for_instance(owner.clone(), instance, progress)?,
        };
        let entries = match dependencies::stage(
            &Install {
                index: &index,
                local: false,
                home: plan.stage(),
                approvals: &[],
                progress,
            },
            asked,
            native_allowed,
            &waiting,
        ) {
            Ok(entries) => entries,
            Err(
                error @ (installed::InstallError::Conflict { .. }
                | installed::InstallError::Unavailable { .. }
                | installed::InstallError::Refused(_)),
            ) => {
                refused.get_or_insert(error);
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if !plan.compatible_candidate(&index)? {
            refused.get_or_insert_with(|| installed::InstallError::Refused(incompatible().into()));
            return Ok(None);
        }
        match plan.prepare(
            &index,
            &[],
            instance.native_profile,
            |step| progress(&step.label(), 0.99),
            Some(registry_consent),
        ) {
            Ok(ready) => Ok(Some((ready, entries))),
            Err(error @ native_mods::Failed::Mods { .. }) => {
                let error = installed::InstallError::for_build(error, &ids.join(", "));
                if !matches!(refused, Some(installed::InstallError::Build { .. })) {
                    refused = Some(error);
                }
                Ok(None)
            }
            Err(error) => Err(installed::InstallError::for_build(error, &ids.join(", "))),
        }
    })?
    .ok_or_else(|| {
        refused.unwrap_or_else(|| installed::InstallError::Refused(incompatible().into()))
    })?;
    Ok(RegistryInstall {
        ready: Box::new(ready),
        evidence,
        entries,
    })
}

fn incompatible() -> &'static str {
    tr("The proposed mod set has incompatible packages or missing dependencies. Nothing was installed.")
}

fn requested_choices<'a>(
    catalogue: &'a index::Index,
    ids: &[String],
    game: &str,
    waiting: &BTreeMap<String, txn::WaitingMod>,
) -> Result<Vec<Vec<Choice<'a>>>, installed::InstallError> {
    let projects: BTreeMap<_, _> =
        registry_candidates(catalogue, ids.iter().map(String::as_str), game, waiting)?
            .into_iter()
            .map(|(project, _)| (project.id.as_str(), project))
            .collect();
    Ok(projects
        .into_values()
        .map(|project| {
            let mut choices: Vec<_> = project
                .versions
                .iter()
                .filter(|v| {
                    fits_game(v, game)
                        && catalogue.revoked(&project.id, &v.file.sha256).is_none()
                        && waiting
                            .get(&project.id)
                            .is_none_or(|waiting| !waiting.same_package(v))
                })
                .map(|v| (project, v))
                .collect();
            choices.sort_by(|(_, a), (_, b)| {
                match (
                    semver::Version::parse(&a.version),
                    semver::Version::parse(&b.version),
                ) {
                    (Ok(a), Ok(b)) => b.cmp_precedence(&a),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => b.version.cmp(&a.version),
                }
                .then_with(|| a.file.sha256.cmp(&b.file.sha256))
            });
            choices
        })
        .collect())
}

/// Search the requested pins in stable ID and descending version order. Each trial's dependency
/// resolver may move a library only as needed; a rejected trial cannot change the next preimage.
fn ranked<T>(
    options: &[Vec<Choice<'_>>],
    mut trial: impl FnMut(&[Choice<'_>]) -> Result<Option<T>, installed::InstallError>,
) -> Result<Option<T>, installed::InstallError> {
    if options.is_empty() || options.iter().any(Vec::is_empty) {
        return Ok(None);
    }
    let mut cursor = vec![0; options.len()];
    for _ in 0..MAX_TRIALS {
        let selected: Vec<_> = options
            .iter()
            .zip(&cursor)
            .map(|(choices, n)| choices[*n])
            .collect();
        if let Some(result) = trial(&selected)? {
            return Ok(Some(result));
        }
        let Some(next) = (0..cursor.len()).rfind(|i| cursor[*i] + 1 < options[*i].len()) else {
            return Ok(None);
        };
        cursor[next] += 1;
        cursor[next + 1..].fill(0);
    }
    Err(tr("The launcher couldn't verify every alternative. Nothing was installed.").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_requests_select_the_older_version_that_satisfies_both() {
        let fixture = tempfile::tempdir().unwrap();
        let catalogue = super::super::tests::conflict_index(fixture.path());
        let waiting = BTreeMap::new();
        let ids = ["lamps".into(), "creatures".into()];
        let game = &catalogue.project("lamps").unwrap().versions[0].game;
        let choices = requested_choices(&catalogue, &ids, game, &waiting).unwrap();
        let mut attempts = Vec::new();
        let result = ranked(&choices, |selected| {
            let stage = tempfile::tempdir().unwrap();
            attempts.push(selected[0].1.version.clone());
            let attempt = dependencies::stage(
                &Install {
                    index: &catalogue,
                    local: true,
                    home: stage.path(),
                    approvals: &[],
                    progress: &|_, _| {},
                },
                selected,
                false,
                &waiting,
            );
            match attempt {
                Err(installed::InstallError::Refused(_)) => Ok(None),
                other => other.map(Some),
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(attempts, ["2.0.0", "1.0.0"]);
        assert_eq!(
            result
                .iter()
                .find(|e| e.project == "creatures")
                .unwrap()
                .version,
            "1.0.0"
        );
        assert_eq!(
            result
                .iter()
                .find(|e| e.project == "lamps")
                .unwrap()
                .version,
            "1.0.0"
        );

        let mut count = 0;
        let failed = ranked::<()>(&choices, |_| {
            count += 1;
            Err(installed::InstallError::Message("network failed".into()))
        })
        .unwrap_err();
        assert_eq!(count, 1, "unknown evidence stops search immediately");
        assert_eq!(failed.to_string(), "network failed");
        let huge = vec![choices[0].clone(); 10];
        assert!(ranked::<()>(&huge, |_| Ok(None))
            .unwrap_err()
            .to_string()
            .contains("couldn't verify every alternative"));
    }
}
