//! The secrets file: keeping sign-ins across starts, its key and sealing, and processes sharing it.

use super::*;

#[test]
fn the_secrets_file_keeps_sign_ins_across_starts() {
    let path = secrets_file("starts");
    let store = FileStore::new(path.clone());
    assert!(store.persistent());
    assert_eq!(store.get(UUID).unwrap(), None);
    let signed_in = secrets("mc-secret", true);
    let mut accounts = Accounts::default();
    accounts.add(&store, notch(), &signed_in).unwrap();
    // The next start reads the same file.
    let again = FileStore::new(path.clone());
    let read = secrets_in(&again, UUID).expect("kept across starts");
    assert_eq!(
        (read.minecraft_token, read.refresh_token, read.owns),
        (signed_in.minecraft_token, signed_in.refresh_token, true)
    );
    // Never in accounts.json.
    assert!(!serde_json::to_string(&accounts)
        .unwrap()
        .contains("mc-secret"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "only the player reads it");
    }
    // No temporary file left behind.
    assert_eq!(
        files_beside(&path),
        [
            "account-secrets.json",
            "account-secrets.key",
            "account-secrets.lock"
        ]
    );
    // Signing the last account out removes the file.
    accounts.remove(&again, UUID).unwrap();
    assert_eq!(again.get(UUID).unwrap(), None);
    assert!(!path.exists());
    again.delete(UUID).unwrap();
    remove_secrets_dir(&path);
}

#[test]
fn a_broken_secrets_file_is_an_error_that_never_shows_it() {
    let path = secrets_file("broken");
    let store = FileStore::new(path.clone());
    // Cut short, and a lone string: serde's own messages quote the latter.
    for broken in [
        "{\"a\":\"leaked-secret",
        "\"leaked-secret\"",
        "[\"leaked-secret\"]",
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let sealed = store.seal(broken.as_bytes()).unwrap();
        std::fs::write(&path, &sealed).unwrap();
        let said = store.get("a").unwrap_err().0;
        assert!(!said.contains("leaked-secret"), "{said}");
        assert!(said.contains("line 1"), "{said}");
        // Never written over: the secrets in it aren't lost in silence.
        assert!(store.set("c", "four").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), sealed);
    }
    remove_secrets_dir(&path);
}

#[test]
fn the_secrets_file_is_never_plain_text() {
    let path = secrets_file("sealed");
    let store = FileStore::new(path.clone());
    store.set("a", "kept-secret").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(super::SEALED));
    assert!(
        !bytes.windows(11).any(|w| w == b"kept-secret"),
        "the secret is encrypted"
    );
    assert_eq!(store.get("a").unwrap().as_deref(), Some("kept-secret"));
    // Plain JSON, or one byte changed in the sealed file: refused, never quoted, never written over.
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    for other in [b"{\"a\":\"leaked-secret\"}".to_vec(), changed] {
        std::fs::write(&path, &other).unwrap();
        let said = store.get("a").unwrap_err().0;
        assert!(!said.contains("leaked-secret"), "{said}");
        assert!(said.contains("Remove it and sign in again"), "{said}");
        assert!(store.set("c", "four").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), other);
    }
    remove_secrets_dir(&path);
}

#[test]
fn the_key_takes_the_install_secret_beside_the_file() {
    let path = secrets_file("install");
    let store = FileStore::new(path.clone());
    store.set("a", "kept-secret").unwrap();
    let secret = path.with_extension("key");
    assert_eq!(std::fs::read(&secret).unwrap().len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&secret).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "only the player reads it");
    }
    // The same file in another install's folder, with a secret of its own: unreadable there.
    let other = secrets_file("install-other");
    let elsewhere = FileStore::new(other.clone());
    elsewhere.set("b", "other-secret").unwrap();
    std::fs::copy(&path, &other).unwrap();
    let said = elsewhere.get("a").unwrap_err().0;
    assert!(said.contains("Remove it and sign in again"), "{said}");
    // Without its secret, the file can't be read either: no key is left to work out.
    std::fs::remove_file(&secret).unwrap();
    assert!(store.get("a").is_err());
    assert!(!secret.exists(), "a read never makes a new secret");
    remove_secrets_dir(&path);
    remove_secrets_dir(&other);
}

#[test]
fn a_file_sealed_under_the_old_key_is_read_and_sealed_again() {
    use chacha20poly1305::{aead::Aead, KeyInit};
    let path = secrets_file("format1");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // As launchers before format 2 wrote it.
    let nonce = [7u8; 12];
    let sealed = chacha20poly1305::ChaCha20Poly1305::new(&super::file_key_v1())
        .encrypt(&nonce.into(), &b"{\"a\":\"old-secret\"}"[..])
        .unwrap();
    std::fs::write(&path, [b"HZSECv1\n".as_slice(), &nonce, &sealed].concat()).unwrap();
    let store = FileStore::new(path.clone());
    assert_eq!(store.get("a").unwrap().as_deref(), Some("old-secret"));
    // Sealed again at once, under the install's secret, and still read.
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"HZSECv2\n"));
    assert!(path.with_extension("key").exists());
    assert_eq!(
        FileStore::new(path.clone()).get("a").unwrap().as_deref(),
        Some("old-secret")
    );
    remove_secrets_dir(&path);
}

#[test]
fn processes_sharing_the_secrets_file_lose_nothing() {
    let path = secrets_file("processes");
    FileStore::new(path.clone()).set("signed-out", "x").unwrap();
    let exe = std::env::current_exe().unwrap();
    let writers: Vec<_> = (0..6)
        .map(|w| {
            std::process::Command::new(&exe)
                .args([
                    "--ignored",
                    "--exact",
                    "account::tests::secrets_file_writer",
                    "--test-threads=1",
                ])
                .env(WRITER, format!("{w}:{}", path.display()))
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut writer in writers {
        assert!(writer.wait().unwrap().success());
    }
    let store = FileStore::new(path.clone());
    for w in 0..6 {
        for i in 0..25 {
            assert_eq!(
                store.get(&format!("{w}-{i}")).unwrap(),
                Some(format!("secret-{w}-{i}"))
            );
        }
    }
    assert_eq!(store.get("signed-out").unwrap(), None);
    assert_eq!(
        files_beside(&path),
        [
            "account-secrets.json",
            "account-secrets.key",
            "account-secrets.lock"
        ]
    );
    remove_secrets_dir(&path);
}

#[test]
fn the_secrets_file_counts_only_sign_ins() {
    let path = secrets_file("counts");
    let file = FileStore::new(path.clone());
    assert!(!file.holds_secrets());
    file.put("A", SIGNED_OUT).unwrap();
    assert!(!file.holds_secrets(), "a sign-out mark alone");
    file.put("B", "secret-b").unwrap();
    assert!(file.holds_secrets());
    std::fs::write(&path, "{\"B\":\"secret-b").unwrap();
    assert!(!file.holds_secrets(), "a file that can't be read");
    remove_secrets_dir(&path);
}
