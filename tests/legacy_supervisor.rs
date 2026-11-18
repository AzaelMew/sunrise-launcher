//! `servers run` also works with a prepared supervisor that predates `engine-command`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::process::Command;

#[test]
fn forwards_start_restart_and_run_to_a_pre_protocol_supervisor() {
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join("launcher");
    let server = data.join("servers/friends");
    let runtime = data.join("server-runtime");
    let release = runtime.join("releases/legacy");
    fs::create_dir_all(&release).unwrap();
    fs::create_dir_all(&server).unwrap();
    symlink(&release, runtime.join("current")).unwrap();
    fs::write(
        data.join("servers.json"),
        serde_json::json!([{"name":"Friends", "dir":server}]).to_string(),
    )
    .unwrap();
    let program = release.join("sunrise-server");
    fs::write(&program, r#"#!/bin/sh
case "$3" in
    engine-command) echo 'sunrise-server: unknown command engine-command; `sunrise-server help` lists them' >&2; exit 1 ;;
    start|restart|run) printf '%s\n' "$3" >> "$2/forwarded" ;;
    *) exit 2 ;;
esac
"#).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    for verb in ["start", "restart", "run"] {
        let output = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
            .args(["servers", "run", "Friends", "--", verb])
            .env("SUNRISE_LAUNCHER_HOME", &data)
            .env("SUNRISE_HOME", home.path().join("game"))
            .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{verb}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(server.join("forwarded")).unwrap(),
        "start\nrestart\nrun\n"
    );
    assert!(!release.join(".installed").exists());
    assert!(sunrise_launcher_smod::runtime_leases::Registry::existing(&data).is_err());
}
