use super::*;
use serde_json::json;

#[test]
fn recovery_changes_only_the_row_that_still_owns_the_published_bytes() {
    let file = "sunrise-mods/ruby.smod";
    let old = json!({"source":"sunrise", "project":"ruby", "file":file,
        "version":"1.0.0", "checksum":format!("SHA-256:{}", super::super::sha256(b"old"))});
    let new = json!({"source":"sunrise", "project":"ruby", "file":file,
        "version":"2.0.0", "checksum":format!("SHA-256:{}", super::super::sha256(b"new"))});
    let mut metadata_changed = old.clone();
    metadata_changed["verdict"] = json!("hold");
    let mut changed = old.clone();
    changed["checksum"] = json!(format!("SHA-256:{}", super::super::sha256(b"third")));
    let other = json!({"source":"unknown", "project":"other", "file":"another.smod", "opaque":25});
    let mut collision = other.clone();
    collision["file"] = json!(file);
    for (bytes, before, after, ambiguous) in [
        (b"new", vec![&old, &other], vec![&other, &new], false),
        (
            b"new",
            vec![&metadata_changed],
            vec![&metadata_changed],
            true,
        ),
        (b"old", vec![&old, &other], vec![&old, &other], false),
        (b"bad", vec![&old, &other], vec![&old, &other], false),
        (b"new", vec![&changed], vec![&changed], true),
        (b"new", vec![&old, &collision], vec![&old, &collision], true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let package = home.join(file);
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, bytes).unwrap();
        let manifest = home.join(MANIFEST);
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({"entries":before})).unwrap(),
        )
        .unwrap();
        let sidecar = home.join(PENDING);
        fs::write(
            &sidecar,
            serde_json::to_vec(&json!({
                "id":"ruby", "file":file, "old":old, "new":new,
            }))
            .unwrap(),
        )
        .unwrap();

        let result = reconcile(home);
        assert_eq!(result.is_err(), ambiguous, "{result:?}");
        let manifest: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        assert_eq!(manifest["entries"], json!(after));
        assert_eq!(fs::read(package).unwrap(), bytes);
        assert_eq!(sidecar.exists(), ambiguous);
    }
}

