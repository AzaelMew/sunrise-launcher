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
    instance: &Instance,
    identity: Identity,
    packages: &[LocalPackage],
    cancellation: &native_mods::Cancellation,
    progress: &dyn Fn(InstallProgress),
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<Vec<String>, InstallError> {
    if instance.kind == InstanceKind::Pack {
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
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    check_regular_mod_dirs(&instance.home)?;
    let edit = profile.begin_pending_edit()?;
    let next = edit.candidate()?;
    let active = edit.current();
    let staged = stage_and_review(
        &instance.home,
        next.path(),
        active,
        packages,
        cancellation,
        progress,
        approve,
    )?;
    let checked_inputs = managed_play::inputs(next.path(), None)?;
    check_cancelled(cancellation)?;
    progress(InstallProgress::Publishing);
    let result = edit.publish_pending(next.path(), checked_inputs, cancellation, || {
        revalidate_claims(&instance.home, &staged.claim_checks)?;
        if let Some((suspect, reviewed)) = &staged.worlds {
            mutations::bind_worlds(&instance.home, suspect, reviewed).map(Some)
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
        let before = managed_play::inputs(home, None)?;
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
        if managed_play::inputs(next.path(), None)? != before
            || managed_play::inputs(home, None)? != before
        {
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
    let checked_inputs = managed_play::inputs(next.path(), None)?;
    check_cancelled(cancellation)?;
    progress(InstallProgress::Publishing);
    let result = (|| {
        let _lock =
            sunrise_launcher_smod::install_lock::lock_home(home).map_err(|e| at(home, e))?;
        crate::browser::installed::recover_game_install(home).map_err(String::from)?;
        if managed_play::inputs(home, None)? != before
            || managed_play::inputs(next.path(), None)? != checked_inputs
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
            if managed_play::inputs(home, None)? != before
                || managed_play::inputs(next.path(), None)? != checked_inputs
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
    let before = inventory(candidate, None)?;
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
        let dest_rel = format!("{dest_dir}/{id}.smod");
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
    let _ = inventory(candidate, None)?;
    if let Some(conflict) = manifest_conflict(candidate, None, None, Side::Both, |_| false)? {
        return Err(InstallError::Refused(conflict.reason));
    }

    let worlds = if replaced_ids.is_empty() {
        None
    } else {
        let replaced = replaced_ids
            .iter()
            .filter_map(|id| before.get(id).map(|seen| &seen.package))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{installed, mod_list as browser_mod_list};
    use crate::github_mods::{read_local_folder, read_local_smod, Origin};
    use std::sync::atomic::{AtomicBool, Ordering};
    use sunrise_launcher_smod::Package;

    fn content_smod(dir: &Path, file: &str, id: &str, name: &str, version: &str) -> PathBuf {
        let path = dir.join(file);
        let manifest = format!(
            "id = '{id}'\nname = '{name}'\nversion = '{version}'\napi = '0.2'\nside = 'both'\n"
        );
        let bytes = Package::write_files(&manifest, &[]).unwrap();
        fs::write(&path, &bytes).unwrap();
        path
    }

    #[test]
    fn failed_batch_leaves_existing_install_intact() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        fs::create_dir_all(&home).unwrap();

        let v1_path = content_smod(dir.path(), "alpha-1.smod", "alpha", "Alpha", "1.0.0");
        let v1_bytes = fs::read(&v1_path).unwrap();
        let v1 = read_local_smod(&v1_path).unwrap();
        install_local_at(
            &home,
            &[v1],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap();

        // Registry claims `beta`, so a batch replacing `alpha` + adding `beta` must fail cleanly
        // without touching `alpha`'s live archive or enabled state.
        let tracked = installed::Installed {
            entries: vec![serde_json::from_value(serde_json::json!({
                "source": "sunrise",
                "kind": "mod",
                "project": "beta",
                "slug": "beta",
                "title": "Beta",
                "version": "1.0.0",
                "number": "1.0.0",
                "file": format!("{}/beta.smod", smod_mod_list::DIR),
                "checksum": "",
                "page": "",
            }))
            .unwrap()],
        };
        tracked.save(&home).unwrap();

        let v2_path = content_smod(dir.path(), "alpha-2.smod", "alpha", "Alpha", "2.0.0");
        let beta_path = content_smod(dir.path(), "beta.smod", "beta", "Beta", "1.0.0");
        let v2 = read_local_smod(&v2_path).unwrap();
        let beta = read_local_smod(&beta_path).unwrap();

        let err = install_local_at(
            &home,
            &[v2.clone(), beta],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Refused(_)), "{err:?}");
        assert_eq!(
            fs::read(home.join(smod_mod_list::DIR).join("alpha.smod")).unwrap(),
            v1_bytes
        );

        // Declining the replacement world-review also leaves the old install intact.
        let reviewed = AtomicBool::new(false);
        let err = install_local_at(
            &home,
            &[v2],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| {
                reviewed.store(true, Ordering::SeqCst);
                false
            },
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Cancelled), "{err:?}");
        assert!(reviewed.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(home.join(smod_mod_list::DIR).join("alpha.smod")).unwrap(),
            v1_bytes
        );
    }

    #[test]
    fn cancelling_before_publication_leaves_profile_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        fs::create_dir_all(&home).unwrap();

        let v1_path = content_smod(dir.path(), "alpha-1.smod", "alpha", "Alpha", "1.0.0");
        let v1_bytes = fs::read(&v1_path).unwrap();
        let v1 = read_local_smod(&v1_path).unwrap();
        install_local_at(
            &home,
            &[v1],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap();

        let v2_path = content_smod(dir.path(), "alpha-2.smod", "alpha", "Alpha", "2.0.0");
        let v2 = read_local_smod(&v2_path).unwrap();
        let cancel = native_mods::Cancellation::default();
        let cancel_on_review = cancel.clone();
        let err = install_local_at(&home, &[v2], &cancel, &|_| {}, &move |_| {
            cancel_on_review.stop();
            true
        })
        .unwrap_err();
        assert!(matches!(err, InstallError::Cancelled), "{err:?}");
        assert_eq!(
            fs::read(home.join(smod_mod_list::DIR).join("alpha.smod")).unwrap(),
            v1_bytes
        );
    }

    #[test]
    fn replaces_differently_named_predecessor_by_manifest_id_and_refuses_github_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        let mods = home.join(smod_mod_list::DIR);
        fs::create_dir_all(&mods).unwrap();

        // Predecessor installed under a non-canonical filename.
        let old_path = content_smod(dir.path(), "renamed-v1.smod", "lantern", "Lantern", "1.0.0");
        fs::copy(&old_path, mods.join("custom-name-v1.smod")).unwrap();
        browser_mod_list::set_enabled(&home, "lantern", false).unwrap();

        let v2_path = content_smod(dir.path(), "lantern-v2.smod", "lantern", "Lantern", "2.0.0");
        let v2 = read_local_smod(&v2_path).unwrap();
        let reviewed = AtomicBool::new(false);
        install_local_at(
            &home,
            &[v2],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| {
                reviewed.store(true, Ordering::SeqCst);
                true
            },
        )
        .unwrap();

        assert!(reviewed.load(Ordering::SeqCst));
        assert!(!mods.join("custom-name-v1.smod").exists());
        assert!(mods.join("lantern.smod").is_file());
        assert!(!browser_mod_list::is_enabled(&home, "lantern"));

        // Refuses to overwrite an existing GitHub native mod with a local package.
        let native_dir = home.join(smod_mod_list::NATIVE_DIR);
        fs::create_dir_all(&native_dir).unwrap();
        let manifest = format!(
            "id = 'gh_mod'\nname = 'GH Mod'\nversion = '1.0.0'\napi = '0.2'\nside = 'both'\n[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
            "a".repeat(40),
            "b".repeat(40),
        );
        let bytes = Package::write_files(
            &manifest,
            &[
                (
                    "Cargo.toml",
                    b"[package]\nname='gh_mod'\nversion='1.0.0'\nedition='2024'\n",
                ),
                ("src/lib.rs", b"pub fn f() {}\n"),
            ],
        )
        .unwrap();
        fs::write(native_dir.join("gh_mod.smod"), &bytes).unwrap();
        let gh_origin = Origin {
            id: "gh_mod".into(),
            version: "1.0.0".into(),
            owner: "alice".into(),
            repo: "gh-mod".into(),
            tag: Some("v1.0.0".into()),
            commit: "a".repeat(40),
            asset: Some("gh_mod.smod".into()),
            sha256: sunrise_launcher_smod::index::sha256(&bytes),
            source: None,
            added_from: Some(crate::github_mods::AddedFrom::Github),
        };
        fs::write(
            native_dir.join("gh_mod.origin.json"),
            serde_json::to_vec(&gh_origin).unwrap(),
        )
        .unwrap();

        let local_copy = content_smod(dir.path(), "gh-local.smod", "gh_mod", "GH Mod", "1.1.0");
        let local_pkg = read_local_smod(&local_copy).unwrap();
        let err = install_local_at(
            &home,
            &[local_pkg],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Refused(_)), "{err:?}");
        assert_eq!(fs::read(native_dir.join("gh_mod.smod")).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_mod_directory() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, home.join(smod_mod_list::DIR)).unwrap();

        let path = content_smod(dir.path(), "alpha.smod", "alpha", "Alpha", "1.0.0");
        let pkg = read_local_smod(&path).unwrap();
        let err = install_local_at(
            &home,
            &[pkg],
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Refused(_)), "{err:?}");
        assert!(!outside.join("alpha.smod").exists());
    }

    #[test]
    fn removing_content_folder_mod_removes_its_verified_origin_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("camp-lanterns");
        let home = dir.path().join("profile");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(
            repo.join("sunrise-mod.toml"),
            "id = 'camp_lanterns'\nname = 'Camp Lanterns'\nversion = '0.4.0'\napi = '0.2'\nside = 'both'\n",
        )
        .unwrap();

        let packages = read_local_folder(&repo).unwrap();
        install_local_at(
            &home,
            &packages,
            &native_mods::Cancellation::default(),
            &|_| {},
            &|_| true,
        )
        .unwrap();

        let mods = home.join(smod_mod_list::DIR);
        assert!(mods.join("camp_lanterns.smod").is_file());
        assert!(mods.join("camp_lanterns.origin.json").is_file());

        let present = inventory(&home, None).unwrap();
        let staged =
            mutations::stage_removal(&home, &home, None, ["camp_lanterns"], &present).unwrap();
        assert_eq!(staged.archives.len(), 1);
        assert!(!mods.join("camp_lanterns.smod").exists());
        assert!(!mods.join("camp_lanterns.origin.json").exists());
    }
}
