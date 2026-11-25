//! The OS keychain: long secrets split into pieces, refused deletes, and its platform errors.

use super::*;

/// The OS keychain itself: Credential Manager and the Keychain take one secret per item, and a
/// sign-in's secrets are longer than one item holds. Ignored by default: it writes to the keychain
/// (a test key, removed again).
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
#[ignore = "writes to the OS keychain"]
fn system_keychain_keeps_long_secrets() {
    let key = "test-long-secret";
    let secret = format!("{{\"refresh_token\":\"{}\"}}", "r".repeat(3000));
    SystemStore.set(key, &secret).unwrap();
    assert_eq!(SystemStore.get(key).unwrap(), Some(secret.clone()));
    SystemStore.delete(key).unwrap();
    assert_eq!(SystemStore.get(key).unwrap(), None);
}

#[test]
fn long_secrets_split_into_keychain_sized_pieces() {
    // Mixed 1-, 2- and 4-byte characters, so the cuts land inside some of them.
    let secret = "aé🔑".repeat(300);
    let pieces = split_secret(&secret);
    assert!(pieces.len() > 1);
    assert_eq!(pieces.concat(), secret);
    for piece in &pieces {
        assert!(piece.len() <= PIECE);
        assert!(piece.encode_utf16().count() * 2 <= 2560);
    }
    assert_eq!(split_secret("short"), vec!["short"]);
}

#[test]
fn a_secret_the_keychain_cant_finish_keeps_the_old_one() {
    let items = FullItems {
        items: Mutex::default(),
        room: 2,
    };
    let old = "o".repeat(PIECE + 1);
    set_pieces(&items, UUID, &old).unwrap();
    assert_eq!(get_pieces(&items, UUID).unwrap(), Some(old.clone()));
    // A renewal three pieces long: the first two overwrite, the third finds the store full.
    let new = "n".repeat(2 * PIECE + 1);
    let said = set_pieces(&items, UUID, &new).unwrap_err();
    assert_eq!(said.0, CREDENTIAL_MANAGER_FULL);
    assert_eq!(get_pieces(&items, UUID).unwrap(), Some(old));
    // A sign-in into a full store keeps nothing, and says why.
    let full = FullItems {
        items: Mutex::default(),
        room: 0,
    };
    assert_eq!(
        set_pieces(&full, UUID, "s").unwrap_err().0,
        CREDENTIAL_MANAGER_FULL
    );
    assert_eq!(get_pieces(&full, UUID).unwrap(), None);
    // Room again: the longer secret replaces the old one whole.
    let items = FullItems { room: 3, ..items };
    set_pieces(&items, UUID, &new).unwrap();
    assert_eq!(get_pieces(&items, UUID).unwrap(), Some(new));
    set_pieces(&items, UUID, "short").unwrap();
    assert_eq!(get_pieces(&items, UUID).unwrap(), Some("short".into()));
    assert_eq!(items.items.lock().unwrap().len(), 1);
}

#[test]
fn a_split_secret_folds_into_one_item() {
    // The layout launchers before #4912 wrote, in memory.
    let items = FullItems {
        items: Mutex::default(),
        room: 99,
    };
    let secret = "s".repeat(2 * PIECE + 1);
    set_pieces(&items, UUID, &secret).unwrap();
    assert_eq!(items.items.lock().unwrap().len(), 3);
    // The first read answers the whole secret and leaves a single item behind.
    assert_eq!(get_item(&items, UUID).unwrap(), Some(secret.clone()));
    assert_eq!(items.items.lock().unwrap().len(), 1);
    assert_eq!(items.read(UUID).unwrap(), Some(secret));
    // A rewrite stays one item; a shorter secret leaves no pieces behind.
    set_item(&items, UUID, "short").unwrap();
    assert_eq!(items.items.lock().unwrap().len(), 1);
    assert_eq!(get_item(&items, UUID).unwrap(), Some("short".into()));
    delete_pieces(&items, UUID).unwrap();
    assert_eq!(get_item(&items, UUID).unwrap(), None);
}

