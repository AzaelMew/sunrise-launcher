//! GitHub Add and checked Update use the same admitted candidate and publication as registry installs.
use super::*;
use crate::github_mods;
use cap_fs_ext::MetadataExt;
use std::io::ErrorKind;

/// Explicitly replace an existing GitHub release archive. The reviewed path and digest
/// are the installed predecessor, not metadata from the update check. No fresh Add here.
pub fn update_github(
    instance: &Instance,
    identity: Identity,
    found: &github_mods::Resolved,
    packed: &github_mods::Packed,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: InstallHooks<'_>,
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<github_mods::Origin, InstallError> {
    let replacement = packed.release_origin(found).ok_or_else(|| {
        InstallError::Refused(tr("GitHub Update needs a checked release asset.").into())
    })?;
    edit_github(
        instance,
        identity,
        &[(packed, replacement)],
        choose,
        hooks,
        Some(approve),
    )?;
    Ok(replacement.clone())
}

/// Install all selected packages together. Fresh Add never silently upgrades or claims an
/// existing file, even when it came from the same repository; use checked Update for that.
pub fn add_github(
    instance: &Instance,
    identity: Identity,
    found: &github_mods::Resolved,
    selected: &[github_mods::Packed],
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: InstallHooks<'_>,
) -> Result<(), InstallError> {
    let packages = selected
        .iter()
        .map(|package| {
            let origin = package.checked_origin(found).ok_or_else(|| {
                InstallError::Refused(tr("Couldn't verify the selected GitHub package.").into())
            })?;
            Ok((package, origin))
        })
        .collect::<Result<Vec<_>, InstallError>>()?;
    edit_github(instance, identity, &packages, choose, hooks, None)
}

fn edit_github(
    instance: &Instance,
    identity: Identity,
    packages: &[(&github_mods::Packed, &github_mods::Origin)],
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: InstallHooks<'_>,
    approve: Option<&dyn Fn(&ChangeImpact) -> bool>,
) -> Result<(), InstallError> {
    if instance.kind == InstanceKind::Pack {
        return Err(InstallError::Refused(
            tr("This pack controls which mods it installs.").into(),
        ));
    }
    if packages.is_empty() {
        return Err(InstallError::Cancelled);
    }
    (hooks.progress)(InstallProgress::Checking);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let file = |id: &str| format!("{}/{id}.smod", native_mods::INSTALLED);
    let predecessor = {
        let _home = crate::browser::lock_home(&instance.home).map_err(|e| at(&instance.home, e))?;
        if approve.is_some() {
            let (_, replacement) = packages[0];
            let id = &replacement.id;
            let prior = installed_release(&instance.home, id, &file(id))?;
            if !prior.owner.eq_ignore_ascii_case(&replacement.owner)
                || !prior.repo.eq_ignore_ascii_case(&replacement.repo)
            {
                return Err(InstallError::Refused(
                    tr("Choose a release from the same GitHub repository as the installed mod.")
                        .into(),
                ));
            }
            if prior.sha256.eq_ignore_ascii_case(&replacement.sha256) {
                return Err(InstallError::Refused(
                    tr("That GitHub archive is already installed. Nothing was changed.").into(),
                ));
            }
            Some(prior)
        } else {
            check_fresh(&instance.home, packages)?;
            None
        }
    };
    let edit = profile.begin_edit(
        choose,
        |f| (hooks.progress)(InstallProgress::Downloading(f)),
        &|_| (hooks.progress)(InstallProgress::Preparing),
    )?;
    let old = edit.candidate()?;
    let next = edit.candidate()?;
    let home = next.path();
    // Add can't change another package, its enabled bit, or a hold. This is the same no-drop
    // case as a fresh registry Install; the graph must fit without switching anything off.
    if predecessor.is_none() {
        check_fresh(home, packages)?;
        if let Some((_, origin)) = packages
            .iter()
            .find(|(_, origin)| edit.held(&origin.id).is_some())
        {
            return Err(held_change_refusal(&origin.id));
        }
    }
    let before = inventory(old.path(), None)?;
    let (prior, hold) = if let Some(origin) = &predecessor {
        let id = &origin.id;
        let prior = before.get(id).ok_or_else(|| changed(id))?;
        if prior.github_origin(old.path()).as_ref() != Some(origin) {
            return Err(changed(id).into());
        }
        let hold = edit.held(id);
        if hold.is_some_and(|held| {
            !matches!(held.source(), managed_play::HeldSource::Github { .. })
                || !prior.matches_hold(old.path(), edit.base(), held)
        }) {
            return Err(held_change_refusal(id));
        }
        (Some(prior), hold)
    } else {
        (None, None)
    };
    fs::create_dir_all(home.join(native_mods::INSTALLED)).map_err(|e| at(home, e))?;
    let mut off_trial = None;
    for (packed, origin) in packages {
        let id = &origin.id;
        let was_enabled = mod_list::is_enabled(old.path(), id);
        let enabled =
            prior.is_none() || hold.map_or(was_enabled, managed_play::Held::enabled_before);
        if hold.is_some() && !enabled {
            off_trial = Some(id);
        }
        packed.copy_verified(&home.join(file(id)))?;
        let sidecar = github_mods::origin_path(home, id);
        let bytes = serde_json::to_vec_pretty(origin).map_err(|e| at(&sidecar, e))?;
        fs::write(&sidecar, bytes).map_err(|e| at(&sidecar, e))?;
        if enabled != was_enabled {
            mod_list::set_enabled(home, id, enabled)?;
        }
    }
    let catalogue = super::super::fetched_index();
    let check_graph = || match manifest_conflict(home, catalogue.as_deref(), None, |_| false)? {
        Some(conflict) => Err(InstallError::Refused(conflict.reason)),
        None => Ok(()),
    };
    check_graph()?;
    let reviewed_inputs = managed_play::inputs(home, None)?;
    let review =
        if let (Some(approve), Some(prior), Some(predecessor)) = (approve, prior, &predecessor) {
            let id = &predecessor.id;
            let (_, replacement) = packages[0];
            let (suspect, unchecked) = mutations::update_worlds(home, [&prior.package])?;
            let impact = ChangeImpact::read(
                &instance.home,
                mutations::Change::GithubUpdate {
                    target: mutations::ReviewedMod {
                        name: prior.package.manifest.name.clone(),
                        id: id.clone(),
                    },
                    file: file(id),
                    releases: [predecessor.into(), replacement.into()],
                },
                &suspect,
            )?
            .with_unchecked_update(unchecked);
            // Update reviews the checked replacement before any code from it can build.
            (hooks.progress)(InstallProgress::Reviewing);
            if !approve(&impact) {
                return Err(InstallError::Cancelled);
            }
            Some((suspect, impact.into_evidence()))
        } else {
            None
        };
    let unchanged_candidate = || {
        if managed_play::inputs(home, None)? != reviewed_inputs {
            return Err(
                tr("The staged GitHub packages changed. Nothing was changed; try again.")
                    .to_owned(),
            );
        }
        Ok(())
    };
    let cancellation = native_mods::Cancellation::default();
    if backend::NativeGrant::read()?.is_none()
        && !(hooks.native)(&NativeQuestion {
            names: packages.iter().map(|(p, _)| p.name.clone()).collect(),
            reviewed: false,
            retry: false,
        })?
    {
        return Err(InstallError::Cancelled);
    }
    cancellation
        .admit_native_work()
        .map_err(|e| InstallError::Unproved(e.into()))?;
    let prove = || {
        prove_edit(&edit, home, instance, &cancellation, &hooks).map_err(|e| match &e {
            native_mods::Failed::Mods { left, .. }
                if left.iter().any(|item| {
                    matches!(item.why, native_mods::Why::Unfit(_))
                        && packages.iter().any(|(_, origin)| {
                            item.id == origin.id && item.version == origin.version
                        })
                }) =>
            {
                InstallError::NotForVersion(e.into())
            }
            native_mods::Failed::Mods {
                compiling: true, ..
            } => InstallError::GithubBuild(e.into()),
            _ => InstallError::Unproved(e.into()),
        })
    };
    if let Some(id) = off_trial {
        // Prove the off replacement would build, but do not publish a different switch or image.
        let list = home.join(sunrise_launcher_smod::mod_list::FILE);
        let selection = match fs::read(&list) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err(at(&list, e).into()),
        };
        unchanged_candidate()?;
        mod_list::set_enabled(home, id, true)?;
        check_graph()?;
        prove()?;
        match selection {
            Some(bytes) => fs::write(&list, bytes),
            None => fs::remove_file(&list),
        }
        .map_err(|e| at(&list, e))?;
    }
    unchanged_candidate()?;
    let proof = prove()?;
    let retired = hold.into_iter().map(|h| h.id().to_owned()).collect();
    (hooks.progress)(InstallProgress::Publishing);
    edit.publish_install(proof, &cancellation, &retired, || {
        unchanged_candidate()?;
        // The journal holds the home lock and compares the input digests. Check claims and
        // aliases here as well: those can change without changing the copied files' bytes.
        if let (Some(predecessor), Some((suspect, evidence))) = (&predecessor, &review) {
            let id = &predecessor.id;
            if installed_release(&instance.home, id, &file(id)).map_err(|e| e.to_string())?
                != *predecessor
            {
                return Err(changed(id));
            }
            mutations::bind_worlds(&instance.home, suspect, evidence).map(Some)
        } else {
            check_fresh(&instance.home, packages).map_err(|e| e.to_string())?;
            Ok(None)
        }
    })?;
    Ok(())
}

