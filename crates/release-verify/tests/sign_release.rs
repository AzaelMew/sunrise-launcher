//! tools/ci/sign_release.sh and verify-launcher-release agree: a folder the script signs passes the check, and another tag, a changed file or an unlisted one fails it.
#![cfg(unix)]
use std::path::Path;
use std::process::Command;

#[test]
fn a_release_the_script_signs_passes_the_check_and_a_changed_one_fails_it() {
    let dist = std::env::temp_dir().join(format!("sunrise-launcher-sign-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dist);
    std::fs::create_dir_all(&dist).unwrap();
    let archive = dist.join("sunrise-launcher-linux-x86_64.tar.gz");
    std::fs::write(&archive, b"launcher").unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ci/sign_release.sh");
    let signed = Command::new("bash")
        .arg(&script)
        .arg("v1.2.3")
        .arg(&archive)
        .env("EPHEMERAL", "yes")
        .status()
        .unwrap();
    assert!(signed.success());
    let key = std::fs::read_to_string(dist.join("ephemeral.pub")).unwrap();
    std::fs::remove_file(dist.join("ephemeral.pub")).unwrap();
    let passes = |tag: &str| {
        Command::new(env!("CARGO_BIN_EXE_verify-launcher-release"))
            .args(["--tag", tag, "--key", key.trim()])
            .arg(&dist)
            .status()
            .unwrap()
            .success()
    };
    assert!(passes("v1.2.3"));
    assert!(!passes("v1.2.4"));
    assert!(!passes("v1.2.3\nx"));
    let unlisted = dist.join("sunrise-launcher-extra.zip");
    std::fs::write(&unlisted, b"unsigned").unwrap();
    assert!(!passes("v1.2.3"));
    std::fs::remove_file(&unlisted).unwrap();
    assert!(passes("v1.2.3"));
    std::fs::write(&archive, b"changed").unwrap();
    assert!(!passes("v1.2.3"));
    std::fs::remove_dir_all(&dist).unwrap();
}
