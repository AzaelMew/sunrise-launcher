use super::*;
use crate::browser::Kind;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const REPLACED_V1_1_2: &str = include_str!("../../../../tests/fixtures/replaced-v1.1.2.json");

fn dir(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "sunrise-auto-test-{name}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn smod_bytes(id: &str, version: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("sunrise-mod.toml", opts).unwrap();
        writeln!(zip, "id = \"{id}\"\nversion = \"{version}\"").unwrap();
        zip.finish().unwrap();
    }
    buf
}

fn write_installed_ruby(home: &Path, version: &str, bytes: &[u8]) -> installed::Entry {
    let mods_dir = home.join("sunrise-mods");
    fs::create_dir_all(&mods_dir).unwrap();
    let file_path = mods_dir.join("ruby.smod");
    fs::write(&file_path, bytes).unwrap();
    let hash = index::sha256_file(&file_path).unwrap();
    let entry = installed::Entry {
        source: SourceId::Sunrise,
        project: "ruby".into(),
        slug: "ruby".into(),
        title: "Ruby".into(),
        kind: Kind::Mod,
        version: version.into(),
        number: version.into(),
        file: "sunrise-mods/ruby.smod".into(),
        checksum: format!("SHA-256:{hash}"),
        page: String::new(),
        dependency: false,
        verdict: None,
        install_hold: None,
    };
    let mut list = installed::Installed::load(home);
    list.entries
        .retain(|row| !(row.source == SourceId::Sunrise && row.project == "ruby"));
    list.entries.push(entry.clone());
    list.save(home).unwrap();
    entry
}

fn legacy_ledger(checksum: &str, copy: &str, pid: u32) -> String {
    REPLACED_V1_1_2
        .replace("CHECKSUM_HERE", checksum)
        .replace("mod-browser/replaced/ruby", copy)
        .replace("12345", &pid.to_string())
}