#[test]
fn a_refused_delete_puts_the_split_secret_back() {
    // The layout launchers before #4912 wrote, in memory.
    let old = "o".repeat(2 * PIECE + 1);
    let pieces: Vec<String> = split_secret(&old).iter().map(|p| p.to_string()).collect();
    for left in [0, 1] {
        let items = RefusedDeletes {
            items: Mutex::new(
                pieces
                    .iter()
                    .enumerate()
                    .map(|(n, p)| (piece_key(UUID, n), p.clone()))
                    .collect(),
            ),
            left: Mutex::new(left),
        };
        // The fold's clean-up is refused, halfway or at once: the read still answers the whole
        // secret, and every old item is back behind it.
        assert_eq!(get_item(&items, UUID).unwrap(), Some(old.clone()));
        assert_eq!(items.items.lock().unwrap().len(), pieces.len());
        assert_eq!(get_item(&items, UUID).unwrap(), Some(old.clone()));
        // A writer's set over the split layout fails the same way, and reads back the same.
        *items.left.lock().unwrap() = left;
        assert_eq!(
            set_item(&items, UUID, "new").unwrap_err().0,
            "the keychain refused the delete"
        );
        assert_eq!(get_item(&items, UUID).unwrap(), Some(old.clone()));
        assert_eq!(items.items.lock().unwrap().len(), pieces.len());
    }
    // Windows' set_pieces rolls a refused leftover delete back the same way.
    let items = RefusedDeletes {
        items: Mutex::new(
            pieces
                .iter()
                .enumerate()
                .map(|(n, p)| (piece_key(UUID, n), p.clone()))
                .collect(),
        ),
        left: Mutex::new(0),
    };
    assert!(set_pieces(&items, UUID, "shorter").is_err());
    assert_eq!(get_pieces(&items, UUID).unwrap(), Some(old));
}

#[test]
fn credential_manager_error_8_says_the_store_is_full() {
    let said = credential_manager_says(8).unwrap();
    assert!(said.contains("Credential Manager is full"));
    assert!(said.contains("Windows Credentials"));
    // Other codes keep the keyring crate's own words.
    assert_eq!(credential_manager_says(5), None);
    assert_eq!(credential_manager_says(1312), None);
}

#[test]
fn macos_removed_executable_error_requests_a_restart_and_new_sign_in() {
    let said = macos_keychain_says(100002).unwrap();
    assert_eq!(said, MACOS_LAUNCHER_RESTART);
    assert!(said.contains("Restart the launcher"));
    assert!(said.contains("sign in with Microsoft again"));
    for code in [0, 2, -25300, -25293, -25291, 100001, 100003] {
        assert_eq!(macos_keychain_says(code), None, "OSStatus {code}");
    }
}

/// Constructs native errors, without creating or reading any Keychain entry.
#[cfg(target_os = "macos")]
#[test]
fn only_the_typed_macos_removed_executable_failure_gets_the_restart_message() {
    let failure = |code| {
        keyring::Error::PlatformFailure(Box::new(security_framework::base::Error::from_code(code)))
    };
    assert_eq!(write_error(failure(100002)), MACOS_LAUNCHER_RESTART);
    for code in [-25300, -25293, -25291, 100003] {
        let error = failure(code);
        let original = error.to_string();
        assert_ne!(original, MACOS_LAUNCHER_RESTART);
        assert_eq!(write_error(error), original);
    }
    let unrelated = keyring::Error::PlatformFailure(Box::new(std::io::Error::other(
        "OSStatus 100002: unrelated provider",
    )));
    let original = unrelated.to_string();
    assert_eq!(write_error(unrelated), original, "not a string match");
    assert_eq!(
        write_error(keyring::Error::NoEntry),
        keyring::Error::NoEntry.to_string()
    );
}
