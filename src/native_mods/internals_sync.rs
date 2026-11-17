//! Internals discovery (`sunrise_native_build::discover`) with the launcher's managed Rust and native cache.
use super::rust_toolchain;
use std::path::{Path, PathBuf};
use sunrise_mod_package::internals::ListedInternal;

/// The internals the mod in `folder` names, as the pinned game's catalog lists them.
pub(crate) fn check(folder: &Path) -> Result<Vec<ListedInternal>, String> {
    let rust = rust_toolchain::install(|_| {})?;
    sunrise_native_build::discover::internals(&rust, folder).map_err(|e| e.to_string())
}

/// The pinned game's build data with only this mod's modules applied, copied into `destination` while no native build can replace it.
pub fn catalog(folder: &Path, destination: &Path) -> Result<PathBuf, String> {
    super::require_consent().map_err(|e| e.to_string())?;
    let _lease = crate::native_cache::Cache::acquire(&crate::backend::build_dir())
        .map_err(super::cache_error)?;
    super::clear_log()?;
    let rust = rust_toolchain::install(|_| {})?;
    sunrise_native_build::discover::catalog(&rust, folder, destination).map_err(|e| e.to_string())
}
