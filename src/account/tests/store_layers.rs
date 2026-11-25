//! The layered secret store: where secrets live, prompts, and a keyring that is slow, gone or unreadable.

use super::*;

#[test]
fn accounts_keep_secrets_in_the_store_only() {
    let store = MemoryStore::default();
    let mut accounts = Accounts::default();
    let account = Account {
        uuid: UUID.into(),
        name: "Notch".into(),
        skin: None,
        cape: None,
    };
    let secrets = Secrets {
        refresh_token: "r".into(),
        sign_in_required: false,
        minecraft_token: "mc-secret".into(),
        minecraft_expires_at: 1,
        xuid: None,
        owns: true,
    };
    accounts.add(&store, account.clone(), &secrets).unwrap();
    assert_eq!(accounts.selected(), Some(&account));
    assert_eq!(accounts.find("notch"), Some(&account));
    assert_eq!(
        accounts.find("069a79f4-44e9-4726-a5be-fca90e38aaf5"),
        Some(&account)
    );
    let saved = serde_json::to_string(&accounts).unwrap();
    assert!(!saved.contains("mc-secret") && !saved.contains("\"r\""));
    assert!(store.get(UUID).unwrap().unwrap().contains("mc-secret"));
    accounts.remove(&store, UUID).unwrap();
    assert_eq!(
        (accounts.selected(), store.get(UUID).unwrap()),
        (None, None)
    );
    assert!(!store.persistent());
}

#[test]
fn a_dismissed_prompt_is_asked_once_a_run() {
    let path = secrets_file("dismissed");
    FileStore::new(path.clone()).set("A", "secret-a").unwrap();
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    // The start asks once to hand A over; nothing after asks again.
    let run = launcher_with(&keychain, &path);
    assert_eq!(keychain.asked(), 1);
    run.set("B", "secret-b").unwrap();
    run.set("A", "renewed-a").unwrap();
    assert_eq!(run.get("A").unwrap(), Some("renewed-a".into()));
    assert!(run.get("never-signed-in").is_err());
    run.delete("B").unwrap();
    assert_eq!(run.get("B").unwrap(), None);
    assert_eq!(keychain.asked(), 1);
    remove_secrets_dir(&path);
}

#[test]
fn a_keyring_that_never_answers_ends_play_with_what_is_wrong() {
    let path = secrets_file("silent");
    let hung = Arc::new(Mutex::new(()));
    let held = hung.lock().unwrap();
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let keyring = Silent {
        hung: hung.clone(),
        asked: asked.clone(),
    };
    let store = Arc::new(Layered::new(
        Box::new(Bounded::new(keyring, SILENT_WAIT)),
        FileStore::new(path.clone()),
    ));
    let mut accounts = signed_in(&MemoryStore::default(), &secrets("mc-token", true));
    let started = Instant::now();
    // The window's sign-in check at start-up asks first, and holds the account lock meanwhile.
    let checking = {
        let store = store.clone();
        std::thread::spawn(move || session::status_with_progress(store.as_ref(), UUID, &|_| {}))
    };
    while asked.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
    // Then Play, behind it.
    let (done, played) = std::sync::mpsc::channel();
    {
        let store = store.clone();
        std::thread::spawn(move || {
            let _ = done.send(launch(
                &offline(),
                Some("test-client"),
                store.as_ref(),
                &mut accounts,
            ));
        });
    }
    let refused = played
        .recv_timeout(Duration::from_secs(10))
        .expect("Play waited on the keyring forever")
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5));
    let status = checking.join().unwrap();
    assert_eq!(status.state, session::State::Retry);
    // Not "sign in with an account that owns Minecraft": the player is signed in.
    let said = format!("Can't read Notch's sign-in: {KEYCHAIN_AWAY}");
    assert_eq!(refused.keyring.as_deref(), Some(said.as_str()));
    assert_eq!(
        (refused.to_string(), refused.in_window()),
        (said.clone(), said)
    );
    // The next start asks the keyring again, and says it didn't answer.
    let restarted = Layered::new(
        Box::new(Bounded::new(
            Silent {
                hung: hung.clone(),
                asked: asked.clone(),
            },
            SILENT_WAIT,
        )),
        FileStore::new(path.clone()),
    );
    let mut accounts = signed_in(&MemoryStore::default(), &secrets("mc-token", true));
    let refused = launch(&offline(), Some("test-client"), &restarted, &mut accounts).unwrap_err();
    assert_eq!(
        refused.keyring,
        Some(format!(
            "Can't read Notch's sign-in: {}",
            keyring_no_answer(SILENT_WAIT)
        ))
    );
    drop(held);
    remove_secrets_dir(&path);
}

#[test]
fn a_keyring_given_up_on_is_not_asked_again_until_it_answers() {
    let hung = Arc::new(Mutex::new(()));
    let held = hung.lock().unwrap();
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let keyring = Bounded::new(
        Silent {
            hung: hung.clone(),
            asked: asked.clone(),
        },
        SILENT_WAIT,
    );
    let started = Instant::now();
    assert_eq!(keyring.get("A"), Err(keyring_no_answer(SILENT_WAIT).into()));
    assert!(started.elapsed() >= SILENT_WAIT);
    // Still stuck: the next calls fail at once, without a second call piling up behind it.
    let again = Instant::now();
    assert_eq!(
        keyring.set("A", "secret-a"),
        Err(keyring_no_answer(SILENT_WAIT).into())
    );
    assert!(again.elapsed() < SILENT_WAIT);
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    // It answers at last: it is asked again, and its answers come back.
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(10);
    while keyring.delete("A").is_err() {
        assert!(Instant::now() < deadline, "the late answer never counted");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(keyring.get("A"), Ok(None));
}

#[test]
fn signing_out_while_the_keychain_is_away_forgets_its_copy() {
    let path = secrets_file("sign-out");
    let keychain = Arc::new(Keychain::default());
    launcher_with(&keychain, &path)
        .set("A", "secret-a")
        .unwrap();
    keychain.set_away(true);
    let away = launcher_with(&keychain, &path);
    away.delete("A").unwrap();
    assert_eq!(away.get("A").unwrap(), None);
    // The keychain's copy goes at the first start it answers, and the file with it.
    keychain.set_away(false);
    let back = launcher_with(&keychain, &path);
    assert_eq!(
        (back.get("A").unwrap(), keychain.kept.get("A").unwrap()),
        (None, None)
    );
    assert!(!path.exists());
    remove_secrets_dir(&path);
}

#[test]
fn an_unreadable_keychain_is_retryable_without_any_write() {
    struct Locked;
    impl SecretStore for Locked {
        fn get(&self, _: &str) -> Result<Option<String>, StoreError> {
            Err("locked-keychain-details".into())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), StoreError> {
            panic!("must not overwrite an unreadable credential")
        }
        fn delete(&self, _: &str) -> Result<(), StoreError> {
            panic!("must not delete an unreadable credential")
        }
        fn persistent(&self) -> bool {
            false
        }
    }
    let status = session::refresh_with(
        &offline(),
        Some("test-client"),
        &Locked,
        &notch(),
        &nowhere(),
    );
    assert_eq!(status.state, session::State::Retry);
    assert!(!status.message.contains("locked-keychain-details"));
}
