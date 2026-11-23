//! Local `.smod` files and local mod folders added via "Add from source".
//! Nothing here trusts the mod: the caller shows the unreviewed-code warning first,
//! and native mods still go through the launcher's native-code permission and build flow.
use super::{sha256, Origin, DEPTH, MAX_ASSET};
use crate::i18n::{tr, trf};
use crate::native_mods;
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sunrise_launcher_smod::mod_list;

/// Attestation status shown on the unreviewed warning for a local `.smod` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalAttestation {
    /// Verified GitHub artifact attestation (wired once the backend verifier lands).
    Signed { repo: String },
    /// No verified attestation; Sunrise cannot verify who built the file.
    Unsigned,
}

/// A validated local `.smod` file or packed local folder ready to install after warning consent.
#[derive(Clone, Debug)]
pub struct LocalPackage {
    pub id: String,
    pub version: String,
    pub name: String,
    pub native: bool,
    pub file_name: String,
    pub file: PathBuf,
    pub sha256: String,
    pub attestation: LocalAttestation,
    /// Present for local repository folders (`From source`), absent for local `.smod` files (`Local`).
    pub origin: Option<Origin>,
    _scratch: Arc<tempfile::TempDir>,
}

/// Read and validate one local `.smod` archive into a scratch copy without modifying any profile.
pub fn read_local_smod(path: &Path) -> Result<LocalPackage, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(tr("Choose a .smod file.").into());
    }
    if path.extension().and_then(|e| e.to_str()) != Some("smod") {
        return Err(tr("Only .smod files can be installed here.").into());
    }
    if meta.len() > MAX_ASSET {
        return Err(tr("That .smod file is too large.").into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take(MAX_ASSET + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_ASSET {
        return Err(tr("That .smod file is too large.").into());
    }
    let scratch = Arc::new(tempfile::tempdir().map_err(|e| e.to_string())?);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mod.smod".into());
    validate_staged(bytes, scratch, file_name, LocalAttestation::Unsigned, None)
}

/// Find and pack every valid Sunrise mod folder under `root` into scratch `.smod` files.
pub fn read_local_folder(root: &Path) -> Result<Vec<LocalPackage>, String> {
    let meta = fs::metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(tr("Choose a mod folder.").into());
    }
    let dirs = find_local_mod_dirs(root)?;
    if dirs.is_empty() {
        return Err(trf(
            "No Sunrise mod folder (sunrise-mod.toml) was found in {folder}.",
            &[("folder", &root.display().to_string())],
        ));
    }
    let scratch = Arc::new(tempfile::tempdir().map_err(|e| e.to_string())?);
    let folder_label = root.display().to_string();
    let mut packages = Vec::with_capacity(dirs.len());
    let mut ids = BTreeSet::new();
    for dir in dirs {
        let bytes = sunrise_launcher_smod::Package::pack_directory(&dir)?;
        if bytes.len() as u64 > MAX_ASSET {
            return Err(tr("The packed mod is too large.").into());
        }
        let mut pkg = validate_staged(
            bytes,
            scratch.clone(),
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder_label.clone()),
            LocalAttestation::Unsigned,
            None,
        )?;
        if !ids.insert(pkg.id.clone()) {
            return Err(trf(
                "That folder holds more than one mod with ID {mod}.",
                &[("mod", &pkg.id)],
            ));
        }
        pkg.origin = Some(Origin::local_folder(
            pkg.id.clone(),
            pkg.version.clone(),
            folder_label.clone(),
            pkg.sha256.clone(),
        ));
        packages.push(pkg);
    }
    packages.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(packages)
}

fn validate_staged(
    bytes: Vec<u8>,
    scratch: Arc<tempfile::TempDir>,
    file_name: String,
    attestation: LocalAttestation,
    origin: Option<Origin>,
) -> Result<LocalPackage, String> {
    let sum = sha256(&bytes);
    let staged = scratch.path().join(format!("{}.smod", &sum[..16]));
    fs::write(&staged, &bytes).map_err(|e| format!("{}: {e}", staged.display()))?;
    let package = sunrise_launcher_smod::manifest::Package::read(&staged).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })?;
    let native = package.manifest.native.is_some();
    if native {
        sunrise_launcher_smod::index::native::Bundle::read(&staged).inspect_err(|_| {
            let _ = fs::remove_file(&staged);
        })?;
    }
    Ok(LocalPackage {
        id: package.manifest.id,
        version: package.manifest.version,
        name: package.manifest.name,
        native,
        file_name,
        file: staged,
        sha256: sum,
        attestation,
        origin,
        _scratch: scratch,
    })
}

