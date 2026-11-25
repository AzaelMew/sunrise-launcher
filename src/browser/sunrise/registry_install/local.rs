//! Local `.smod` and local-folder installs run through the checked profile transaction:
//! recover pending edits, check raw claims and alias-aware destinations under the profile lock,
//! resolve predecessors by manifest ID before any write or deletion, review exact replacements
//! and their saved-world impact, and publish archives, sidecars and enabled state together.
use super::*;
use crate::{
    github_mods::LocalPackage,
    profile_txn::{Admission, Intent},
};
use std::io::ErrorKind;
use sunrise_launcher_smod::{index::sha256_file, mod_list as smod_mod_list};

/// Install validated local `.smod` or local-folder packages into `instance` as one checked,
/// cancellable profile transaction.
pub fn install_local(
    target: &Target,
    identity: Identity,
    packages: &[LocalPackage],
    cancellation: &native_mods::Cancellation,
    progress: &dyn Fn(InstallProgress),
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<Vec<String>, InstallError> {
    if target.is_pack() {
        return Err(InstallError::Refused(
            tr("This pack controls which mods it installs.").into(),
        ));
    }
    if packages.is_empty() {
        return Err(InstallError::Cancelled);
    }
    check_cancelled(cancellation)?;
    progress(InstallProgress::Checking);
    let mut profile =
        Profile::open_for(target.home(), None, Some(identity)).map_err(String::from)?;
    check_regular_mod_dirs(target.home())?;
    let edit = profile.begin_pending_edit()?;
    let next = edit.candidate()?;
    let active = edit.current();
    let staged = stage_and_review(
        target.home(),
        next.path(),
        active,
        packages,
        cancellation,
        progress,
        approve,
    )?;
    let checked_inputs = managed_play::inputs(next.path())?;
    check_cancelled(cancellation)?;
    progress(InstallProgress::Publishing);
    let result = edit.publish_pending(next.path(), checked_inputs, cancellation, || {
        revalidate_claims(target.home(), &staged.claim_checks)?;
        if let Some((suspect, reviewed)) = &staged.worlds {
            mutations::bind_worlds(target.home(), suspect, reviewed).map(Some)
        } else {
            Ok(None)
        }
    });
    finish_publish(result, cancellation, staged.names)
}

/// Install validated local `.smod` or local-folder packages into `home` directly through
/// `profile_txn::Admission` without requiring an `Instance` or game cache.
pub fn install_local_at(
    home: &Path,
    packages: &[LocalPackage],
    cancellation: &native_mods::Cancellation,
    progress: &dyn Fn(InstallProgress),
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<Vec<String>, InstallError> {
    if packages.is_empty() {
        return Err(InstallError::Cancelled);
    }
    check_cancelled(cancellation)?;
    progress(InstallProgress::Checking);
    let mut admission = Admission::open(home, None)?;
    check_regular_mod_dirs(home)?;
    let (before, active, stage) = {
        let _lock =
            sunrise_launcher_smod::install_lock::lock_home(home).map_err(|e| at(home, e))?;
        crate::browser::installed::recover_game_install(home).map_err(String::from)?;
        if admission.has_bootstrap()? {
            return Err(InstallError::Unproved(
                tr("Finish the profile's first Play before changing mods.").into(),
            ));
        }
        let before = managed_play::inputs(home)?;
        // Under Admission and the home lock: read the profile's own pointer, as Profile::active
        // does. display_active would refuse while any other profile publishes.
        let active = managed_play::Active::read_pointer(home)?;
        let targets = match &active {
            Some(current) => [current.base().to_owned(), current.executable().to_owned()]
                .into_iter()
                .collect(),
            None => BTreeSet::new(),
        };
        let stage = admission.reserve(Intent::Install, targets)?;
        (before, active, stage)
    };
    let next = tempfile::Builder::new()
        .prefix("candidate-")
        .tempdir_in(stage.path())
        .map_err(|e| at(stage.path(), e))?;
    {
        let _lock =
            sunrise_launcher_smod::install_lock::lock_home(home).map_err(|e| at(home, e))?;
        managed_play::copy_install_inputs(home, next.path())?;
        if managed_play::inputs(next.path())? != before || managed_play::inputs(home)? != before {
            return Err(InstallError::Unproved(
                tr(
                    "The profile changed while preparing this install. Nothing was changed; try again.",
                )
                .into(),
            ));
        }
    }
    let staged = stage_and_review(
        home,
        next.path(),
        active.as_ref(),
        packages,
        cancellation,
        progress,
        approve,
    )?;
    let checked_inputs = managed_play::inputs(next.path())?;
    check_cancelled(cancellation)?;
    progress(InstallProgress::Publishing);
    let result = (|| {
        let _lock =
            sunrise_launcher_smod::install_lock::lock_home(home).map_err(|e| at(home, e))?;
        crate::browser::installed::recover_game_install(home).map_err(String::from)?;
        if managed_play::inputs(home)? != before
            || managed_play::inputs(next.path())? != checked_inputs
        {
            return Err(tr(
                "The profile changed while preparing this install. Nothing was changed; try again.",
            )
            .into());
        }
        let files = managed_play::changed_files(home, next.path())?;
        if files.is_empty() {
            return Ok(());
        }
        let mut authorization = None;
        stage.seal_files(None, &files)?.publish(|| {
            revalidate_claims(home, &staged.claim_checks)?;
            authorization = if let Some((suspect, reviewed)) = &staged.worlds {
                Some(mutations::bind_worlds(home, suspect, reviewed)?)
            } else {
                None
            };
            if managed_play::inputs(home)? != before
                || managed_play::inputs(next.path())? != checked_inputs
            {
                return Err(tr(
                    "The profile changed while preparing this install. Nothing was changed; try again.",
                )
                .into());
            }
            cancellation.begin_publication()
        })
    })();
    finish_publish(result, cancellation, staged.names)
}

fn finish_publish(
    result: Result<(), String>,
    cancellation: &native_mods::Cancellation,
    names: Vec<String>,
) -> Result<Vec<String>, InstallError> {
    match result {
        Ok(()) => Ok(names),
        Err(error) if cancellation.is_stopped() || error == native_mods::CANCELLED => {
            Err(InstallError::Cancelled)
        }
        Err(error) => Err(InstallError::Unproved(error)),
    }
}

fn check_cancelled(cancellation: &native_mods::Cancellation) -> Result<(), InstallError> {
    if cancellation.is_stopped() || cancellation.check().is_err() {
        Err(InstallError::Cancelled)
    } else {
        Ok(())
    }
}

struct StagedLocal {
    names: Vec<String>,
    claim_checks: Vec<(String, String)>,
    worlds: Option<(crate::worlds::SuspectMods, Vec<crate::worlds::WorldRisk>)>,
}

struct PlannedPackage<'a> {
    pkg: &'a LocalPackage,
    dest_dir: &'static str,
    dest_rel: String,
    sidecar_rel: String,
    prior_archive: Option<PathBuf>,
    prior_sidecar: Option<PathBuf>,
    prior_version: Option<String>,
}

fn stage_and_review(
    live: &Path,
    candidate: &Path,
    active: Option<&managed_play::Active>,
    packages: &[LocalPackage],
    cancellation: &native_mods::Cancellation,
    progress: &dyn Fn(InstallProgress),
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<StagedLocal, InstallError> {
    check_cancelled(cancellation)?;
    check_regular_mod_dirs(live)?;

    let claims = browser_state::Installed::read(live).map_err(|e| e.details(trf))?;
    let before = inventory(candidate)?;
    // The saved-world comparison reads the live profile, which staging never changes: the
    // candidate's copy of a replaced archive is overwritten or removed before the review.
    let original = inventory(live)?;
    let enabled_before = mutations::enabled(candidate)?;

    let mut seen_ids = BTreeSet::new();
    let mut planned = Vec::with_capacity(packages.len());
    let mut claim_checks = Vec::new();
    let mut planned_destinations = BTreeSet::new();

    // Validate ownership, raw claims, aliases, and manifest-ID predecessors before writing or
    // deleting a single file in the candidate copy.
    for pkg in packages {
        check_cancelled(cancellation)?;
        let id = &pkg.id;
        if !seen_ids.insert(id.as_str()) {
            return Err(InstallError::Refused(trf(
                "Duplicate mod {mod} in the selected packages.",
                &[("mod", id)],
            )));
        }
        if active.is_some_and(|a| a.holds().any(|held| held.id() == id)) {
            return Err(held_change_refusal(id));
        }
        if claims.single(id).map_err(|e| e.render(trf))?.is_some() {
            return Err(InstallError::Refused(trf(
                "{mod} is installed from the Sunrise registry. Remove it first before installing an unreviewed copy.",
                &[("mod", &pkg.name)],
            )));
        }
        let dest_dir = if pkg.native {
            smod_mod_list::NATIVE_DIR
        } else {
            smod_mod_list::DIR
        };
        let mut dest_rel = format!("{dest_dir}/{id}.smod");
        let sidecar_rel = format!("{dest_dir}/{id}.origin.json");
        for rel in [&dest_rel, &sidecar_rel] {
            if !planned_destinations.insert(rel.to_ascii_lowercase()) {
                return Err(InstallError::Refused(trf(
                    "Duplicate mod {mod} in the selected packages.",
                    &[("mod", id)],
                )));
            }
            claims
                .check_other_claims(live, id, rel)
                .map_err(|e| e.render(trf))?;
            claim_checks.push((id.clone(), rel.clone()));
        }

        let (prior_archive, prior_sidecar, prior_version) = if let Some(prior) = before.get(id) {
            if prior.registry_claim {
                return Err(InstallError::Refused(trf(
                    "{mod} is installed from the Sunrise registry. Remove it first before installing an unreviewed copy.",
                    &[("mod", &pkg.name)],
                )));
            }
            if prior.package.path.is_dir() {
                return Err(InstallError::Refused(trf(
                    "{mod} is installed as an unpacked folder. Remove it first to install a local copy.",
                    &[("mod", &pkg.name)],
                )));
            }
            let prior_rel = relative_leaf(candidate, &prior.package.path)?;
            claims
                .check_other_claims(live, id, &prior_rel)
                .map_err(|e| e.render(trf))?;
            claim_checks.push((id.clone(), prior_rel.clone()));
            // A predecessor whose name differs only by case keeps its name: two leaves that fold
            // to the same name would be two changes to one file on a case-insensitive disk.
            if prior_rel.eq_ignore_ascii_case(&dest_rel) {
                dest_rel = prior_rel.clone();
            }

            let prior_sidecar = match prior.verified_origin_sidecar(candidate) {
                Ok(Some((sidecar_path, origin))) => {
                    if origin.local_folder_path().is_none() {
                        return Err(InstallError::Refused(trf(
                            "{mod} is already installed from GitHub. Remove it first to install a local copy.",
                            &[("mod", &pkg.name)],
                        )));
                    }
                    let sidecar_rel = relative_leaf(candidate, &sidecar_path)?;
                    claims
                        .check_other_claims(live, id, &sidecar_rel)
                        .map_err(|e| e.render(trf))?;
                    claim_checks.push((id.clone(), sidecar_rel));
                    Some(sidecar_path)
                }
                Ok(None) => None,
                Err(()) => {
                    return Err(InstallError::Refused(trf(
                        "{mod} is already installed from another source or location. Remove the existing mod first.",
                        &[("mod", &pkg.name)],
                    )));
                }
            };

            let allowed_archive = Some(prior.package.path.as_path());
            let allowed_sidecar = prior_sidecar.as_deref();
            check_destination_free(
                candidate,
                dest_dir,
                &format!("{id}.smod"),
                allowed_archive,
                &pkg.name,
            )?;
            check_destination_free(
                candidate,
                dest_dir,
                &format!("{id}.origin.json"),
                allowed_sidecar,
                &pkg.name,
            )?;

            (
                Some(prior.package.path.clone()),
                prior_sidecar,
                Some(prior.package.manifest.version.clone()),
            )
        } else {
            check_destination_free(candidate, dest_dir, &format!("{id}.smod"), None, &pkg.name)?;
            check_destination_free(
                candidate,
                dest_dir,
                &format!("{id}.origin.json"),
                None,
                &pkg.name,
            )?;
            (None, None, None)
        };

        planned.push(PlannedPackage {
            pkg,
            dest_dir,
            dest_rel,
            sidecar_rel,
            prior_archive,
            prior_sidecar,
            prior_version,
        });
    }

    // Stage the full selection into `candidate`, removing replaced predecessors first so no
    // duplicate manifest IDs remain across filenames or mod directories.
    let mut replaced_ids = Vec::new();
    let mut reviewed_updates = Vec::new();
    let mut names = Vec::with_capacity(planned.len());
    for item in &planned {
        check_cancelled(cancellation)?;
        let actual = sha256_file(&item.pkg.file)?;
        if !actual.eq_ignore_ascii_case(&item.pkg.sha256) {
            return Err(InstallError::Refused(trf(
                "The staged package for {mod} changed before install. Nothing was written.",
                &[("mod", &item.pkg.name)],
            )));
        }
        if let Some(old_archive) = &item.prior_archive {
            fs::remove_file(old_archive).map_err(|e| at(old_archive, e))?;
        }
        if let Some(old_sidecar) = &item.prior_sidecar {
            fs::remove_file(old_sidecar).map_err(|e| at(old_sidecar, e))?;
        }
        let target_dir = candidate.join(item.dest_dir);
        fs::create_dir_all(&target_dir).map_err(|e| at(&target_dir, e))?;
        let dest = candidate.join(&item.dest_rel);
        fs::copy(&item.pkg.file, &dest).map_err(|e| at(&dest, e))?;
        if !sha256_file(&dest)?.eq_ignore_ascii_case(&item.pkg.sha256) {
            return Err(InstallError::Refused(trf(
                "The staged package for {mod} changed before install. Nothing was written.",
                &[("mod", &item.pkg.name)],
            )));
        }
        if let Some(origin) = &item.pkg.origin {
            let sidecar = candidate.join(&item.sidecar_rel);
            let json = serde_json::to_vec_pretty(origin).map_err(|e| e.to_string())?;
            fs::write(&sidecar, json).map_err(|e| at(&sidecar, e))?;
        }
        let keep_enabled = if item.prior_archive.is_some() {
            enabled_before.contains(&item.pkg.id)
        } else {
            true
        };
        if keep_enabled != mod_list::is_enabled(candidate, &item.pkg.id) {
            mod_list::set_enabled(candidate, &item.pkg.id, keep_enabled)?;
        }
        if let Some(old_version) = &item.prior_version {
            replaced_ids.push(item.pkg.id.clone());
            reviewed_updates.push(mutations::ReviewedUpdate {
                name: item.pkg.name.clone(),
                installed: Some(old_version.clone()),
                replacement: item.pkg.version.clone(),
            });
        }
        names.push(item.pkg.name.clone());
    }

    // Verify the resulting candidate inventory has no corrupt archives, duplicate IDs, or
    // manifest conflicts before asking for world review or publishing.
    check_cancelled(cancellation)?;
    let _ = inventory(candidate)?;
    if let Some(conflict) = manifest_conflict(candidate, Side::Both, |_| false)? {
        return Err(InstallError::Refused(conflict.reason));
    }

    let worlds = if replaced_ids.is_empty() {
        None
    } else {
        let replaced = replaced_ids
            .iter()
            .filter_map(|id| original.get(id).map(|seen| &seen.package))
            .collect::<Vec<_>>();
        let (suspect, unchecked) = mutations::update_worlds(candidate, replaced)?;
        let impact =
            ChangeImpact::read(live, mutations::Change::Update(reviewed_updates), &suspect)?
                .with_unchecked_update(unchecked);
        check_cancelled(cancellation)?;
        progress(InstallProgress::Reviewing);
        if !approve(&impact) {
            return Err(InstallError::Cancelled);
        }
        Some((suspect, impact.into_evidence()))
    };

    Ok(StagedLocal {
        names,
        claim_checks,
        worlds,
    })
}

fn check_regular_mod_dirs(live: &Path) -> Result<(), InstallError> {
    for dir in [smod_mod_list::DIR, smod_mod_list::NATIVE_DIR] {
        let path = live.join(dir);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => (),
            Ok(_) => {
                return Err(InstallError::Refused(
                    tr("Mod directories must be regular folders inside this profile.").into(),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(at(&path, error).into()),
        }
    }
    Ok(())
}

fn relative_leaf(root: &Path, path: &Path) -> Result<String, InstallError> {
    let rel = path.strip_prefix(root).map_err(|_| unproved_mods())?;
    rel.to_str()
        .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| unproved_mods().into())
}

/// Ensures no existing file in `candidate/{dir}` collides with `leaf` (exact or case-folded
/// alias) unless it is the exact `allowed` predecessor path we are replacing.
fn check_destination_free(
    candidate: &Path,
    dir: &str,
    leaf: &str,
    allowed: Option<&Path>,
    display_name: &str,
) -> Result<(), InstallError> {
    let parent = candidate.join(dir);
    let entries = match fs::read_dir(&parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(at(&parent, error).into()),
    };
    let allowed_leaf = allowed
        .filter(|path| path.parent() == Some(parent.as_path()))
        .and_then(Path::file_name);
    for entry in entries {
        let entry = entry.map_err(|e| at(&parent, e))?;
        let name = entry.file_name();
        if allowed_leaf == Some(name.as_os_str()) {
            continue;
        }
        if name.to_string_lossy().eq_ignore_ascii_case(leaf) {
            return Err(InstallError::Refused(trf(
                "{mod} is already installed from another source or location. Remove the existing mod first.",
                &[("mod", display_name)],
            )));
        }
    }
    Ok(())
}

fn revalidate_claims(live: &Path, checks: &[(String, String)]) -> Result<(), String> {
    let claims = browser_state::Installed::read(live).map_err(|e| e.details(trf))?;
    for (id, rel) in checks {
        if claims.single(id).map_err(|e| e.render(trf))?.is_some() {
            return Err(trf(
                "{mod} is installed from the Sunrise registry. Remove it first before installing an unreviewed copy.",
                &[("mod", id)],
            ));
        }
        claims
            .check_other_claims(live, id, rel)
            .map_err(|e| e.render(trf))?;
    }
    Ok(())
}

/// Turn a held local or source mod back on in `instance`: its exact bytes go through the fit
/// check and build for the profile's current build, and only a proved image is published,
/// retiring the hold. A failed check returns what it found, and nothing changes.
pub fn restore_held(
    target: &Target,
    identity: Identity,
    id: &str,
    cancellation: &native_mods::Cancellation,
) -> Result<(), String> {
    let mut profile =
        Profile::open_for(target.home(), None, Some(identity)).map_err(String::from)?;
    let edit = profile.begin_pending_edit()?;
    // A registry hold waits for its author's update; only local and source bytes are retried.
    let (title, digest) = edit
        .held(id)
        .filter(|hold| {
            !matches!(hold.source(), managed_play::HeldSource::Registry { .. })
                && hold.matches_archive(target.home())
        })
        .map(|hold| {
            let digest: String = hold.digest().iter().map(|b| format!("{b:02x}")).collect();
            (hold.title().to_owned(), digest)
        })
        .ok_or_else(|| held_change_refusal(id).to_string())?;
    let next = edit.candidate()?;
    let packages = inventory(next.path())?;
    let seen = packages
        .get(id)
        .ok_or_else(|| held_change_refusal(id).to_string())?;
    let manifest = &seen.package.manifest;
    // These exact bytes stay off once taken down: the signed index's digest takedowns reach local
    // and source copies, so it is always asked, and an unreadable index refuses rather than
    // skipping the check.
    let catalog = crate::browser::sunrise::v2::change_index().map_err(|error| {
        trf(
            "Couldn't check {mod} against the registry's takedowns: {error}",
            &[("mod", &title), ("error", &error.to_string())],
        )
    })?;
    let takedown = catalog
        .index
        .revoked_by_sha256(&digest)
        .map(|row| row.reason.clone());
    if let Some(reason) = takedown {
        return Err(trf(
            "{mod} was taken down: {reason}",
            &[("mod", &title), ("reason", &reason)],
        ));
    }
    if manifest.side == Side::Server {
        return Err(trf(
            "{mod} runs only on servers, so it can't be turned on here.",
            &[("mod", &manifest.name)],
        ));
    }
    if manifest.api != sunrise_launcher_smod::manifest::API {
        return Err(trf(
            "{mod} needs mod API {api}, which this launcher doesn't run.",
            &[("mod", &manifest.name), ("api", &manifest.api)],
        ));
    }
    mutations::check_activation(next.path(), None, &[id])?;
    mod_list::set_all_enabled(next.path(), &[id], true)?;
    if let Some(conflict) =
        manifest_conflict(next.path(), Side::Both, |_| false).map_err(|error| error.to_string())?
    {
        return Err(conflict.reason);
    }
    let proof = edit
        .prove(
            next.path(),
            target.native_profile(),
            cancellation,
            |_| {},
            None,
        )
        .map_err(String::from)?;
    edit.publish_install(
        proof,
        cancellation,
        &BTreeSet::from([id.to_owned()]),
        || Ok(()),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
