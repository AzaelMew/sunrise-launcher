//! Explicit same-base mutations. Approval is for the exact aggregate impact before publication.
use super::*;
use crate::worlds;
use sunrise_launcher_smod::{dependencies::parse_version, mod_list::ModList};

pub(super) fn update_floors(
    catalogue: &index::Index,
    before: &Inventory,
    game: &str,
    requested: Option<&[String]>,
) -> Result<BTreeMap<String, semver::Version>, InstallError> {
    let ids = match requested {
        Some(ids) => ids.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        None => before
            .iter()
            .filter(|(_, entry)| entry.owned)
            .map(|(id, _)| id.as_str())
            .collect(),
    };
    let mut above = BTreeMap::new();
    for id in ids {
        let old = before.get(id).filter(|p| p.owned).ok_or_else(|| {
            InstallError::Refused(trf(
                "{mod} wasn't installed from the Sunrise registry.",
                &[("mod", id)],
            ))
        })?;
        let installed =
            parse_version(&old.package.manifest.version).map_err(|_| unproved_mods())?;
        if catalogue.project(id).is_some_and(|p| {
            p.versions.iter().any(|v| {
                fits_game(v, game)
                    && catalogue.revoked(id, &v.file.sha256).is_none()
                    && parse_version(&v.version)
                        .is_ok_and(|new| new.cmp_precedence(&installed).is_gt())
            })
        }) {
            above.insert(id.to_owned(), installed);
        }
    }
    Ok(above)
}

pub(super) fn enabled(home: &Path) -> Result<BTreeSet<String>, String> {
    let path = home.join(sunrise_launcher_smod::mod_list::FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(ModList::parse(&text).enabled),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(at(&path, error)),
    }
}

pub(super) fn world_evidence(
    home: &Path,
    suspect: &worlds::SuspectMods,
) -> Result<Vec<worlds::WorldRisk>, String> {
    if suspect.is_empty() {
        return Ok(Vec::new());
    }
    worlds::review_risk(home, suspect)
}

pub(super) fn bind_worlds(
    home: &Path,
    suspect: &worlds::SuspectMods,
    reviewed: &[worlds::WorldRisk],
) -> Result<worlds::ReviewedWorldLocks, String> {
    let changed = || {
        tr("The affected worlds changed after review. Nothing was changed; try again.").to_owned()
    };
    let locks = match worlds::lock_reviewed(reviewed) {
        Ok(locks) => locks,
        Err(error) => {
            return Err(if world_evidence(home, suspect)? != reviewed {
                changed()
            } else {
                error
            });
        }
    };
    if world_evidence(home, suspect)? != reviewed {
        return Err(changed());
    }
    Ok(locks)
}

#[derive(Debug)]
pub struct ChangeImpact {
    change: Change,
    evidence: Vec<worlds::WorldRisk>,
}

#[derive(Debug)]
pub(super) enum Change {
    Update(Vec<String>),
    Remove {
        removed: Vec<String>,
        disabled: Vec<String>,
    },
}

impl ChangeImpact {
    pub(super) fn read(
        home: &Path,
        change: Change,
        suspect: &worlds::SuspectMods,
    ) -> Result<Self, String> {
        Ok(Self {
            change,
            evidence: world_evidence(home, suspect)?,
        })
    }

    pub(super) fn into_evidence(self) -> Vec<worlds::WorldRisk> {
        self.evidence
    }

    fn affected(&self) -> impl Iterator<Item = &worlds::WorldRisk> {
        self.evidence.iter().filter(|world| !world.mods.is_empty())
    }

    pub fn risks_worlds(&self) -> bool {
        self.affected().next().is_some()
    }

    pub fn is_removal(&self) -> bool {
        matches!(self.change, Change::Remove { .. })
    }

    pub fn detail(&self) -> String {
        let mut lines = Vec::new();
        match &self.change {
            Change::Update(mods) => {
                lines.push(trf("Update: {mods}.", &[("mods", &mods.join(", "))]));
            }
            Change::Remove { removed, disabled } => {
                lines.push(trf("Remove: {mods}.", &[("mods", &removed.join(", "))]));
                if !disabled.is_empty() {
                    lines.push(trf(
                        "These mods need the ones you are removing. They will be turned off, but their files will stay installed: {mods}.",
                        &[("mods", &disabled.join(", "))],
                    ));
                }
            }
        }
        match self.affected().count() {
            0 => (),
            1 => lines.push(tr(
                "Blocks and items from the affected mods will be lost when you open 1 saved world without their definitions.",
            ).into()),
            n => lines.push(trf(
                "Blocks and items from the affected mods will be lost when you open {count} saved worlds without their definitions.",
                &[("count", &n.to_string())],
            )),
        }
        if self.affected().any(|world| !world.known) {
            lines.push(tr("Sunrise couldn't check which mods some of these worlds use.").into());
        }
        lines.join("\n\n")
    }
}