#[test]
fn backup_paths_stay_in_the_backup_folder() {
    let root = dir("backup-path");
    let home = root.join("home");
    let v1 = smod_bytes("ruby", "1.0.0");
    let entry = write_installed_ruby(&home, "1.0.0", &v1);
    let outside = root.join("outside");
    fs::write(&outside, &v1).unwrap();
    fs::create_dir_all(home.join(KEPT_DIR)).unwrap();
    let ledger = home.join(KEPT);

    // An escaping backup path is refused and leaves the outside file untouched.
    fs::write(
        &ledger,
        legacy_ledger(&entry.checksum, "mod-browser/replaced/../../outside", 12345),
    )
    .unwrap();
    assert!(recover(&home).is_err());
    assert!(outside.is_file());
    assert!(ledger.is_file());

    // A stamped backup file directly inside `mod-browser/replaced/` is accepted and retired.
    let copy = "mod-browser/replaced/ruby.0123456789abcdef01234567";
    fs::write(home.join(copy), &v1).unwrap();
    fs::write(&ledger, legacy_ledger(&entry.checksum, copy, 12345)).unwrap();
    assert!(recover(&home).is_ok());
    assert!(!ledger.is_file());
    assert!(home.join(copy).is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_omitted_installed_claim_cannot_authorize_retiring_the_backup() {
    let root = dir("omitted-claim");
    let home = root.join("home");
    let v1 = smod_bytes("ruby", "1.0.0");
    let v2 = smod_bytes("ruby", "1.0.1");
    let old = write_installed_ruby(&home, "1.0.0", &v1);
    let _live = write_installed_ruby(&home, "1.0.1", &v2);
    let backup = home.join(KEPT_DIR).join("ruby");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, &v1).unwrap();
    let ledger = home.join(KEPT);
    let ledger_text = legacy_ledger(&old.checksum, "mod-browser/replaced/ruby", 12345);
    fs::write(&ledger, &ledger_text).unwrap();

    let manifest = home.join(installed::MANIFEST);
    let valid_manifest = fs::read_to_string(&manifest).unwrap();
    let mut raw: serde_json::Value = serde_json::from_str(&valid_manifest).unwrap();
    raw["entries"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "source": "sunrise",
            "project": "ruby",
            "slug": "ruby",
            "title": "Ruby",
            "kind": "mod",
            "version": "1.0.0",
            "number": "1.0.0",
            "file": "../external",
            "checksum": old.checksum,
            "page": ""
        }));
    let tampered = serde_json::to_string_pretty(&raw).unwrap();
    fs::write(&manifest, &tampered).unwrap();

    assert!(recover(&home).is_err());
    assert_eq!(fs::read_to_string(&manifest).unwrap(), tampered);
    assert_eq!(fs::read_to_string(&ledger).unwrap(), ledger_text);
    assert!(backup.is_file());

    // Once the manifest is valid again, recovery retires the legacy ledger while keeping the copy.
    fs::write(&manifest, &valid_manifest).unwrap();
    assert!(recover(&home).is_ok());
    assert!(!ledger.exists());
    assert!(backup.is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unproved_backup_stops_recovery_but_not_play() {
    let root = dir("legacy-recovery");
    let home = root.join("home");
    let v1 = smod_bytes("ruby", "1.0.0");
    let v2 = smod_bytes("ruby", "1.0.1");
    let old = write_installed_ruby(&home, "1.0.0", &v1);
    let _live = write_installed_ruby(&home, "1.0.1", &v2);
    let backup = home.join(KEPT_DIR).join("ruby");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, &v1).unwrap();
    let ledger = home.join(KEPT);

    // Real v1.1.2 ledger (no session field) retires even when the old PID happens to match ours.
    let valid = legacy_ledger(
        &old.checksum,
        "mod-browser/replaced/ruby",
        std::process::id(),
    );
    fs::write(&ledger, &valid).unwrap();
    assert!(recover(&home).is_ok());
    assert!(!ledger.exists());
    assert!(backup.is_file());

    // Corrupt ledger, missing backup or directory backup: recover keeps everything and lets Play continue.
    for (name, bytes, remove_backup, dir_backup) in [
        ("invalid-json", "{".to_owned(), false, false),
        ("missing-backup", valid.clone(), true, false),
        ("dir-backup", valid, true, true),
    ] {
        fs::write(&ledger, &bytes).unwrap();
        if remove_backup {
            let _ = fs::remove_file(&backup);
            let _ = fs::remove_dir_all(&backup);
        } else {
            let _ = fs::remove_dir_all(&backup);
            fs::write(&backup, &v1).unwrap();
        }
        if dir_backup {
            fs::create_dir_all(&backup).unwrap();
        }
        let error = recover(&home).unwrap_err();
        assert!(error.contains("You can still play."), "{name}: {error}");
        assert_eq!(fs::read_to_string(&ledger).unwrap(), bytes, "{name}");
        assert_eq!(backup.is_dir(), dir_backup, "{name}");
        assert_eq!(backup.is_file(), !remove_backup, "{name}");
        assert_eq!(fs::read(home.join("sunrise-mods/ruby.smod")).unwrap(), v2);
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unproved_live_package_is_not_overwritten_from_the_old_ledger() {
    let root = dir("unproved-live");
    let home = root.join("home");
    let v1 = smod_bytes("ruby", "1.0.0");
    let old = write_installed_ruby(&home, "1.0.0", &v1);
    let backup = home.join(KEPT_DIR).join("ruby");
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    fs::write(&backup, &v1).unwrap();
    let ledger = home.join(KEPT);
    fs::write(
        &ledger,
        legacy_ledger(&old.checksum, "mod-browser/replaced/ruby", 12345),
    )
    .unwrap();

    // Corrupt the live package bytes so they no longer match mod-browser.json.
    fs::write(home.join("sunrise-mods/ruby.smod"), b"half").unwrap();
    assert!(recover(&home).is_err());
    assert_eq!(
        fs::read(home.join("sunrise-mods/ruby.smod")).unwrap(),
        b"half"
    );
    assert_eq!(fs::read(&backup).unwrap(), v1);
    assert_eq!(kept_list(&home).unwrap().len(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn lock_failure_reports_an_unchecked_update_not_a_retained_backup() {
    let root = dir("lock-failure");
    let home = root.join("home-as-file");
    fs::write(&home, b"not a directory").unwrap();
    assert_eq!(
        recover(&home).unwrap_err(),
        "The launcher couldn't check for unfinished mod updates. You can still play."
    );
    assert_eq!(fs::read(&home).unwrap(), b"not a directory");
    let _ = fs::remove_dir_all(&root);
}
