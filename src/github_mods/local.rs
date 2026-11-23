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

/// Maximum accepted sidecar `.sigstore.json` attestation bundle size (1 MiB).
const MAX_ATTESTATION: u64 = 1024 * 1024;

/// Attestation status shown on the unreviewed warning for a local `.smod` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalAttestation {
    /// No attestation bundle came with the package; Sunrise cannot verify who built the file.
    Unsigned,
    /// A sidecar `.sigstore.json` bundle verified offline against the staged archive's SHA-256.
    /// Proves the GitHub repository and build workflow, not that Sunrise reviewed the code.
    Verified {
        repository: String,
        workflow: String,
    },
    /// A sidecar `.sigstore.json` bundle was present, but failed verification.
    Failed(String),
}

/// A validated local `.smod` file or packed local folder ready to install after warning consent.
#[derive(Clone, Debug)]
pub struct LocalPackage {
    pub id: String,
    pub version: String,
    pub name: String,
    pub native: bool,
    pub file_name: String,
    /// The file or folder path the player picked on disk.
    pub source_path: PathBuf,
    pub file: PathBuf,
    pub sha256: String,
    pub attestation: LocalAttestation,
    /// Present for local repository folders (`From source`), absent for local `.smod` files (`Local`).
    pub origin: Option<Origin>,
    _scratch: Arc<tempfile::TempDir>,
}

type LocalVerifier<'a> = dyn Fn(&[u8], [u8; 32]) -> Result<crate::attestation::Attested, sunrise_attestation::Error>
    + 'a;

/// Read and validate one local `.smod` archive into a scratch copy without modifying any profile.
/// If a sidecar `<file>.sigstore.json` bundle sits beside it, verify it offline against the staged
/// archive's SHA-256 using the same Sigstore verifier as registry installs.
pub fn read_local_smod(path: &Path) -> Result<LocalPackage, String> {
    read_local_smod_with(path, &crate::attestation::verify_local)
}

fn read_local_smod_with(path: &Path, verify: &LocalVerifier<'_>) -> Result<LocalPackage, String> {
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
    let mut pkg = validate_staged(
        bytes,
        scratch,
        file_name,
        path.to_path_buf(),
        LocalAttestation::Unsigned,
        None,
    )?;
    pkg.attestation = check_sidecar_attestation(path, &pkg.sha256, verify);
    Ok(pkg)
}

fn sidecar_bundle_path(smod_path: &Path) -> PathBuf {
    let mut os = smod_path.as_os_str().to_os_string();
    os.push(".sigstore.json");
    PathBuf::from(os)
}

fn check_sidecar_attestation(
    smod_path: &Path,
    sha256_hex: &str,
    verify: &LocalVerifier<'_>,
) -> LocalAttestation {
    let bundle_path = sidecar_bundle_path(smod_path);
    let file = match fs::File::open(&bundle_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return LocalAttestation::Unsigned;
        }
        Err(err) => {
            return LocalAttestation::Failed(format!("{}: {err}", bundle_path.display()));
        }
    };
    let mut bundle_bytes = Vec::new();
    if let Err(err) = file
        .take(MAX_ATTESTATION + 1)
        .read_to_end(&mut bundle_bytes)
    {
        return LocalAttestation::Failed(format!("{}: {err}", bundle_path.display()));
    }
    if bundle_bytes.len() as u64 > MAX_ATTESTATION {
        return LocalAttestation::Failed(tr("That attestation file is too large.").into());
    }
    let Some(digest) = crate::attestation::parse_sha256_hex(sha256_hex) else {
        return LocalAttestation::Failed("invalid staged archive SHA-256".into());
    };
    match verify(&bundle_bytes, digest) {
        Ok(attested) => LocalAttestation::Verified {
            repository: attested.repository_name,
            workflow: attested.signer_workflow_ref,
        },
        Err(err) => LocalAttestation::Failed(err.to_string()),
    }
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
            dir.clone(),
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
    source_path: PathBuf,
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
        source_path,
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

