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