fn check_fresh(
    home: &Path,
    packages: &[(&github_mods::Packed, &github_mods::Origin)],
) -> Result<(), InstallError> {
    let claims = browser_state::Installed::read(home).map_err(|e| e.details(trf))?;
    let present = inventory(home, None)?;
    let mut ids = BTreeSet::new();
    for (_, origin) in packages {
        let id = &origin.id;
        let collision = || {
            InstallError::Refused(trf(
            "{mod} is already installed. Use Update for a GitHub release or remove the existing mod first.",
            &[("mod", id)],
        ))
        };
        if !ids.insert(id)
            || present.contains_key(id)
            || claims.single(id).map_err(|e| e.render(trf))?.is_some()
        {
            return Err(collision());
        }
        for suffix in ["smod", "origin.json"] {
            let relative = format!("{}/{id}.{suffix}", native_mods::INSTALLED);
            claims
                .check_other_claims(home, id, &relative)
                .map_err(|e| e.render(trf))?;
            let path = home.join(&relative);
            match fs::symlink_metadata(&path) {
                Ok(_) => return Err(collision()),
                Err(e) if e.kind() == ErrorKind::NotFound => (),
                Err(e) => return Err(at(&path, e).into()),
            }
        }
    }
    Ok(())
}

fn installed_release(
    home: &Path,
    id: &str,
    file: &str,
) -> Result<github_mods::Origin, InstallError> {
    let refusal = || {
        InstallError::Refused(trf(
            "Couldn't verify an installed GitHub release asset for {mod}. Nothing was changed.",
            &[("mod", id)],
        ))
    };
    let claims = browser_state::Installed::read(home).map_err(|e| e.details(trf))?;
    if claims.single(id).map_err(|e| e.render(trf))?.is_some() {
        return Err(refusal());
    }
    let sidecar = format!("{}/{id}.origin.json", native_mods::INSTALLED);
    for relative in [file, sidecar.as_str()] {
        claims
            .check_other_claims(home, id, relative)
            .map_err(|e| e.render(trf))?;
    }
    let packages = inventory(home, None)?;
    let origin = packages
        .get(id)
        .and_then(|p| p.github_origin(home))
        .filter(github_mods::Origin::is_release_asset)
        .ok_or_else(refusal)?;
    // The candidate copies lose hard-link identity, and case-insensitive filesystems can
    // open a differently spelled sidecar through the canonical path. Require exact leaves.
    for relative in [file, sidecar.as_str()] {
        let path = home.join(relative);
        let leaf = path.file_name().expect("an installed leaf");
        let parent = path.parent().expect("an installed directory");
        let mut exact = false;
        for entry in fs::read_dir(parent).map_err(|e| at(parent, e))? {
            let entry = entry.map_err(|e| at(parent, e))?;
            let name = entry.file_name();
            if name.eq_ignore_ascii_case(leaf) {
                if name != leaf {
                    return Err(refusal());
                }
                exact = true;
            }
        }
        let open = fs::File::open(&path).map_err(|e| at(&path, e))?;
        let meta = cap_std::fs::File::from_std(open)
            .metadata()
            .map_err(|e| at(&path, e))?;
        if !exact || meta.nlink() != 1 {
            return Err(refusal());
        }
    }
    Ok(origin)
}

