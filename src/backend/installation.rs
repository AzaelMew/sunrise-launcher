//! Installation publication excludes concurrent capability snapshots.
use super::*;
use crate::{managed_play::Artifact, profile_txn::durable};
use sunrise_launcher_smod::{
    locks::{LockMode, Release as LockRelease},
    runtime_leases::Registry,
};

mod bound;
pub(crate) use bound::{BoundVariant, GameDownload, GameProof};

const STAGE_PREFIX: &str = ".build-stage-";
const REPAIR: &str = ".repair-";

pub(crate) fn private_stage(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with(STAGE_PREFIX) || name.starts_with(REPAIR))
}

pub(crate) fn valid_archive_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The old tree's move to `previous` is the recovery marker. The replacement commits
/// when the complete, flushed tree becomes visible under its version name.
fn recover_previous(repair: &Path, target: &Path) -> std::io::Result<()> {
    use std::io::ErrorKind;
    let plain = |meta: &fs::Metadata| !crate::native_cache::linked(meta);
    match fs::symlink_metadata(repair) {
        Ok(meta) if meta.is_dir() && plain(&meta) => {}
        Ok(_) => return Err(ErrorKind::InvalidData.into()),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    let complete = |tree: &Path| {
        fs::symlink_metadata(tree.join(COMPLETE)).is_ok_and(|meta| meta.is_file() && plain(&meta))
    };
    let incomplete = match fs::symlink_metadata(target) {
        Ok(meta) if !meta.is_dir() || !plain(&meta) => return Err(ErrorKind::InvalidData.into()),
        Ok(_) if complete(target) => return fs::remove_dir_all(repair),
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let previous = repair.join("previous");
    let previous_meta = fs::symlink_metadata(&previous)?;
    if !previous_meta.is_dir() || !plain(&previous_meta) || !complete(&previous) {
        return Err(ErrorKind::InvalidData.into());
    }
    // A repair from an earlier launcher can leave a visible uncommitted tree.
    if incomplete {
        durable::move_new_directory(target, &repair.join("incomplete"))?;
    }
    durable::move_new_directory(&previous, target)?;
    fs::remove_dir_all(repair)
}

pub(super) fn recover_replacement(tag: &str) -> Result<(), String> {
    if Path::new(tag).file_name() != Some(std::ffi::OsStr::new(tag)) {
        return Err(existing_build(tag));
    }
    let repair = versions_dir().join(format!("{REPAIR}{tag}"));
    match fs::symlink_metadata(&repair) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }
    let registry = Registry::with_release(&data_dir(), LockRelease::Close)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    let _publication = registry
        .publication(LockMode::Exclusive)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    recover_previous(&repair, &version_dir(tag)).map_err(|e| e.to_string())
}

/// Restore complete installations before either the CLI or window resolves profile pins.
pub fn recover_game_installations() {
    let Ok(entries) = fs::read_dir(versions_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(tag) = name.to_str().and_then(|n| n.strip_prefix(REPAIR)) else {
            continue;
        };
        if let Err(error) = recover_replacement(tag) {
            eprintln!("Could not recover game installation {tag}: {error}");
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallIntent {
    /// First Play uses only its saved binding. A different installation stays untouched.
    Exact,
    /// The release install action may repair this exact tag, never a profile's saved binding.
    Repair,
}

pub(crate) fn install_as(
    release: &Release,
    tag: &str,
    sha256: &str,
    requested_architecture: Option<GameArchitecture>,
    release_facts: Option<(&[u8], &str)>,
    intent: InstallIntent,
    progress: impl Fn(f32),
) -> Result<(), String> {
    let (url, size) = release
        .asset
        .clone()
        .ok_or_else(|| format!("{tag} has no build for this platform."))?;
    let root = versions_dir();
    let target = version_dir(tag);
    let matches_archive = || Artifact::matches_archive(&target, tag, sha256);
    if intent == InstallIntent::Exact {
        match fs::symlink_metadata(&target) {
            Ok(_) if matches_archive() => {
                progress(1.0);
                return Ok(());
            }
            Ok(_) => return Err(existing_build(tag)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix(STAGE_PREFIX)
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let archive = stage.path().join("download");
    download(&url, size, sha256, &archive, &progress)?;
    let unpacked = stage.path().join("unpacked");
    unpack(&archive, &url, &unpacked)?;
    progress(0.97);
    // The archives hold one top folder (sunrise-linux-x86_64/...); the version folder is its contents.
    let entries: Vec<_> = fs::read_dir(&unpacked)
        .and_then(|entries| entries.collect::<std::io::Result<_>>())
        .map_err(|e| e.to_string())?;
    let top = match entries.as_slice() {
        [only] if only.path().is_dir() => only.path(),
        _ => unpacked.clone(),
    };
    if !fs::symlink_metadata(&top)
        .is_ok_and(|meta| meta.is_dir() && !crate::native_cache::linked(&meta))
        || !fs::symlink_metadata(top.join(CLIENT))
            .is_ok_and(|meta| meta.is_file() && !crate::native_cache::linked(&meta))
    {
        return Err(format!("The download has no {CLIENT}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(top.join(CLIENT), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    if let (Some(requested), Some(binary)) = (requested_architecture, architecture_for_asset(&url))
    {
        let mut marker =
            fs::File::create_new(top.join(GAME_BUILD_ARCHITECTURE)).map_err(|e| e.to_string())?;
        serde_json::to_writer(
            &mut marker,
            &InstalledGameArchitecture { requested, binary },
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some((bytes, sha256)) = release_facts {
        keep_release_file(&top, bytes, sha256)?;
    }
    Artifact::record_archive(&top, tag, sha256)?;
    crate::nightly::record_install_time(&top, release).map_err(|e| e.to_string())?;
    // Transfer and unpack privately. Only the visibility boundary fences readers.
    let registry = Registry::with_release(&data_dir(), LockRelease::Close)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    let mode = if intent == InstallIntent::Repair {
        LockMode::Exclusive
    } else {
        LockMode::Shared
    };
    let _publication = registry
        .publication(mode)
        .map_err(|e| crate::native_mods::cache_error(e.into()))?;
    let replaced = intent == InstallIntent::Repair
        && replace_installed(
            &registry,
            tag,
            &top,
            &target,
            &root.join(format!("{REPAIR}{tag}")),
        )?;
    if !replaced {
        match crate::run_leases::publish_build(&top, &target) {
            Ok(()) => {}
            Err(crate::native_cache::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                if !matches_archive() {
                    return Err(existing_build(tag));
                }
                progress(1.0);
                return Ok(());
            }
            Err(e) => return Err(crate::native_mods::cache_error(e)),
        }
    }
    if release.nightly.as_deref() == Some(sunrise_launcher_release_verify::NIGHTLY) {
        crate::nightly::record_current(&root, tag).map_err(|e| e.to_string())?;
    }
    // First Play prepares through the profile's staged handoff, not the cache-only
    // import hook (whose detached preparer could otherwise outlive that admission).
    progress(1.0);
    Ok(())
}

/// Reconcile an explicit release install with an existing complete version. The caller
/// holds the exclusive publication fence. `false` means there was no earlier version.
fn replace_installed(
    registry: &std::sync::Arc<Registry>,
    tag: &str,
    top: &Path,
    target: &Path,
    repair: &Path,
) -> Result<bool, String> {
    use crate::native_mods::cache_error;
    recover_previous(repair, target).map_err(|e| e.to_string())?;
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() && !crate::native_cache::linked(&meta) => {}
        Ok(_) => return Err(existing_build(tag)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    }
    // A different image behind a saved exact name would strand its profile.
    crate::managed_play::authorize_cache_removal(tag).map_err(cache_error)?;
    let tree = cap_std::fs::Dir::open_ambient_dir(target, cap_std::ambient_authority())
        .map_err(|e| e.to_string())?;
    let old = registry
        .lock(&tree, LockMode::Exclusive)
        .map_err(|e| cache_error(e.into()))?;
    if in_use(tag) {
        return Err(cache_error(crate::storage::Error::Busy));
    }
    if !fs::symlink_metadata(target.join(COMPLETE))
        .is_ok_and(|meta| meta.is_file() && !crate::native_cache::linked(&meta))
    {
        return Err(existing_build(tag));
    }
    let from = resources_at(target);
    let to = resources_at(top);
    if !to.exists()
        && read_json(&from.join(RESOURCES_MANIFEST))
            .is_some_and(|manifest| resources_compatible(target, top, &manifest))
    {
        copy_dir_except(&from, &to, Some(&from.join(crate::preparer::DATA_FILE)))
            .map_err(|e| e.to_string())?;
    }
    if target.join(AUTO_INSTALLED).is_file() {
        fs::write(top.join(AUTO_INSTALLED), "").map_err(|e| e.to_string())?;
    }
    drop(tree);
    fs::create_dir(repair).map_err(|e| e.to_string())?;
    let publish = durable::move_new_directory(target, &repair.join("previous"))
        .map_err(|e| e.to_string())
        .and_then(|()| crate::run_leases::publish_build(top, target).map_err(cache_error));
    if let Err(error) = publish {
        return match recover_previous(repair, target) {
            Ok(()) => Err(error),
            Err(restore) => Err(format!("{error}; {restore}")),
        };
    }
    if recover_previous(repair, target).is_ok() {
        let _ = old.retire();
    }
    Ok(true)
}

fn existing_build(tag: &str) -> String {
    crate::i18n::trf(
        "A different installation already occupies {build}. It was not replaced.",
        &[("build", tag)],
    )
}
