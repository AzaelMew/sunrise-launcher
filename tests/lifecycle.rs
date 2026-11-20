//! Real CLI contenders for launcher data and profile lifecycle admission.
use std::{fs, process::Command};
use sunrise_launcher::data_location::{self, Roots};

#[test]
fn move_excludes_cli_writers_between_settlement_and_copying() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let source = Roots {
        launcher: base.join("launcher"),
        game: base.join("game"),
        build: base.join("build"),
    };
    for dir in [&source.launcher, &source.game, &source.build] {
        fs::create_dir(dir).unwrap();
    }
    let bootstrap = base.join("location.json");
    let point = |path: &std::path::Path, roots: &Roots| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::json!({"version":1,"roots":roots}).to_string(),
        )
        .unwrap();
    };
    point(&bootstrap, &source);
    let alternate = base.join("alternate/location.json");
    point(&alternate, &source);
    // Sharing any one of the physical roots is enough to make this a contender.
    let spare = Roots {
        launcher: base.join("spare/launcher"),
        game: base.join("spare/game"),
        build: base.join("spare/build"),
    };
    for path in [&spare.launcher, &spare.game, &spare.build] {
        fs::create_dir_all(path).unwrap();
    }
    let single_root = ["launcher", "game", "build"].map(|shared| {
        let path = base.join("alternate").join(format!("{shared}.json"));
        let mut roots = spare.clone();
        match shared {
            "launcher" => roots.launcher = source.launcher.clone(),
            "game" => roots.game = source.game.clone(),
            _ => roots.build = source.build.clone(),
        }
        point(&path, &roots);
        path
    });
    let create = |location: &std::path::Path, overrides: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"));
        cmd.args(["instance", "create", "Late Writer"])
            .env("SUNRISE_LAUNCHER_LOCATION_FILE", location)
            .env_remove("SUNRISE_HOME")
            .env_remove("SUNRISE_LAUNCHER_HOME");
        if overrides {
            cmd.env("SUNRISE_HOME", &source.game)
                .env("SUNRISE_LAUNCHER_HOME", &source.launcher);
        }
        cmd.output().unwrap()
    };
    let mut reached_copy = false;
    let moved = data_location::migrate(&source, &base.join("destination"), &bootstrap, |step| {
        if step != "Copying and verifying launcher data" {
            return;
        }
        reached_copy = true;
        let assert_blocked = |path: &std::path::Path, overrides, lock: &str| {
            let blocked = create(path, overrides);
            let error = String::from_utf8_lossy(&blocked.stderr);
            assert_eq!(blocked.status.code(), Some(1), "{blocked:?}");
            assert!(
                error.contains("Cannot lock") && error.contains(lock),
                "{error}"
            );
            assert!(!source.game.join("instances/late-writer").exists());
            assert!(!spare.game.join("instances/late-writer").exists());
        };
        assert_blocked(&bootstrap, false, "migration.lock");
        let root_claim = "another launcher is using or moving this data";
        assert_blocked(&alternate, false, root_claim);
        assert_blocked(&alternate, true, root_claim);
        for path in &single_root {
            assert_blocked(path, false, root_claim);
        }
    })
    .unwrap();
    assert!(reached_copy);
    assert!(!moved.game.join("instances/late-writer").exists());
    let accepted = create(&bootstrap, false);
    assert!(accepted.status.success(), "{accepted:?}");
    assert!(moved.game.join("instances/late-writer").is_dir());
    assert!(!source.game.join("instances/late-writer").exists());
}

