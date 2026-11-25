//! Handing secrets between the file and the keychain, while other launchers and sign-outs race it.

use super::*;

#[test]
fn the_keychain_takes_back_what_the_file_kept_while_it_was_away() {
    let path = secrets_file("recovery");
    let keychain = Arc::new(Keychain::default());
    // A is signed in while the keychain answers: no file.
    launcher_with(&keychain, &path)
        .set("A", "secret-a")
        .unwrap();
    assert!(!path.exists());
    // A start the keychain misses: B goes to the file, and A can't be read.
    keychain.set_away(true);
    let missed = launcher_with(&keychain, &path);
    missed.set("B", "secret-b").unwrap();
    assert!(missed.get("A").is_err());
    assert_eq!(missed.get("B").unwrap(), Some("secret-b".into()));
    // The keychain is back: the next start hands it B, and the file is gone.
    keychain.set_away(false);
    let back = launcher_with(&keychain, &path);
    assert_eq!(
        (
            keychain.kept.get("A").unwrap(),
            keychain.kept.get("B").unwrap()
        ),
        (Some("secret-a".into()), Some("secret-b".into()))
    );
    assert_eq!(back.get("B").unwrap(), Some("secret-b".into()));
    assert!(!path.exists());
    remove_secrets_dir(&path);
}

#[test]
fn a_launcher_on_the_file_keeps_working_after_another_hands_it_to_the_keychain() {
    let path = secrets_file("running");
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    let running = launcher_with(&keychain, &path);
    running.set("A", "secret-a").unwrap();
    // The keychain comes back and a second launcher (or the CLI) starts.
    keychain.set_away(false);
    let second = launcher_with(&keychain, &path);
    assert!(!path.exists());
    // The first one still reads, renews and signs out.
    assert_eq!(running.get("A").unwrap(), Some("secret-a".into()));
    running.set("A", "renewed-a").unwrap();
    assert_eq!(second.get("A").unwrap(), Some("renewed-a".into()));
    running.delete("A").unwrap();
    assert_eq!(
        (second.get("A").unwrap(), keychain.kept.get("A").unwrap()),
        (None, None)
    );
    assert!(!path.exists());
    remove_secrets_dir(&path);
}

#[test]
fn launchers_started_on_an_old_file_find_it_after_another_hands_it_over() {
    let path = secrets_file("inherited");
    // A previous run kept A in the file.
    FileStore::new(path.clone()).set("A", "secret-a").unwrap();
    // Two launchers start while the keychain is away: their hand-overs fail. One reads A then.
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    let read_before = launcher_with(&keychain, &path);
    let never_read = launcher_with(&keychain, &path);
    assert_eq!(read_before.get("A").unwrap(), Some("secret-a".into()));
    // The keychain is back and a third launcher hands A over.
    keychain.set_away(false);
    launcher_with(&keychain, &path);
    assert!(!path.exists());
    // Both running launchers still read A.
    assert_eq!(read_before.get("A").unwrap(), Some("secret-a".into()));
    assert_eq!(never_read.get("A").unwrap(), Some("secret-a".into()));
    remove_secrets_dir(&path);
}

#[test]
fn a_hand_over_by_another_process_right_after_a_start_missed_the_keychain() {
    let path = secrets_file("inherited-race");
    FileStore::new(path.clone()).set("A", "secret-a").unwrap();
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    // The moment this start's hand-over fails, the keychain is back and another process hands A
    // over (the keychain takes it, the file lets it go), before this start has done anything else.
    *keychain.after_away.lock().unwrap() = Some(Box::new({
        let (keychain, path) = (keychain.clone(), path.clone());
        move || {
            keychain.set_away(false);
            keychain.kept.set("A", "secret-a").unwrap();
            FileStore::new(path).delete("A").unwrap();
        }
    }));
    let started = launcher_with(&keychain, &path);
    assert!(!path.exists());
    assert_eq!(started.get("A").unwrap(), Some("secret-a".into()));
    remove_secrets_dir(&path);
}

#[test]
fn a_secret_renewed_in_the_file_during_the_hand_over_is_found_after_the_next() {
    let path = secrets_file("renewed-in-file");
    let file = FileStore::new(path.clone());
    file.set("A", "old-a").unwrap();
    file.set("B", "secret-b").unwrap();
    let keychain = Arc::new(Keychain::default());
    // While this start hands A over, a launcher on the file renews A there; then the keychain
    // refuses B, and this run stops asking it.
    *keychain.while_taking.lock().unwrap() = Some(Box::new({
        let path = path.clone();
        move || FileStore::new(path).set("A", "renewed-a").unwrap()
    }));
    keychain.refuses.lock().unwrap().push("B".into());
    let started = launcher_with(&keychain, &path);
    assert_eq!(keychain.kept.get("A").unwrap(), Some("old-a".into()));
    // Another start hands both over: this run, which hasn't read A since, still reads both, the
    // renewed A.
    keychain.refuses.lock().unwrap().clear();
    launcher_with(&keychain, &path);
    assert!(!path.exists());
    assert_eq!(
        (started.get("A").unwrap(), started.get("B").unwrap()),
        (Some("renewed-a".into()), Some("secret-b".into()))
    );
    remove_secrets_dir(&path);
}

