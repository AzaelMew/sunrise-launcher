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
    fs::write(
        &bootstrap,
        serde_json::json!({"version":1,"roots":source}).to_string(),
    )
    .unwrap();
    let create = |overrides: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"));
        cmd.args(["instance", "create", "Late Writer"])
            .env("SUNRISE_LAUNCHER_LOCATION_FILE", &bootstrap)
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
        for overrides in [false, true] {
            let blocked = create(overrides);
            let error = String::from_utf8_lossy(&blocked.stderr);
            assert_eq!(blocked.status.code(), Some(1), "{blocked:?}");
            assert!(
                error.contains("Cannot lock") && error.contains("migration.lock"),
                "{error}"
            );
            assert!(!source.game.join("instances/late-writer").exists());
        }
    })
    .unwrap();
    assert!(reached_copy);
    assert!(!moved.game.join("instances/late-writer").exists());
    let accepted = create(false);
    assert!(accepted.status.success(), "{accepted:?}");
    assert!(moved.game.join("instances/late-writer").is_dir());
    assert!(!source.game.join("instances/late-writer").exists());
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