#[cfg(unix)]
#[test]
fn a_writable_custom_root_does_not_require_a_writable_parent() {
    use std::{os::unix::fs::PermissionsExt as _, path::PathBuf};
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let parent = base.join("protected");
    let source = Roots {
        launcher: parent.join("launcher"),
        game: parent.join("game"),
        build: parent.join("build"),
    };
    for dir in [&source.launcher, &source.game, &source.build] {
        fs::create_dir_all(dir).unwrap();
    }
    let location = base.join("location.json");
    fs::write(
        &location,
        serde_json::json!({"version":1,"roots":source}).to_string(),
    )
    .unwrap();
    struct RestoreMode(PathBuf);
    impl Drop for RestoreMode {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    let _restore = RestoreMode(parent.clone());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = parent.join("parent-write-probe");
    match fs::write(&probe, b"must be denied") {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied),
        Ok(()) => {
            fs::remove_file(probe).unwrap();
            eprintln!("protected-parent fixture needs an unprivileged account");
            return;
        }
    }
    let made = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
        .args(["instance", "create", "Writable Home"])
        .env("SUNRISE_LAUNCHER_LOCATION_FILE", &location)
        .env_remove("SUNRISE_HOME")
        .env_remove("SUNRISE_LAUNCHER_HOME")
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    assert!(source.game.join("instances/writable-home").is_dir());
    let moved =
        data_location::migrate(&source, &base.join("destination"), &location, |_| {}).unwrap();
    assert!(moved.game.join("instances/writable-home").is_dir());
    assert_eq!(fs::read_dir(parent).unwrap().count(), 3);
}

#[test]
fn profile_copy_and_world_writers_share_admission() {
    use std::io::Write as _;
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    let data = root.path().join("launcher");
    let trash = root.path().join("trash");
    fs::create_dir(&trash).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
            .args(args)
            .current_dir(root.path())
            .env(
                "SUNRISE_LAUNCHER_LOCATION_FILE",
                root.path().join("location.json"),
            )
            .env("SUNRISE_HOME", &game)
            .env("SUNRISE_LAUNCHER_HOME", &data)
            .env("SUNRISE_LAUNCHER_TRASH", &trash)
            .output()
            .unwrap()
    };
    let source = run(&["instance", "create", "Source"]);
    assert!(source.status.success(), "{source:?}");
    let world = game.join("instances/source/saves/Meadow");
    fs::create_dir_all(&world).unwrap();
    fs::write(world.join("level.dat"), b"old world").unwrap();
    let world_lock = fs::File::create(world.join("session.lock")).unwrap();
    world_lock.try_lock().unwrap();
    let copy = || run(&["instance", "create", "Duplicate", "--copy", "Source"]);
    let refused = copy();
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("Could not lock world Meadow"),
        "{refused:?}"
    );
    assert!(!game.join("instances/duplicate").exists());
    world_lock.unlock().unwrap();
    drop(world_lock);

    let path = root.path().join("lantern.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("pack.mcmeta", options).unwrap();
    zip.write_all(br#"{"pack":{"description":"Lanterns"}}"#)
        .unwrap();
    zip.start_file("data/lantern/function/tick.mcfunction", options)
        .unwrap();
    zip.write_all(b"say lantern").unwrap();
    zip.finish().unwrap();
    let add = || {
        run(&[
            "worlds",
            "datapack-add",
            "Meadow",
            "lantern.zip",
            "--instance",
            "Source",
        ])
    };
    let delete = || run(&["worlds", "delete", "Meadow", "--instance", "Source"]);
    // This is the actual name gate Profile Copy keeps through snapshot and publication.
    let owner = fs::File::options()
        .read(true)
        .write(true)
        .open(game.join(".launcher-instance-gates/instances/source"))
        .unwrap();
    owner.try_lock().unwrap();
    for blocked in [delete(), add()] {
        assert_eq!(blocked.status.code(), Some(1), "{blocked:?}");
        assert!(
            String::from_utf8_lossy(&blocked.stderr).contains("profile is in use"),
            "{blocked:?}"
        );
    }
    assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"old world");
    assert!(!world.join("datapacks").exists());
    owner.unlock().unwrap();
    drop(owner);

    let accepted = add();
    assert!(accepted.status.success(), "{accepted:?}");
    let copied = copy();
    assert!(copied.status.success(), "{copied:?}");
    let snapshot = game.join("instances/duplicate/saves/Meadow");
    assert_eq!(
        fs::read(snapshot.join("datapacks/lantern.zip")).unwrap(),
        fs::read(path).unwrap()
    );
    assert!(!snapshot.join("session.lock").exists());
    let deleted = delete();
    assert!(deleted.status.success(), "{deleted:?}");
    assert!(!world.exists());
    assert!(snapshot.join("level.dat").is_file());
}