/// The ordinary enable switch must not reactivate an exact hold or a mod missing a required
/// dependency. Call under the game's manifest lock and write the list under the same lock.
pub fn check_activation(home: &Path, activate: &[&str]) -> Result<(), String> {
    let packages = inventory(home)?;
    let mut enabled = enabled(home)?;
    enabled.extend(activate.iter().map(|&id| id.to_owned()));
    let manifests = packages
        .iter()
        .filter(|(id, _)| enabled.contains(*id))
        .map(|(id, p)| (id.as_str(), &p.package.manifest))
        .collect::<BTreeMap<_, _>>();
    let active = managed_play::Active::read_pointer(home)?;
    let mut visited = BTreeSet::new();
    let mut remaining = activate.to_vec();
    while let Some(id) = remaining.pop() {
        if !visited.insert(id) {
            continue;
        }
        if active
            .as_ref()
            .is_some_and(|active| active.holds().any(|hold| hold.id() == id))
        {
            return Err(trf(
                "{mod} needs an update from its author before it can be turned on.",
                &[("mod", id)],
            ));
        }
        let target = manifests.get(id).ok_or_else(unproved_mods)?;
        for (name, dep) in &target.dependencies {
            let requirement = dep.details();
            if requirement.optional {
                continue;
            }
            if !manifests.get(name.as_str()).is_some_and(|other| {
                parse_version(&other.version)
                    .is_ok_and(|version| requirement.version.matches(&version))
            }) {
                return Err(trf("{mod} needs {required} {version}. Install or turn on a version that fits before trying again.", &[
                    ("mod", &target.name), ("required", name), ("version", &requirement.version.to_string()),
                ]));
            }
            remaining.push(name.as_str());
        }
    }
    Ok(())
}

/// Remove exact owned archive(s) and disable, rather than delete, the required dependent closure.
pub fn remove_mods(
    instance: &Instance,
    identity: Identity,
    ids: &[String],
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: InstallHooks<'_>,
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<Vec<String>, InstallError> {
    if instance.kind == InstanceKind::Pack || ids.is_empty() {
        return Err(InstallError::Cancelled);
    }
    (hooks.progress)(InstallProgress::Checking);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let edit = profile.begin_edit(
        choose,
        |f| (hooks.progress)(InstallProgress::Downloading(f)),
        &|_| (hooks.progress)(InstallProgress::Preparing),
    )?;
    let old = edit.candidate()?;
    let next = edit.candidate()?;
    let home = next.path();
    let present = inventory(home)?;
    let ids = ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut rows = Document::read(home)?;
    let enabled = enabled(home)?;
    let mut changed_rows = false;
    let mut retired = BTreeSet::new();
    for id in &ids {
        let item = present.get(id).ok_or_else(|| {
            trf(
                "Couldn't verify the installed file for {mod}. Nothing was removed.",
                &[("mod", id)],
            )
        })?;
        if let Some(hold) = edit.held(id) {
            if !item.matches_hold(home, edit.base(), hold) {
                return Err(held_change_refusal(id));
            }
            retired.insert(id.clone());
        }
        if item.owned {
            rows.remove(id)?;
            changed_rows = true;
        } else if item.github_origin(home).is_some() {
            let origin = item.package.path.with_extension("origin.json");
            fs::remove_file(&origin).map_err(|e| at(&origin, e))?;
        } else {
            return Err(InstallError::Refused(trf(
                "{mod} isn't an installed registry or GitHub mod. Nothing was removed.",
                &[("mod", &item.package.manifest.name)],
            )));
        }
        fs::remove_file(&item.package.path).map_err(|e| at(&item.package.path, e))?;
    }
    if changed_rows {
        rows.write(home)?;
    }

    // Only propagate along actual, enabled, required edges. Keep every dependent path and
    // provenance byte; validate the effective enabled graph separately after changing its list.
    let mut disabled = BTreeSet::new();
    let mut unavailable = ids.clone();
    loop {
        let before = unavailable.len();
        for (id, item) in &present {
            if !unavailable.contains(id)
                && enabled.contains(id)
                && item
                    .package
                    .manifest
                    .dependencies
                    .iter()
                    .any(|(dep, rule)| !rule.details().optional && unavailable.contains(dep))
            {
                disabled.insert(id.clone());
                unavailable.insert(id.clone());
            }
        }
        if before == unavailable.len() {
            break;
        }
    }
    let disabled_ids = unavailable.iter().map(String::as_str).collect::<Vec<_>>();
    mod_list::set_all_enabled(home, &disabled_ids, false)?;
    if let Some(conflict) = manifest_conflict(home, None)? {
        return Err(InstallError::Refused(conflict.reason));
    }
    let named = |id: &String| format!("{} ({id})", present[id].package.manifest.name);
    let removed = ids.iter().map(named).collect::<Vec<_>>();
    let suspect = worlds::unproved_mod_effects(
        old.path(),
        edit.active_basis(),
        home,
        edit.active_basis(),
        unavailable.iter().map(String::as_str),
    )?;
    let impact = ChangeImpact::read(
        &instance.home,
        Change::Remove {
            removed: removed.clone(),
            disabled: disabled.iter().map(named).collect(),
        },
        &suspect,
    )?;
    (hooks.progress)(InstallProgress::Reviewing);
    if !approve(&impact) {
        return Err(InstallError::Cancelled);
    }
    let cancellation = native_mods::Cancellation::default();
    let proof = prove_edit(&edit, home, instance, &cancellation, &hooks)
        .map_err(|e| InstallError::Unproved(e.into()))?;
    (hooks.progress)(InstallProgress::Publishing);
    let reviewed = impact.into_evidence();
    edit.publish(proof, &cancellation, &retired, || {
        bind_worlds(&instance.home, &suspect, &reviewed)
    })?;
    Ok(removed)
}
