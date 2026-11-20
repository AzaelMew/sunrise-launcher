//! The cache can be shared by launchers with different SUNRISE_HOME values. The
//! publication lock covers this roster, journal/pointer visibility and deletion.
use super::{build, durable, unsettled};
use crate::backend;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

const FILE: &str = "profile-artifact-roots.json";

fn read(cache: &Path) -> Result<BTreeSet<PathBuf>, String> {
    let Some(bytes) = build::optional_bytes(&cache.join(FILE))? else {
        return Ok(BTreeSet::new());
    };
    let roots: BTreeSet<PathBuf> = serde_json::from_slice(&bytes).map_err(|_| unsettled())?;
    if roots.iter().any(|root| !root.is_absolute()) {
        return Err(unsettled());
    }
    Ok(roots)
}

/// Register the actual game root before a managed profile exposes a reference.
pub(super) fn register(profile: &Path) -> Result<(), String> {
    let root = backend::game_home();
    let is_managed = root.canonicalize().is_ok_and(|root| {
        profile.canonicalize().is_ok_and(|profile| {
            profile.strip_prefix(root).is_ok_and(|relative| {
                match relative.iter().collect::<Vec<_>>().as_slice() {
                    [] => true,
                    [collection, _] => *collection == "instances",
                    [collection, _, home] => *collection == "sunrise-packs" && *home == "home",
                    _ => false,
                }
            })
        })
    });
    if !is_managed {
        // Unit fixtures may stage isolated homes outside the configured game root.
        // Production entry points may never publish into an unscannable profile.
        return if cfg!(test) { Ok(()) } else { Err(unsettled()) };
    }
    register_root(&root)
}

pub(crate) fn register_root(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|e| at(root, e))?;
    build::require_dir(&root)?;
    if cfg!(test) && backend::game_home().canonicalize().as_ref().ok() != Some(&root) {
        return Ok(());
    }
    let cache = backend::data_dir();
    let mut roots = read(&cache)?;
    if roots.insert(root) {
        write(&cache, &roots)?;
    }
    Ok(())
}

/// A relocated cache is a new copy, not shared with the other users of the old cache.
/// Register the copied game root before making the new location selectable.
pub(crate) fn register_relocated(cache: &Path, game: &Path) -> Result<(), String> {
    if read(cache)?.is_empty() {
        return Ok(());
    }
    let game = game.canonicalize().map_err(|e| at(game, e))?;
    build::require_dir(&game)?;
    write(cache, &BTreeSet::from([game]))
}

fn write(cache: &Path, roots: &BTreeSet<PathBuf>) -> Result<(), String> {
    let bytes = serde_json::to_vec(roots).map_err(|e| e.to_string())?;
    let path = cache.join(FILE);
    durable::replace(cache, &path, &bytes).map_err(|e| at(&path, e))
}

pub(super) fn any(
    mut references: impl FnMut(&Path) -> Result<bool, String>,
) -> Result<bool, String> {
    let roots = read(&backend::data_dir())?;
    for root in &roots {
        // A vanished mount, permission change or redirect is unknown, not empty.
        if root.canonicalize().map_err(|e| at(root, e))? != *root {
            return Err(unsettled());
        }
        build::require_dir(root)?;
        fs::read_dir(root).map_err(|e| at(root, e))?;
        if references(root)? {
            return Ok(true);
        }
    }
    // Also protect a previously initialized profile in this invocation's home, even
    // before its first new publish has registered that home with the cache.
    let current = backend::game_home();
    match current.canonicalize() {
        Ok(root) if roots.contains(&root) => Ok(false),
        Ok(root) => references(&root),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(at(&current, e)),
    }
}

fn at(path: &Path, e: io::Error) -> String {
    format!("{}: {e}", path.display())
}
