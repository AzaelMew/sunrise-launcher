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
    for (bytes, before, after) in [
        (b"new", vec![&old, &other], vec![&other, &new]),
        (b"new", vec![&metadata_changed], vec![&metadata_changed]),
        (b"old", vec![&old, &other], vec![&old, &other]),
        (b"bad", vec![&old, &other], vec![&old, &other]),
        (b"new", vec![&changed], vec![&changed]),
        (b"new", vec![&old, &collision], vec![&old, &collision]),
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

        reconcile(home).unwrap();
        let manifest: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        assert_eq!(manifest["entries"], json!(after));
        assert_eq!(fs::read(package).unwrap(), bytes);
        assert!(!sidecar.exists());
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

    let result = reconcile_locked(home, &sidecar, |path| {
        assert_eq!(path, manifest);
        assert_eq!(fs::read(&sidecar).unwrap(), pending);
        Err(io::Error::other("manifest sync failed"))
    });
    assert!(result.unwrap_err().contains("manifest sync failed"));
    assert_eq!(fs::read(&sidecar).unwrap(), pending);

    let calls = std::cell::RefCell::new(Vec::new());
    reconcile_locked(home, &sidecar, |path| {
        calls
            .borrow_mut()
            .push((path.to_path_buf(), sidecar.exists()));
        Ok(())
    })
    .unwrap();
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