/// Install validated local `.smod` or local-folder packages into `home` through the checked
/// profile transaction.
///
/// Refuses to overwrite a registry-tracked or GitHub mod. An existing local mod with the same
/// manifest ID is replaced in place (even if stored under a different filename or in the other
/// mod directory), preserving its enabled state; a newly added mod starts enabled.
pub fn install_local_packages(
    home: &Path,
    packages: &[LocalPackage],
) -> Result<Vec<String>, String> {
    if !home.exists() {
        fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
    }
    crate::browser::sunrise::install_local_at(
        home,
        packages,
        &native_mods::Cancellation::default(),
        &|_| {},
        &|_| true,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{installed, mod_list as browser_mod_list};
    use crate::github_mods::AddedFrom;
    use sunrise_launcher_smod::{mod_list, Package};

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
        assert_eq!(pkg.source_path, smod_path);
        assert_eq!(pkg.attestation, LocalAttestation::Unsigned);
        assert!(pkg.origin.is_none());
        assert!(!pkg.native);

        let installed = install_local_packages(&home, std::slice::from_ref(&pkg)).unwrap();
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
        install_local_packages(&home, std::slice::from_ref(&pkg)).unwrap();
        assert!(!browser_mod_list::is_enabled(&home, "lantern_tweaks"));

        // Refuses to overwrite a registry-tracked mod.
        let tracked = installed::Installed {
            entries: vec![serde_json::from_value(serde_json::json!({
                "source": "sunrise",
                "kind": "mod",
                "project": "lantern-project",
                "slug": "lantern-tweaks",
                "title": "Lantern Tweaks",
                "version": "0.1.0",
                "number": "0.1.0",
                "file": format!("{}/lantern_tweaks.smod", mod_list::DIR),
                "checksum": "",
                "page": "",
            }))
            .unwrap()],
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

    #[test]
    fn read_local_folder_preserves_native_uses_json_and_origins_skips_local_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("native-lantern");
        let home = dir.path().join("profile");
        fs::create_dir_all(repo.join("src")).unwrap();
        let manifest = format!(
            "{}[native]\ngame = '26.3'\nsunrise = '{}'\nmixins = '{}'\n",
            content_manifest("native_lantern", "Native Lantern", "0.2.0"),
            "a".repeat(40),
            "b".repeat(40),
        );
        fs::write(repo.join("sunrise-mod.toml"), &manifest).unwrap();
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = 'native_lantern'\nversion = '0.2.0'\nedition = '2024'\n",
        )
        .unwrap();
        fs::write(repo.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        let table = br#"{"schema":1,"made_for":"x","entries":[]}"#;
        fs::write(repo.join(sunrise_launcher_smod::manifest::USES_FILE), table).unwrap();

        let packages = read_local_folder(&repo).unwrap();
        assert_eq!(packages.len(), 1);
        assert!(packages[0].native);
        let carried = Package::entry(
            &packages[0].file,
            sunrise_launcher_smod::manifest::USES_FILE,
            1024,
        )
        .unwrap();
        assert_eq!(carried.as_deref(), Some(table.as_slice()));

        install_local_packages(&home, &packages).unwrap();
        assert!(home
            .join(mod_list::NATIVE_DIR)
            .join("native_lantern.origin.json")
            .is_file());
        // CLI GitHub updater enumerates only GitHub origins, never local folder receipts.
        assert!(crate::github_mods::origins(&home).is_empty());
    }

    #[test]
    fn read_local_smod_without_sidecar_bundle_is_unsigned() {
        let dir = tempfile::tempdir().unwrap();
        let smod_path = dir.path().join("no-bundle-0.1.0.smod");
        let bytes = Package::write_files(&content_manifest("no_bundle", "No Bundle", "0.1.0"), &[])
            .unwrap();
        fs::write(&smod_path, &bytes).unwrap();

        let pkg = read_local_smod(&smod_path).unwrap();
        assert_eq!(pkg.attestation, LocalAttestation::Unsigned);
    }

    #[test]
    fn read_local_smod_with_mismatched_bundle_reports_failed_attestation() {
        let dir = tempfile::tempdir().unwrap();
        let smod_path = dir.path().join("bad-bundle-0.1.0.smod");
        let bytes =
            Package::write_files(&content_manifest("bad_bundle", "Bad Bundle", "0.1.0"), &[])
                .unwrap();
        fs::write(&smod_path, &bytes).unwrap();
        // A real Sigstore bundle for a different artifact fails real offline verification.
        let bundle =
            include_bytes!("../../crates/attestation/tests/fixtures/ruff-source.sigstore.json");
        fs::write(sidecar_bundle_path(&smod_path), bundle).unwrap();

        let pkg = read_local_smod(&smod_path).unwrap();
        assert!(
            matches!(pkg.attestation, LocalAttestation::Failed(ref msg) if !msg.is_empty()),
            "expected Failed attestation, got {:?}",
            pkg.attestation
        );
    }

    #[test]
    fn read_local_smod_with_valid_bundle_reports_verified_publisher() {
        let dir = tempfile::tempdir().unwrap();
        let smod_path = dir.path().join("good-bundle-0.1.0.smod");
        let bytes = Package::write_files(
            &content_manifest("good_bundle", "Good Bundle", "0.1.0"),
            &[],
        )
        .unwrap();
        fs::write(&smod_path, &bytes).unwrap();
        let expected_sha = sha256(&bytes);
        let expected_digest = crate::attestation::parse_sha256_hex(&expected_sha).unwrap();
        let bundle =
            include_bytes!("../../crates/attestation/tests/fixtures/ruff-source.sigstore.json");
        fs::write(sidecar_bundle_path(&smod_path), bundle).unwrap();

        // Check through the real local reader that the sidecar bundle and the staged archive's
        // SHA-256 are passed to the verifier, and that a verified Attested becomes Verified.
        let pkg = read_local_smod_with(&smod_path, &|got_bundle, got_digest| {
            assert_eq!(got_bundle, bundle);
            assert_eq!(got_digest, expected_digest);
            // Also exercise the real offline Sigstore verifier against the fixture's own digest.
            let fixture_digest = crate::attestation::parse_sha256_hex(
                "0378ef60356caaad7b84653691a5d57a1d8b57fd59d173cfc27b0fd4a91a13f3",
            )
            .unwrap();
            crate::attestation::verify_local(got_bundle, fixture_digest)
        })
        .unwrap();

        assert_eq!(
            pkg.attestation,
            LocalAttestation::Verified {
                repository: "astral-sh/ruff".into(),
                workflow: "astral-sh/ruff/.github/workflows/release.yml@refs/heads/main".into(),
            }
        );
    }
}