fn find_local_mod_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
        if native_mods::bundle_in(dir)?.is_some() {
            out.push(dir.to_path_buf());
            return Ok(());
        }
        if dir.join("sunrise-mod.toml").is_file() {
            let text = fs::read_to_string(dir.join("sunrise-mod.toml"))
                .map_err(|e| format!("{}: {e}", dir.join("sunrise-mod.toml").display()))?;
            let manifest = sunrise_launcher_smod::Manifest::parse(&text)?;
            if manifest.native.is_none() {
                out.push(dir.to_path_buf());
                return Ok(());
            }
        }
        if depth == 0 {
            return Ok(());
        }
        let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for sub in dirs {
            walk(&sub, depth - 1, out)?;
        }
        Ok(())
    }
    let mut found = Vec::new();
    walk(root, DEPTH, &mut found)?;
    Ok(found)
}

/// Install validated local `.smod` or local-folder packages into `home`.
///
/// Refuses to overwrite a registry-tracked mod (`mod-browser.json`). An existing untracked local
/// or from-source mod with the same ID is replaced in place so Add from source can refresh it,
/// keeping its enabled state; a newly added mod starts enabled.
pub fn install_local_packages(
    home: &Path,
    packages: &[LocalPackage],
) -> Result<Vec<String>, String> {
    let _guard = crate::browser::lock_home(home).map_err(|e| e.to_string())?;
    let claims = crate::browser::installed::Installed::read(home)?;
    let mut seen = BTreeSet::new();
    for pkg in packages {
        if !seen.insert(pkg.id.as_str()) {
            return Err(trf(
                "Duplicate mod {mod} in the selected packages.",
                &[("mod", &pkg.id)],
            ));
        }
        let dir = if pkg.native {
            mod_list::NATIVE_DIR
        } else {
            mod_list::DIR
        };
        let relative = format!("{dir}/{}.smod", pkg.id);
        let sidecar = format!("{dir}/{}.origin.json", pkg.id);
        let other_dir = if pkg.native {
            mod_list::DIR
        } else {
            mod_list::NATIVE_DIR
        };
        let other_relative = format!("{other_dir}/{}.smod", pkg.id);
        let registry_owns = claims.entries.iter().any(|entry| {
            crate::browser::sunrise::mod_id(entry) == Some(pkg.id.as_str())
                || entry.file == relative
                || entry.file == sidecar
                || entry.file == other_relative
        });
        if registry_owns {
            return Err(trf(
                "{mod} is installed from the Sunrise registry. Remove it first before installing an unreviewed copy.",
                &[("mod", &pkg.name)],
            ));
        }
    }

    let mut installed_names = Vec::with_capacity(packages.len());
    for pkg in packages {
        let bytes = fs::read(&pkg.file).map_err(|e| format!("{}: {e}", pkg.file.display()))?;
        if sha256(&bytes) != pkg.sha256 {
            return Err(trf(
                "The staged package for {mod} changed before install. Nothing was written.",
                &[("mod", &pkg.name)],
            ));
        }
        let target_dir = home.join(if pkg.native {
            mod_list::NATIVE_DIR
        } else {
            mod_list::DIR
        });
        let other_dir = home.join(if pkg.native {
            mod_list::DIR
        } else {
            mod_list::NATIVE_DIR
        });
        fs::create_dir_all(&target_dir).map_err(|e| format!("{}: {e}", target_dir.display()))?;

        let dest = target_dir.join(format!("{}.smod", pkg.id));
        let sidecar = target_dir.join(format!("{}.origin.json", pkg.id));
        let other_dest = other_dir.join(format!("{}.smod", pkg.id));
        let other_sidecar = other_dir.join(format!("{}.origin.json", pkg.id));
        let replacing = dest.exists() || other_dest.exists();

        let tmp = target_dir.join(format!(".{}.{}.tmp", pkg.id, std::process::id()));
        fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
        fs::rename(&tmp, &dest).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{}: {e}", dest.display())
        })?;
        if let Some(origin) = &pkg.origin {
            let json = serde_json::to_vec_pretty(origin).map_err(|e| e.to_string())?;
            let sidecar_tmp =
                target_dir.join(format!(".{}.{}.origin.tmp", pkg.id, std::process::id()));
            fs::write(&sidecar_tmp, &json)
                .map_err(|e| format!("{}: {e}", sidecar_tmp.display()))?;
            fs::rename(&sidecar_tmp, &sidecar).map_err(|e| {
                let _ = fs::remove_file(&sidecar_tmp);
                format!("{}: {e}", sidecar.display())
            })?;
        } else if sidecar.exists() {
            let _ = fs::remove_file(&sidecar);
        }
        if other_dest.exists() {
            let _ = fs::remove_file(&other_dest);
        }
        if other_sidecar.exists() {
            let _ = fs::remove_file(&other_sidecar);
        }
        if !replacing {
            crate::browser::mod_list::set_enabled(home, &pkg.id, true)?;
        }
        installed_names.push(pkg.name.clone());
    }
    Ok(installed_names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{installed, mod_list as browser_mod_list, Kind, SourceId};
    use crate::github_mods::AddedFrom;
    use sunrise_launcher_smod::Package;

    fn content_manifest(id: &str, name: &str, version: &str) -> String {
        format!("id = '{id}'\nname = '{name}'\nversion = '{version}'\napi = '0.2'\nside = 'both'\n")
    }

    #[test]
    fn read_and_install_local_smod_marks_unsigned_and_refuses_registry_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("profile");
        let smod_path = dir.path().join("lantern-tweaks-0.1.0.smod");
        let bytes = Package::write_files(
            &content_manifest("lantern_tweaks", "Lantern Tweaks", "0.1.0"),
            &[],
        )
        .unwrap();
        fs::write(&smod_path, &bytes).unwrap();

        let pkg = read_local_smod(&smod_path).unwrap();
        assert_eq!(pkg.id, "lantern_tweaks");
        assert_eq!(pkg.attestation, LocalAttestation::Unsigned);
        assert!(pkg.origin.is_none());
        assert!(!pkg.native);

        let installed = install_local_packages(&home, &[pkg.clone()]).unwrap();
        assert_eq!(installed, ["Lantern Tweaks"]);
        assert!(home
            .join(mod_list::DIR)
            .join("lantern_tweaks.smod")
            .is_file());
        assert!(!home
            .join(mod_list::DIR)
            .join("lantern_tweaks.origin.json")
            .exists());
        assert!(browser_mod_list::is_enabled(&home, "lantern_tweaks"));

        // Refreshing via Add from source preserves disabled state.
        browser_mod_list::set_enabled(&home, "lantern_tweaks", false).unwrap();
        install_local_packages(&home, &[pkg.clone()]).unwrap();
        assert!(!browser_mod_list::is_enabled(&home, "lantern_tweaks"));

        // Refuses to overwrite a registry-tracked mod.
        let tracked = installed::Installed {
            entries: vec![installed::Entry {
                source: SourceId::Sunrise,
                kind: Kind::Mod,
                project: "lantern-project".into(),
                slug: "lantern-tweaks".into(),
                title: "Lantern Tweaks".into(),
                version: "0.1.0".into(),
                number: "0.1.0".into(),
                file: format!("{}/lantern_tweaks.smod", mod_list::DIR),
                checksum: String::new(),
                page: String::new(),
                dependency: false,
                verdict: None,
                install_hold: None,
            }],
        };
        tracked.save(&home).unwrap();
        assert!(install_local_packages(&home, &[pkg]).is_err());
    }

    #[test]
    fn read_and_install_local_folder_records_manual_from_source_origin() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("camp-lanterns");
        let home = dir.path().join("profile");
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            repo.join("sunrise-mod.toml"),
            content_manifest("camp_lanterns", "Camp Lanterns", "0.4.0"),
        )
        .unwrap();

        let packages = read_local_folder(&repo).unwrap();
        assert_eq!(packages.len(), 1);
        let origin = packages[0].origin.as_ref().unwrap();
        assert!(origin.is_manual());
        assert_eq!(origin.local_folder_path(), Some(repo.to_str().unwrap()));
        assert!(matches!(
            origin.added_from,
            Some(AddedFrom::LocalFolder { .. })
        ));

        install_local_packages(&home, &packages).unwrap();
        let found = crate::native_status::untracked_content(&home);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "camp_lanterns");
        assert!(found[0].origin.as_ref().is_some_and(Origin::is_manual));
    }
}
