//! Publish native installations before exposing their completion marker.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn install(
    base: &str,
    version: &str,
    target: Target,
    binary: &Path,
    libraries: &[workspace::Library],
    about: &serde_json::Value,
    mods: &[Bundle],
    cancellation: &Cancellation,
) -> Result<(), String> {
    let _publication = crate::run_leases::installation().map_err(cache_error)?;
    let from = backend::version_dir(base);
    let partial = target.directory(&format!(".{version}.partial"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    copy_base(&from, &partial, target, target == Target::Client)?;
    let installed = partial.join(target.binary());
    fs::copy(binary, &installed).map_err(|e| format!("The built game: {e}"))?;
    // The prebuilt game's libraries load from beside the executable.
    for library in libraries {
        library.copy_into(&partial).map_err(|error| {
            let _ = append_log(&error.to_string());
            tr("A verified native library couldn't be copied. The build wasn't installed.")
                .to_owned()
        })?;
    }
    fs::write(partial.join("native-mods.json"), about.to_string()).map_err(|e| e.to_string())?;
    for bundle in mods {
        copy_bundle(
            bundle,
            &partial
                .join(sunrise_launcher_smod::mod_list::NATIVE_DIR)
                .join(&bundle.manifest.id),
        )?;
    }
    let _permission = match cancellation.publication_guard() {
        Ok(permission) => permission,
        Err(cancelled) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(cancelled);
        }
    };
    let dir = target.directory(version);
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&partial, &dir).map_err(|e| e.to_string())?;
    crate::run_leases::publish(&dir).map_err(cache_error)?;
    fs::write(dir.join(".installed"), "").map_err(|e| e.to_string())
}