fn changed(id: &str) -> String {
    trf(
        "The installed GitHub archive or origin for {mod} changed. Nothing was changed; try again.",
        &[("mod", id)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_sidecar_only_owns_its_exact_unaliased_archive() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let file = format!("{}/zoom.smod", native_mods::INSTALLED);
        let package = home.join(&file);
        let receipt = github_mods::origin_path(home, "zoom");
        let commit = "a".repeat(40);
        let bytes = sunrise_launcher_smod::Package::write_files(
            &format!("id = 'zoom'\nname = 'Zoom'\nversion = '1.2.0'\napi = '0.2'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nsunrise = '{commit}'\nmixins = '{commit}'\n"),
            &[("Cargo.toml", b"[package]\nname = 'zoom'\nversion = '1.2.0'\n"), ("src/lib.rs", b"pub fn mod_hooks() {}")],
        ).unwrap();
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, &bytes).unwrap();
        assert!(installed_release(home, "zoom", &file).is_err());
        let mut origin = github_mods::Origin {
            id: "zoom".into(),
            version: "1.2.0".into(),
            owner: "owner".into(),
            repo: "zoom".into(),
            tag: Some("v1.2.0".into()),
            commit,
            asset: Some("zoom-1.2.0.smod".into()),
            sha256: "0".repeat(64),
        };
        let write_origin = |origin: &github_mods::Origin| {
            fs::write(&receipt, serde_json::to_vec(origin).unwrap()).unwrap();
        };
        write_origin(&origin);
        assert!(installed_release(home, "zoom", &file).is_err());
        origin.sha256 = index::sha256(&bytes);
        write_origin(&origin);
        assert_eq!(installed_release(home, "zoom", &file).unwrap(), origin);
        let alias = home.join("alias");
        for path in [&package, &receipt] {
            fs::hard_link(path, &alias).unwrap();
            assert!(installed_release(home, "zoom", &file).is_err());
            fs::remove_file(&alias).unwrap();
        }
        let mut doc = Document::read(home).unwrap();
        doc.rows
            .entries
            .push(serde_json::json!({"project": "other", "file": file}));
        doc.write(home).unwrap();
        assert!(installed_release(home, "zoom", &file).is_err());
    }
}
