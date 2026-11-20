//! Exercise the same process entry used for native compiler invocations.
#![cfg(target_os = "linux")]

use std::{fs, io::Write, process::Command};
use sunrise_launcher::process::compiler_worker::ARG;

#[test]
fn an_escaped_descendant_is_not_a_proved_compiler_exit() {
    let isolated = tempfile::tempdir().unwrap();
    let root = isolated.path();
    let worker = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"));
        cmd.arg(ARG).arg("/bin/sh").arg("-ec");
        cmd
    };
    assert_eq!(
        worker().arg("( exit 0 ); exit 0").status().unwrap().code(),
        Some(0)
    );
    assert_eq!(worker().arg("exit 1").status().unwrap().code(), Some(1));
    let missing = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
        .arg(ARG)
        .arg(root.join("missing-tool"))
        .status()
        .unwrap();
    assert_eq!(missing.code(), Some(124));
    assert_eq!(worker().arg("exit 124").status().unwrap().code(), Some(125));
    for name in ["started", "release", "finished"] {
        assert!(Command::new("mkfifo")
            .arg(root.join(name))
            .status()
            .unwrap()
            .success());
    }
    let result = worker().args([
        r#"setsid /bin/sh -ec 'printf "started\n" > "$1/started"; read release < "$1/release"; printf "wrote old tree\n" > "$1/finished"' worker "$1" &
        read started < "$1/started"
        exit 0"#, "test",
    ]).arg(root).status().unwrap();
    // Unblock and observe the escaped writer before asserting. The old private tree still
    // belongs to it even though the top-level compiler already exited successfully.
    fs::OpenOptions::new()
        .write(true)
        .open(root.join("release"))
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("finished")).unwrap(),
        "wrote old tree\n"
    );
    assert_eq!(result.code(), Some(125));
}