#[test]
fn already_published_manifest_is_synced_before_removing_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let file = "sunrise-mods/ruby.smod";
    let package = home.join(file);
    fs::create_dir_all(package.parent().unwrap()).unwrap();
    fs::write(&package, b"new").unwrap();
    let new = json!({"source":"sunrise", "project":"ruby", "file":file,
        "version":"2.0.0", "checksum":format!("SHA-256:{}", super::super::sha256(b"new"))});
    let manifest = home.join(MANIFEST);
    fs::write(
        &manifest,
        serde_json::to_vec(&json!({"entries":[new]})).unwrap(),
    )
    .unwrap();
    let sidecar = home.join(PENDING);
    let pending = serde_json::to_vec(&json!({"id":"ruby", "file":file, "new":new})).unwrap();
    fs::write(&sidecar, &pending).unwrap();
    let _lock = lock_home(home).unwrap();

    let result = reconcile_locked(home, &sidecar, sha256_file, |path| {
        assert_eq!(path, manifest);
        assert_eq!(fs::read(&sidecar).unwrap(), pending);
        Err(io::Error::other("manifest sync failed"))
    });
    assert!(
        matches!(result, Err(RecoveryProblem::Durability { path, error })
        if path == manifest && error.to_string() == "manifest sync failed")
    );
    assert_eq!(fs::read(&sidecar).unwrap(), pending);

    let calls = std::cell::RefCell::new(Vec::new());
    let retirement = reconcile_locked(home, &sidecar, sha256_file, |path| {
        calls
            .borrow_mut()
            .push((path.to_path_buf(), sidecar.exists()));
        if path == sidecar {
            Err(io::Error::other("retirement sync failed"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(
        matches!(retirement, RecoveryProblem::Durability { path, error }
        if path == sidecar && error.to_string() == "retirement sync failed")
    );
    assert_eq!(
        calls.into_inner(),
        [(manifest, true), (sidecar.clone(), false)]
    );
    assert!(!sidecar.exists());
}

#[test]
fn unreadable_pending_entries_never_publish_a_manifest() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(PENDING);
    let invalid = b"{oops";
    fs::write(&path, invalid).unwrap();
    assert!(reconcile(home.path()).is_err());
    assert!(!home.path().join(MANIFEST).exists());
    assert_eq!(fs::read(&path).unwrap(), invalid);

    let len = MAX_PENDING + 1;
    fs::File::create(&path).unwrap().set_len(len).unwrap();
    assert!(
        reconcile(home.path())
            .unwrap_err()
            .to_string()
            .contains("pending entry too large")
    );
    assert!(!home.path().join(MANIFEST).exists());
    assert_eq!(fs::metadata(&path).unwrap().len(), len);

    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(reconcile(home.path()).is_err());
    assert!(!home.path().join(MANIFEST).exists());
    assert!(path.is_dir());
}

#[test]
fn real_game_row_keeps_its_fields_and_refuses_duplicate_or_aliased_claims() {
    // The complete row emitted by sunrise-client/src/mod_registry.rs at f92f138bfe.
    let file = "sunrise-mods/ruby.smod";
    let row = |v, bytes| {
        json!({
            "source": "sunrise", "project": "ruby", "slug": "ruby", "title": "Ruby",
            "kind": "mod", "version": v, "number": v, "file": file,
            "checksum": format!("SHA-256:{}", super::super::sha256(bytes)),
            "page": "https://mods.playsunrise.net/ruby", "dependency": false,
            "verdict": "Verified"
        })
    };
    let old = row("1.0.0", b"old");
    let new = row("2.0.0", b"new");
    let home = tempfile::tempdir().unwrap();
    let package = home.path().join(file);
    let aliases = [
        "sunrise-mods//./ruby.smod",
        "other/../sunrise-mods/ruby.smod",
        &package.to_string_lossy(),
        #[cfg(windows)]
        r"sunrise-mods\RUBY.smod",
        #[cfg(windows)]
        "sunrise-mods/ruby.smod.",
        #[cfg(windows)]
        "sunrise-mods/ruby.smod ",
    ];
    fs::create_dir(package.parent().unwrap()).unwrap();
    fs::write(&package, b"new").unwrap();
    let journal = home.path().join(PENDING);
    let pending =
        serde_json::to_vec(&json!({"id": "ruby", "file": file, "old": old, "new": new})).unwrap();
    let manifest = home.path().join(MANIFEST);
    let contested = aliases.into_iter().flat_map(|file| {
        let alias = json!({"source": "future-source", "project": "other", "file": file});
        [json!([old, alias]), json!([new, alias])]
    });
    for rows in [json!([old, old]), json!([new, old])]
        .into_iter()
        .chain(contested)
    {
        fs::write(&journal, &pending).unwrap();
        let before = serde_json::to_vec(&json!({"entries": rows})).unwrap();
        fs::write(&manifest, &before).unwrap();
        assert!(reconcile(home.path()).is_err());
        assert_eq!(fs::read(&journal).unwrap(), pending);
        assert_eq!(fs::read(&manifest).unwrap(), before);
        assert_eq!(fs::read(&package).unwrap(), b"new");
    }

    fs::write(
        &manifest,
        serde_json::to_vec(&json!({"entries": [old]})).unwrap(),
    )
    .unwrap();
    reconcile(home.path()).unwrap();
    let published: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(published, json!({"entries": [new]}));
    assert!(!journal.exists());
}

#[test]
fn check_other_claims_catches_windows_style_competing_claim() {
    let home = tempfile::tempdir().unwrap();
    let mods = home.path().join("sunrise-mods");
    fs::create_dir_all(&mods).unwrap();
    fs::write(mods.join("lantern.origin.json"), b"{}").unwrap();
    let rows = Installed {
        entries: vec![json!({
            "source": "future-source",
            "project": "other",
            "file": r"sunrise-mods\lantern.origin.json",
        })],
    };
    assert!(
        rows.check_other_claims(home.path(), "lantern", "sunrise-mods/lantern.origin.json")
            .is_err(),
        "Windows-style competing claim must be detected on every OS"
    );
}
