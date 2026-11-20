//! Installation publication excludes concurrent capability snapshots.
use super::*;

pub(crate) fn install_as(
    release: &Release,
    tag: &str,
    sha256: &str,
    requested_architecture: Option<GameArchitecture>,
    progress: impl Fn(f32),
) -> Result<(), String> {
    let staging = crate::run_leases::installation().map_err(crate::native_mods::cache_error)?;
    let (url, size) = release
        .asset
        .clone()
        .ok_or_else(|| format!("{tag} has no build for this platform."))?;
    let root = versions_dir();
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let work = tempfile::Builder::new()
        .prefix(&format!(".{tag}.partial-"))
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let partial = work.path();
    let archive = partial.join("download");
    download(&url, size, sha256, &archive, &progress)?;
    let unpacked = partial.join("unpacked");
    unpack(&archive, &url, &unpacked)?;
    progress(0.97);
    // The archives hold one top folder (sunrise-linux-x86_64/...); the version folder is its contents.
    let entries: Vec<_> = fs::read_dir(&unpacked)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    let top = match entries.as_slice() {
        [only] if only.path().is_dir() => only.path(),
        _ => unpacked.clone(),
    };
    if !top.join(CLIENT).exists() {
        return Err(format!("The download has no {CLIENT}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(top.join(CLIENT), fs::Permissions::from_mode(0o755));
    }
    if let (Some(requested), Some(binary)) = (requested_architecture, architecture_for_asset(&url))
    {
        fs::write(
            top.join(GAME_BUILD_ARCHITECTURE),
            serde_json::to_vec(&InstalledGameArchitecture { requested, binary })
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    crate::nightly::record_install_time(&top, release).map_err(|e| e.to_string())?;
    // Use an exclusive publication only for the directory switch, not for the download. That
    // makes two installs of the same tag see either the old tree or the new one, never the gap.
    drop(staging);
    use sunrise_launcher_smod::{
        locks::{LockMode, Release},
        runtime_leases::Registry,
    };
    let registry = Registry::with_release(&data_dir(), Release::Close)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    let publication = registry
        .publication(LockMode::Exclusive)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    let target = version_dir(tag);
    let previous = partial.join("previous");
    let old_claim = if target.try_exists().map_err(|e| e.to_string())? {
        let tree = cap_std::fs::Dir::open_ambient_dir(&target, cap_std::ambient_authority())
            .map_err(|e| e.to_string())?;
        let claim = registry
            .lock(&tree, LockMode::Exclusive)
            .map_err(|e| crate::native_mods::cache_error(e.into()))?;
        if in_use(tag) {
            return Err(crate::native_mods::cache_error(crate::storage::Error::Busy));
        }
        // Keep preparation and the automatic-install classification of the same build.
        let from = resources_at(&target);
        let to = resources_at(&top);
        if !to.exists()
            && read_json(&from.join(RESOURCES_MANIFEST))
                .is_some_and(|manifest| resources_compatible(&target, &top, &manifest))
        {
            copy_dir_except(&from, &to, Some(&from.join(crate::preparer::DATA_FILE)))
                .map_err(|e| e.to_string())?;
        }
        if target.join(AUTO_INSTALLED).is_file() {
            fs::write(top.join(AUTO_INSTALLED), "").map_err(|e| e.to_string())?;
        }
        drop(tree);
        // An unmanaged Windows game that started meanwhile can refuse this rename. Never
        // remove files from the old installation one by one in that case.
        fs::rename(&target, &previous).map_err(|e| e.to_string())?;
        Some(claim)
    } else {
        None
    };
    let mut moved = false;
    let publish = (|| {
        fs::rename(&top, &target).map_err(|e| e.to_string())?;
        moved = true;
        crate::run_leases::publish(&target).map_err(crate::native_mods::cache_error)?;
        if release.nightly.as_deref() == Some(sunrise_launcher_release_verify::NIGHTLY) {
            crate::nightly::record_current(&root, tag).map_err(|e| e.to_string())?;
        }
        fs::write(target.join(COMPLETE), "").map_err(|e| e.to_string())
    })();
    if let Err(error) = publish {
        if old_claim.is_some() {
            let restore = if moved {
                fs::rename(&target, &top).and_then(|()| fs::rename(&previous, &target))
            } else {
                fs::rename(&previous, &target)
            };
            if let Err(restore_error) = restore {
                let backup = work.keep().join("previous");
                return Err(format!("{error}; {restore_error}: {}", backup.display()));
            }
        }
        return Err(error);
    }
    if let Some(claim) = old_claim {
        if fs::remove_dir_all(&previous).is_ok() {
            let _ = claim.retire();
        }
    }
    drop(publication);
    reuse_prepared_for(tag);
    progress(1.0);
    Ok(())
}
