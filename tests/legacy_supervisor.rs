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

#[test]
fn forwards_to_a_running_supervisor_when_the_engine_command_probe_fails() {
    use std::io::{Read, Write};
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join("launcher");
    let server = data.join("servers/friends");
    let runtime = data.join("server-runtime");
    let release = runtime.join("releases/current-build");
    fs::create_dir_all(&release).unwrap();
    fs::create_dir_all(server.join(".sunrise")).unwrap();
    symlink(&release, runtime.join("current")).unwrap();
    fs::write(
        data.join("servers.json"),
        serde_json::json!([{"name":"Friends", "dir":server}]).to_string(),
    )
    .unwrap();
    // The running supervisor's control API answers its status.
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    fs::write(
        server.join(".sunrise/api.json"),
        serde_json::json!({"address": api.local_addr().unwrap().to_string(), "token": "t"})
            .to_string(),
    )
    .unwrap();
    std::thread::spawn(move || {
        for mut stream in api.incoming().flatten() {
            let _ = stream.read(&mut [0; 2048]);
            let body = r#"{"phase":"running"}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    // Its engine-command probe fails: the game files are missing.
    let program = release.join("sunrise-server");
    fs::write(
        &program,
        r#"#!/bin/sh
case "$3" in
    engine-command) echo '{"format":1,"error":{"kind":"resources","message":"missing"}}'; exit 1 ;;
    restart) printf '%s\n' "$3" >> "$2/forwarded" ;;
    *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
        .args(["servers", "run", "Friends", "--", "restart"])
        .env("SUNRISE_LAUNCHER_HOME", &data)
        .env("SUNRISE_HOME", home.path().join("game"))
        .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(server.join("forwarded")).unwrap(),
        "restart\n"
    );
    assert!(!release.join(".installed").exists());
}