#[test]
fn a_move_the_keychain_half_refuses_then_a_sign_out_leaves_no_copy() {
    let path = secrets_file("half");
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    let away = launcher_with(&keychain, &path);
    away.set("A", "secret-a").unwrap();
    away.set("B", "secret-b").unwrap();
    // Back, but it refuses B: A moves, B stays in the file.
    keychain.set_away(false);
    keychain.refuses.lock().unwrap().push("B".into());
    let back = launcher_with(&keychain, &path);
    assert_eq!(
        (
            keychain.kept.get("A").unwrap(),
            keychain.kept.get("B").unwrap()
        ),
        (Some("secret-a".into()), None)
    );
    assert_eq!(back.get("B").unwrap(), Some("secret-b".into()));
    // A signs out on that run, which stopped asking the keychain: no launcher reads A again...
    back.delete("A").unwrap();
    assert_eq!(back.get("A").unwrap(), None);
    // (A start where the keychain still refuses B then reads A as "away": no secret either.)
    assert_eq!(
        launcher_with(&keychain, &path).get("A").ok().flatten(),
        None
    );
    // ...and the keychain's copy goes at the first start it takes everything.
    keychain.refuses.lock().unwrap().clear();
    let later = launcher_with(&keychain, &path);
    assert_eq!(
        (keychain.kept.get("A").unwrap(), later.get("B").unwrap()),
        (None, Some("secret-b".into()))
    );
    assert!(!path.exists());
    remove_secrets_dir(&path);
}

#[test]
fn a_keychain_waiting_on_a_prompt_never_holds_the_file_up() {
    let path = secrets_file("prompt");
    let keychain = Arc::new(Keychain::default());
    keychain.set_away(true);
    launcher_with(&keychain, &path)
        .set("B", "secret-b")
        .unwrap();
    keychain.set_away(false);
    // An unlock prompt nobody answers yet, while one launcher signs A in.
    let prompt = keychain.prompt.lock().unwrap();
    let asked = keychain.asked();
    let signing_in = {
        let (keychain, path) = (keychain.clone(), path.clone());
        std::thread::spawn(move || {
            Layered {
                keychain: Box::new(Shared(keychain)),
                file: FileStore::new(path),
                away: Default::default(),
                parked: Default::default(),
            }
            .set("A", "secret-a")
        })
    };
    while keychain.asked() == asked {
        std::thread::sleep(Duration::from_millis(1));
    }
    // Another process still reads and writes the file meanwhile.
    let (done, said) = std::sync::mpsc::channel();
    let other = path.clone();
    std::thread::spawn(move || {
        let file = FileStore::new(other);
        let _ = done.send((file.get("B"), file.set("C", "secret-c")));
    });
    let (read, written) = said
        .recv_timeout(Duration::from_secs(10))
        .expect("the file waited on the keychain's prompt");
    assert_eq!((read.unwrap(), written), (Some("secret-b".into()), Ok(())));
    // The prompt is answered: A goes to the keychain, B and C are still in the file.
    drop(prompt);
    signing_in.join().unwrap().unwrap();
    let file = FileStore::new(path.clone());
    assert_eq!(keychain.kept.get("A").unwrap(), Some("secret-a".into()));
    assert_eq!(
        (
            file.get("A").unwrap(),
            file.get("B").unwrap(),
            file.get("C").unwrap()
        ),
        (None, Some("secret-b".into()), Some("secret-c".into()))
    );
    remove_secrets_dir(&path);
}

#[test]
fn a_hand_over_never_brings_back_an_account_signed_out_meanwhile() {
    let (keychain, path) = hand_over_racing("race-sign-out", |running| {
        running.delete("A").unwrap();
    });
    assert_eq!(keychain.kept.get("A").unwrap(), None);
    assert_eq!(launcher_with(&keychain, &path).get("A").unwrap(), None);
    assert!(!path.exists());
    remove_secrets_dir(&path);
}

#[test]
fn a_hand_over_never_writes_over_a_token_renewed_meanwhile() {
    let (keychain, path) = hand_over_racing("race-renewal", |running| {
        running.set("A", "renewed-a").unwrap();
    });
    assert_eq!(keychain.kept.get("A").unwrap(), Some("renewed-a".into()));
    assert_eq!(
        launcher_with(&keychain, &path).get("A").unwrap(),
        Some("renewed-a".into())
    );
    assert!(!path.exists());
    remove_secrets_dir(&path);
}
