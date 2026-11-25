//! Admission: the gate a staged worker holds, and sealing once it exits.

use super::*;

#[cfg(unix)]
#[test]
fn admission_without_successful_handoff_does_not_leave_an_incidental_fork_holding_the_gate() {
    use crate::process::{leases::StagedChild, owned::Owned};
    for fail_spawn in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        initialized(home, true);
        let owner = Admission::open(home, None).unwrap();
        // A fork before exec has the same open description, even with CLOEXEC set.
        let not_an_admitted_child = owner.gate.file.try_clone().unwrap();
        if fail_spawn {
            let child = Owned::spawn_staged(
                std::process::Command::new(home.join("no-such-executable")),
                owner.handoff(),
                StagedChild::Prepare,
            );
            assert_eq!(child.err().unwrap().kind(), io::ErrorKind::NotFound);
        }
        drop(owner);
        assert!(Admission::open(home, None).is_ok());
        drop(not_an_admitted_child);
    }
}

#[test]
fn staged_child_survives_launcher_death_and_blocks_an_already_open_launcher() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut contender = pause_actor(home, "check-exclusion");
    let launcher = pause_actor(home, "launcher-with-staged-worker");
    let stage = read(home)
        .unwrap()
        .unwrap()
        .operation()
        .unwrap()
        .stage
        .path(home);
    drop(launcher); // Kill the real owner process, not merely its Rust lock object.
    contender
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert!(contender.0.wait().unwrap().success());
    assert_eq!(
        fs::read(stage.join("worker.smod")).unwrap(),
        package("2.0.0")
    );
    fs::write(stage.join("finish-worker"), []).unwrap();
    await_fixture(|| Admission::open(home, None).is_ok());
    assert!(!stage.exists() && !home.join(RECORD).exists());
    pair(home, false, false);
}

#[test]
fn the_same_admission_seals_after_a_staged_worker_exits() {
    use crate::process::{leases::StagedChild, owned::Owned};
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut owner = Admission::open(home, None).unwrap();
    let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
    let mut worker = Owned::spawn_staged(
        actor_command(home, "staged-worker"),
        stage.handoff(),
        StagedChild::Compiler,
    )
    .unwrap();
    await_fixture(|| stage.path().join("worker-ready").is_dir());
    assert!(Admission::open(home, None).is_err());
    fs::write(stage.path().join("finish-worker"), []).unwrap();
    assert!(worker.wait().unwrap().success());
    assert!(
        Admission::open(home, None).is_err(),
        "worker exit must not unlock the parent"
    );
    let output = stage.path().join("worker.smod");
    stage
        .seal(
            INSTALLED.to_vec(),
            &[(PACK.into(), Some(output)), (LIST.into(), None)],
        )
        .unwrap()
        .publish(|| Ok(()))
        .unwrap();
    assert_eq!(fs::read(home.join(POINTER)).unwrap(), INSTALLED);
    assert_eq!(fs::read(home.join(PACK)).unwrap(), package("2.0.0"));
    assert!(
        Admission::open(home, None).is_err(),
        "publication keeps the owning profile"
    );
    drop(owner);
    // An unrelated fork may still briefly own a CLOEXEC reference to the handed-off file.
    await_fixture(|| Admission::open(home, None).is_ok());
}
