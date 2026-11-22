//! Hydrate listed source pins; let the registry solver choose a consistent version set.
use super::*;
use crate::{
    browser::sunrise::{self, sources},
    instances::Channel,
};

#[derive(Clone)]
pub struct SourceDependency {
    pub name: String,
    pub version: String,
    pub origin: Option<Origin>,
}

#[derive(Default)]
pub(in crate::browser::sunrise::registry_install) struct SourceAdd {
    pub(in crate::browser::sunrise::registry_install) dependencies: Vec<SourceDependency>,
    registry_stage: Option<tempfile::TempDir>,
}

impl SourceAdd {
    pub(in crate::browser::sunrise::registry_install) fn home<'a>(
        &'a self,
        source_stage: &'a Path,
    ) -> &'a Path {
        self.registry_stage
            .as_ref()
            .map_or(source_stage, tempfile::TempDir::path)
    }
}

pub(in crate::browser::sunrise::registry_install) fn complete_source_add(
    instance: &Instance,
    home: &Path,
    base: &str,
    roots: &[Entry<'_>],
    active: Option<&managed_play::Active>,
    cancellation: &native_mods::Cancellation,
    progress: &dyn Fn(InstallProgress),
) -> Result<SourceAdd, SourceError> {
    let before = inventory(home, None)?;
    let enabled = mutations::enabled(home)?;
    let mut manifests = before
        .iter()
        .filter(|(id, _)| enabled.contains(*id))
        .map(|(id, seen)| (id.clone(), seen.package.manifest.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut pending = roots
        .iter()
        .map(|root| root.origin.id.clone())
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut stable_missing = false;
    let mut listings = None;
    let mut packed = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancellation.check()?;
        if !visited.insert(id.clone()) {
            continue;
        }
        if !manifests.contains_key(&id) {
            if let Some(existing) = before.get(&id) {
                return Err(SourceError::Catalogue(trf(
                    "Required mod {mod} is installed but off. Enable it first.",
                    &[("mod", &existing.package.manifest.name)],
                )));
            }
            if instance.channel == Channel::Stable {
                // The solver can walk the registry versions' own transitive requirements.
                stable_missing = true;
                continue;
            }
            let catalogue = listings.get_or_insert_with(|| sources::catalogue(false));
            if catalogue.error.is_some() || catalogue.saved_at.is_none() {
                return Err(SourceError::Catalogue(
                    tr("Connect to check the source catalogue for required mods.").into(),
                ));
            }
            let listing = catalogue
                .sources
                .iter()
                .find(|source| source.id() == id)
                .ok_or_else(|| {
                    SourceError::Catalogue(trf(
                        "Required mod {mod} isn't listed in this profile's catalogue.",
                        &[("mod", &id)],
                    ))
                })?;
            let pin = sources::resolve(listing, false).map_err(SourceError::Github)?;
            cancellation.check()?;
            let (package, mut origin) =
                github_mods::pack_pin(&pin, cancellation).map_err(SourceError::Github)?;
            origin.added_from = Some(AddedFrom::Discover);
            let bundle = index::native::Bundle::read(&package.file)?;
            manifests.insert(id.clone(), bundle.manifest);
            packed.insert(id.clone(), PastedSource { package, origin });
        }
        pending.extend(
            manifests[&id]
                .dependencies
                .iter()
                .filter(|(_, rule)| {
                    let details = rule.details();
                    !details.optional && details.side != Side::Server
                })
                .map(|(id, _)| id.clone()),
        );
    }
    let local = sunrise::local();
    let (catalogue, authority) = if stable_missing {
        let location = sunrise::location();
        if location != sunrise::INDEX_URL && !(cfg!(debug_assertions) && local) {
            return Err(SourceError::Catalogue(
                tr("Connect to the Sunrise registry before installing a mod and its dependencies.")
                    .into(),
            ));
        }
        let (catalogue, _, authority) = fetch_with_official(&location, local)?;
        if !catalogue.left_out.is_empty() {
            return Err(SourceError::Catalogue(
                tr("The mod registry didn't send a complete index. Nothing was installed.").into(),
            ));
        }
        (catalogue, authority)
    } else {
        (index::Index::default(), None)
    };
    let fixed = manifests
        .iter()
        .map(|(id, manifest)| {
            resolve::FixedMod::new(
                manifest,
                before
                    .get(id)
                    .and_then(|existing| existing.digest.as_deref())
                    .or_else(|| packed.get(id).map(|source| source.origin.sha256.as_str())),
            )
        })
        .collect::<Vec<_>>();
    let requests = roots
        .iter()
        .map(|root| resolve::Request {
            id: root.origin.id.clone(),
            version: None,
        })
        .collect::<Vec<_>>();
    let game = backend::game_version(base);
    let mut conflicts = Vec::new();
    let mut unfit = BTreeSet::new();
    let mut last_refusal = None;
    'trials: for _ in 0..MAX_TRIALS {
        let plan = resolve::install_with_fixed(
            &catalogue,
            &requests,
            &fixed,
            &game,
            Side::Client,
            resolve::WholeOptions {
                conflicts: &conflicts,
                native_fits: |p: &index::Project, v: &index::Version| {
                    !unfit.contains(&(p.id.clone(), v.version.clone()))
                },
                messages: trf,
            },
        )
        .map_err(|error| match error {
            resolve::WholeRefusal::Inconsistent(why) => {
                last_refusal.take().unwrap_or(SourceError::Catalogue(why))
            }
            resolve::WholeRefusal::Unproved(why) => SourceError::Unproved(why),
        })?;
        if plan.install.is_empty() {
            // A source listing contributes one exact pin. There are no alternate source
            // versions to try, and the solver has checked all their manifest requirements.
            if !packed.is_empty() {
                let entries = packed
                    .values()
                    .map(|pasted| Entry {
                        package: &pasted.package,
                        origin: &pasted.origin,
                        prior: github::Prior::Fresh,
                    })
                    .collect::<Vec<_>>();
                github::stage_entries(home, &entries, &BTreeMap::new(), active, base)?;
            }
            check_source_set(home, &[], base, cancellation)?;
            return Ok(SourceAdd {
                dependencies: packed
                    .into_values()
                    .map(|pasted| SourceDependency {
                        name: pasted.package.name,
                        version: pasted.package.version,
                        origin: Some(pasted.origin),
                    })
                    .collect(),
                registry_stage: None,
            });
        }
        // Start each registry trial from the same checked roots. Only positive graph or
        // SDK evidence may exclude a version; an offline or unproved check stops the add.
        let parent = home
            .parent()
            .expect("source stage is a temporary directory");
        let stage = tempfile::tempdir_in(parent).map_err(|e| at(parent, e))?;
        managed_play::copy_install_inputs(home, stage.path())?;
        let mut doc = Document::read(stage.path())?;
        let mut added = Vec::new();
        for row in &plan.install {
            cancellation.check()?;
            if active.is_some_and(|active| active.holds().any(|held| held.id() == row.id)) {
                return Err(held_change_refusal(&row.id).into());
            }
            let (project, version) = planned_version(&catalogue, &row.id, &row.version, &game)?;
            let mut entry = stage_package(
                &catalogue,
                local,
                stage.path(),
                &before,
                project,
                version,
                |fraction| progress(InstallProgress::Downloading(fraction)),
            )?;
            entry.dependency = true;
            doc.replace(&entry)?;
            mod_list::set_enabled(stage.path(), &row.id, true)?;
            added.push(SourceDependency {
                name: project.name.clone(),
                version: row.version.clone(),
                origin: None,
            });
        }
        doc.write(stage.path())?;
        // All solver candidates (fixed and downloaded) have checked digests at this point.
        if let Some(conflict) =
            manifest_conflict(stage.path(), Some(&catalogue), authority.as_ref(), |_| true)?
        {
            if conflicts.contains(&conflict) {
                return Err(SourceError::Unproved(conflict.reason));
            }
            last_refusal = Some(SourceError::Catalogue(conflict.reason.clone()));
            conflicts.push(conflict);
            continue;
        }
        for (id, route) in source_fit(stage.path(), base, cancellation)?.mods {
            if let native_mods::Route::Unfit { line, .. } = &route {
                if let Some(row) = plan.install.iter().find(|row| row.id == id) {
                    last_refusal = Some(SourceError::DoesNotFit(line.clone()));
                    unfit.insert((id, row.version.clone()));
                    continue 'trials;
                }
            }
            checked_source_route(route)?;
        }
        return Ok(SourceAdd {
            dependencies: added,
            registry_stage: Some(stage),
        });
    }
    Err(SourceError::Unproved(
        tr("Not every compatible version could be checked. Nothing was installed.").into(),
    ))
}
