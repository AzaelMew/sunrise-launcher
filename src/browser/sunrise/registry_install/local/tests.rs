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

    let present = inventory(&home).unwrap();
    let staged = mutations::stage_removal(&home, &home, None, ["camp_lanterns"], &present).unwrap();
    assert_eq!(staged.archives.len(), 1);
    assert!(!mods.join("camp_lanterns.smod").exists());
    assert!(!mods.join("camp_lanterns.origin.json").exists());
}

#[test]
fn stage_removal_cleans_row_missing_display_field_and_catches_windows_competing_claim() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("profile");
    let mods = home.join(smod_mod_list::DIR);
    fs::create_dir_all(&mods).unwrap();
    let bytes = Package::write_files(
        "id = 'alpha'\nname = 'Alpha'\nversion = '1.0.0'\napi = '0.2'\nside = 'both'\n",
        &[],
    )
    .unwrap();
    fs::write(mods.join("alpha.smod"), &bytes).unwrap();
    // Sunrise row missing display field `page`: cannot deserialize as installed::Entry, so
    // `registry_claim` stays false, yet removal must still clean it up.
    let file = format!("{}/alpha.smod", smod_mod_list::DIR);
    let manifest_path = home.join(installed::MANIFEST);
    fs::write(
        &manifest_path,
        serde_json::to_vec(&serde_json::json!({
            "entries": [{
                "source": "sunrise",
                "kind": "mod",
                "project": "alpha",
                "file": file,
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let present = inventory(&home).unwrap();
    assert!(
        !present["alpha"].registry_claim,
        "missing display field must not grant replacement permission"
    );
    mutations::stage_removal(&home, &home, None, ["alpha"], &present).unwrap();
    assert!(!mods.join("alpha.smod").exists());
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(after["entries"].as_array().unwrap().len(), 0);

    // Now add a competing row with Windows-style `\` separators claiming alpha's receipt.
    fs::write(mods.join("alpha.smod"), &bytes).unwrap();
    let win_file = format!("{}\\alpha.smod", smod_mod_list::DIR);
    fs::write(
        &manifest_path,
        serde_json::to_vec(&serde_json::json!({
            "entries": [{
                "source": "other-source",
                "kind": "mod",
                "project": "other_owner",
                "file": win_file,
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let present = inventory(&home).unwrap();
    let err = mutations::stage_removal(&home, &home, None, ["alpha"], &present)
        .err()
        .expect("competing Windows-style claim must block removal");
    assert!(
        err.to_string().contains("other_owner"),
        "unexpected error: {err}"
    );
}

// Reuses the saved-world helpers of `update_tests`, which build only there.
#[cfg(all(debug_assertions, unix))]
#[test]
fn replacing_a_content_mod_compares_saved_worlds_with_the_original_archive() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("profile");
    fs::create_dir_all(&home).unwrap();
    let block = "data/alpha/sunrise/block/amber.json";
    let package = |file: &str, version: &str, files: &[(&str, &[u8])]| {
        let path = dir.path().join(file);
        let manifest = format!(
            "id = 'alpha'\nname = 'Alpha'\nversion = '{version}'\napi = '0.2'\nside = 'both'\n"
        );
        fs::write(&path, Package::write_files(&manifest, files).unwrap()).unwrap();
        read_local_smod(&path).unwrap()
    };
    let v1 = package(
        "alpha-1.smod",
        "1.0.0",
        &[(block, br#"{"properties":{"lit":[true,false]}}"#)],
    );
    install_local_at(
        &home,
        &[v1],
        &native_mods::Cancellation::default(),
        &|_| {},
        &|_| true,
    )
    .unwrap();
    super::super::update_tests::world(
        &home,
        "Uses Alpha",
        super::super::update_tests::roster("alpha"),
    );

    // Same file name, and the block is gone: by review time the candidate's alpha.smod is
    // already the new archive, so only the live copy still shows what the world loses.
    let v2 = package("alpha.smod", "2.0.0", &[]);
    let risked = AtomicBool::new(false);
    let err = install_local_at(
        &home,
        &[v2],
        &native_mods::Cancellation::default(),
        &|_| {},
        &|impact| {
            risked.store(impact.risks_worlds(), Ordering::SeqCst);
            false
        },
    )
    .unwrap_err();
    assert!(matches!(err, InstallError::Cancelled), "{err:?}");
    assert!(
        risked.load(Ordering::SeqCst),
        "the world using alpha's block is named"
    );
}

#[test]
fn a_predecessor_named_with_other_case_is_replaced_under_its_own_name() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("profile");
    let mods = home.join(smod_mod_list::DIR);
    fs::create_dir_all(&mods).unwrap();
    content_smod(&mods, "Alpha.smod", "alpha", "Alpha", "1.0.0");

    let v2_path = content_smod(dir.path(), "alpha-2.smod", "alpha", "Alpha", "2.0.0");
    let v2_bytes = fs::read(&v2_path).unwrap();
    let v2 = read_local_smod(&v2_path).unwrap();
    install_local_at(
        &home,
        &[v2],
        &native_mods::Cancellation::default(),
        &|_| {},
        &|_| true,
    )
    .unwrap();

    let archives: Vec<String> = fs::read_dir(&mods)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".smod"))
        .collect();
    assert_eq!(archives, ["Alpha.smod"]);
    assert_eq!(fs::read(mods.join("Alpha.smod")).unwrap(), v2_bytes);
}
