//! Undoing and restoring: permissions, occupied stage destinations and private directory replay.

use super::*;

#[cfg(unix)]
#[test]
fn undo_restores_restricted_pointer_and_package_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    for (file, mode) in [(POINTER, 0o600), (PACK, 0o640), (LIST, 0o604)] {
        fs::set_permissions(home.join(file), fs::Permissions::from_mode(mode)).unwrap();
    }
    drop(pause_actor(home, "install:pointer-renamed"));
    assert_eq!(fs::read(home.join(POINTER)).unwrap(), INSTALLED);
    drop(Admission::open(home, None).unwrap());
    pair(home, false, false);
    for (file, mode) in [(POINTER, 0o600), (PACK, 0o640), (LIST, 0o604)] {
        assert_eq!(
            fs::metadata(home.join(file)).unwrap().permissions().mode() & 0o7777,
            mode
        );
    }
}

#[test]
fn directory_restore_does_not_replace_an_occupied_stage_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, false);
    let mut owner = Admission::open(home, None).unwrap();
    let sealed = seal(&mut owner, true);
    let record = &sealed.0.record;
    let op = record.operation().unwrap();
    let plan = op.ready.as_ref().unwrap();
    let stage = op.stage.path(home);
    let (src, dst, id) = plan.directory(home, &stage, 0).unwrap();
    let mut slots = progress::Slots::open(&stage, record.digest().unwrap(), plan.steps()).unwrap();
    slots.advance(progress::Phase::Publishing, 1).unwrap();
    drop(slots);
    durable::move_new_directory(&src, &dst).unwrap();
    fs::create_dir(&src).unwrap();
    assert!(record.complete(home, |_| {}, || {}).is_err());
    assert!(Identity::of(&dst).unwrap() == id);
    assert!(Identity::of(&src).unwrap() != id);
    fs::remove_dir(&src).unwrap();
    assert!(!record.complete(home, |_| {}, || {}).unwrap());
    pair(home, true, false);
}

#[cfg(windows)]
#[test]
fn interrupted_private_directory_replay_recovers_only_its_recorded_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, false);
    let mut owner = Admission::open(home, None).unwrap();
    let sealed = seal(&mut owner, true);
    let record = &sealed.0.record;
    let op = record.operation().unwrap();
    let plan = op.ready.as_ref().unwrap();
    let stage = op.stage.path(home);
    let (src, dst, id) = plan.directory(home, &stage, 0).unwrap();
    let mut slots = progress::Slots::open(&stage, record.digest().unwrap(), plan.steps()).unwrap();
    slots.advance(progress::Phase::Publishing, 1).unwrap();
    slots.advance(progress::Phase::Undoing, 1).unwrap();
    drop(slots);
    let replay = src.with_extension("undo-directory");
    // Recreate a process killed during the private barrier's first rename.
    durable::move_new_directory(&src, &replay).unwrap();
    assert!(absent(&dst).unwrap() && Identity::of(&replay).unwrap() == id);
    assert!(!record.complete(home, |_| {}, || {}).unwrap());
    assert!(absent(&replay).unwrap());
    pair(home, true, false);
}
