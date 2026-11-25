//! Editing through a transaction: the pointer, rollback before the commit marker, sealing, and the proof a first binding keeps.

use super::*;

#[test]
fn files_only_edit_keeps_the_exact_pointer_and_rolls_back_before_the_commit_marker() {
    for has_pointer in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let before = has_pointer.then(|| OLD.to_vec());
        if has_pointer {
            fs::create_dir(home.join("mod-browser")).unwrap();
            fs::write(home.join(POINTER), OLD).unwrap();
        }
        let mut owner = Admission::open(home, None).unwrap();
        for commit in [false, true] {
            let stage = owner
                .reserve(Intent::Install, BTreeSet::from(["nightly-old".into()]))
                .unwrap();
            let candidate = stage.path().join("candidate.smod");
            fs::write(&candidate, package("2.0.0")).unwrap();
            let result = stage
                .seal_files(None, &[(PACK.into(), Some(candidate))])
                .unwrap()
                .publish_observed(
                    || Ok(()),
                    |_, boundary| {
                        let refs = references(home)?;
                        assert_eq!(refs.pointer, before);
                        assert!(refs.after.is_none());
                        if boundary == Boundary::BeforeMarker && !commit {
                            return Err("interrupted after the file renames".into());
                        }
                        Ok(())
                    },
                );
            assert_eq!(result.is_ok(), commit);
            assert_eq!(pointer_bytes(home).unwrap(), before);
            assert_eq!(home.join(PACK).is_file(), commit);
            assert!(!recovery_pending(home).unwrap());
        }
    }
}

#[test]
fn first_binding_and_signed_proof_survive_an_abandoned_prepare_and_pointer_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mut owner = Admission::open(home, None).unwrap();
    assert!(owner
        .bind_bootstrap(
            || Ok(first_binding()),
            |_| Err::<(), _>("asset gone".into())
        )
        .is_err());
    assert!(!home.join(RECORD).exists());
    let chosen = owner
        .bind_bootstrap(|| Ok(first_binding()), |_| Ok(()))
        .unwrap();
    let proven = owner
        .prove_bootstrap(|binding| {
            assert_eq!(binding, &chosen);
            Ok(serde_json::json!({"sha256":"fixture-signed-proof"}))
        })
        .unwrap();
    assert!(proven.proof.is_some());
    let refs = references(home).unwrap();
    assert_eq!(refs.targets, chosen.targets());
    assert!(refs.before.is_none() && refs.after.is_none() && refs.pointer.is_none());
    drop(owner);

    for boundary in ["bootstrap:prepare", "bootstrap:pointer-renamed"] {
        let actor = pause_actor(home, boundary);
        assert!(
            Admission::open(home, None).is_err(),
            "ongoing first Play holds admission"
        );
        let refs = references(home).unwrap(); // Reader takes no profile admission.
        assert_eq!(refs.targets, chosen.targets());
        assert!(refs.before.is_none());
        if boundary.ends_with("prepare") {
            assert!(refs.after.is_none() && refs.pointer.is_none());
        } else {
            assert_eq!(refs.after.as_deref(), Some(FIRST_POINTER));
            assert_eq!(refs.pointer.as_deref(), Some(FIRST_POINTER));
        }
        let stage = read(home)
            .unwrap()
            .unwrap()
            .operation()
            .unwrap()
            .stage
            .path(home);
        drop(actor);
        let mut next = Admission::open(home, None).unwrap();
        assert!(pointer_bytes(home).unwrap().is_none());
        assert!(!stage.exists(), "settlement retires only the old attempt");
        assert_eq!(
            next.bind_bootstrap::<()>(
                || panic!("new Nightly must not be selected"),
                |_| panic!("existing reference is not being created")
            )
            .unwrap(),
            proven
        );
        assert_eq!(
            next.prove_bootstrap(|_| panic!("new SDK proof must not be fetched"))
                .unwrap(),
            proven
        );
    }
    let mut owner = Admission::open(home, None).unwrap();
    owner
        .resume_bootstrap()
        .unwrap()
        .seal(FIRST_POINTER.to_vec(), &[])
        .unwrap()
        .publish(|| Ok(()))
        .unwrap();
    let refs = references(home).unwrap();
    assert!(refs.targets.is_empty() && refs.before.is_none() && refs.after.is_none());
    assert_eq!(refs.pointer.as_deref(), Some(FIRST_POINTER));
    assert!(!home.join(RECORD).exists());
    assert!(owner
        .bind_bootstrap(|| panic!("a committed profile cannot rebind"), |_| Ok(()))
        .is_err());
}

#[test]
fn settling_a_failed_first_play_keeps_an_unresolved_publication() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mut owner = Admission::open(home, None).unwrap();
    owner
        .bind_bootstrap(|| Ok(first_binding()), |_| Ok(()))
        .unwrap();
    let proof = owner
        .prove_bootstrap(|_| Ok(serde_json::json!({"fixture": 1})))
        .unwrap();
    let _sealed = owner
        .resume_bootstrap()
        .unwrap()
        .seal(FIRST_POINTER.to_vec(), &[])
        .unwrap();
    owner.release_failed_bootstrap().unwrap();
    let refs = references(home).unwrap();
    assert_eq!(refs.after.as_deref(), Some(FIRST_POINTER));
    assert!(refs.pointer.is_none());
    drop(owner);
    let mut next = Admission::open(home, None).unwrap();
    assert_eq!(
        next.prove_bootstrap(|_| panic!("kept exact proof"))
            .unwrap(),
        proof
    );
}

#[test]
fn candidate_targets_are_durable_before_sealing_and_rollback_retires_only_those_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mut owner = Admission::open(home, None).unwrap();
    let first = owner
        .bind_bootstrap(|| Ok(first_binding()), |_| Ok(()))
        .unwrap();
    let proven = owner
        .prove_bootstrap(|_| Ok(serde_json::json!({"fixture": 1})))
        .unwrap();
    let stage = owner.resume_bootstrap().unwrap();
    assert!(stage.retain_target("../untrusted").is_err());
    stage.retain_target("staged-native-image").unwrap();
    let mut pending = first.targets();
    pending.insert("staged-native-image".into());
    assert_eq!(references(home).unwrap().targets, pending);
    let sealed = stage.seal(FIRST_POINTER.to_vec(), &[]).unwrap();
    assert_eq!(references(home).unwrap().targets, pending);
    assert!(sealed
        .publish(|| Err::<(), _>("the index changed".into()))
        .is_err());
    assert_eq!(references(home).unwrap().targets, first.targets());
    assert!(pointer_bytes(home).unwrap().is_none());
    assert_eq!(
        owner.prove_bootstrap(|_| panic!("kept proof")).unwrap(),
        proven
    );
}

#[test]
fn reference_snapshot_reads_exact_old_and_new_pointers_and_refuses_unowned_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut owner = Admission::open(home, None).unwrap();
    let staged = seal(&mut owner, false);
    let refs = references(home).unwrap();
    assert_eq!(refs.before.as_deref(), Some(OLD));
    assert_eq!(refs.after.as_deref(), Some(INSTALLED));
    assert_eq!(refs.pointer.as_deref(), Some(OLD));
    fs::write(home.join(POINTER), SWITCHED).unwrap();
    assert!(references(home).is_err());
    fs::write(home.join(POINTER), OLD).unwrap();
    drop(staged);
    drop(owner);
    drop(Admission::open(home, None).unwrap());
    fs::write(home.join(RECORD), b"{\"version\":1}").unwrap();
    assert!(references(home).is_err());
}

#[test]
fn refused_cutoff_keeps_the_original_bytes_and_success_uses_the_sealed_pointer() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut admission = Admission::open(home, None).unwrap();
    let cause = seal(&mut admission, false)
        .publish(|| Err::<(), _>("declined".into()))
        .unwrap_err();
    assert_eq!(cause, "declined");
    pair(home, false, false);
    assert!(!home.join(RECORD).exists());
    seal(&mut admission, false).publish(|| Ok(())).unwrap();
    pair(home, false, true);
    assert!(!home.join(RECORD).exists());
    assert!(!home.join(RETIRED).exists());
}

#[test]
fn torn_marker_enospc_undo_consumes_the_old_pointer_and_reuses_both_progress_slots() {
    use progress::{Phase, Slots};
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut admission = Admission::open(home, None).unwrap();
    let sealed = seal(&mut admission, false);
    let disk = read(home).unwrap().unwrap();
    let op = disk.operation().unwrap();
    let steps = op.ready.as_ref().unwrap().steps();
    let stage = op.stage.path(home);
    let old = stage.join("pointer-old");
    let pointer_preimage = Identity::file_at(&old).unwrap().unwrap();
    assert_eq!(fs::read(&old).unwrap(), OLD);
    let progress_file = stage.join("progress");
    let slots_preimage = Identity::file_at(&progress_file).unwrap().unwrap();
    let live_record = home.join(RECORD);
    let record_preimage = Identity::file_at(&live_record).unwrap().unwrap();
    progress::fail_marker(progress::MarkerFault::TornWrite);
    let error = disk.publish_at(home, || Ok(()), |_, _| Ok(())).unwrap_err();
    assert_eq!(error, at(&progress_file, progress::storage_full()));
    pair(home, false, true);
    let slots = || {
        Slots::open(&stage, disk.digest().unwrap(), steps)
            .unwrap()
            .current()
    };
    assert_eq!(
        (slots().phase, slots().frontier),
        (Phase::Publishing, steps)
    );
    drop(sealed);
    drop(admission);
    for (index, boundary) in ["undo-pointer", "undo-list"].into_iter().enumerate() {
        let actor = pause_actor(home, boundary);
        assert!(Admission::open(home, None).is_err());
        let progress = slots();
        assert_eq!(
            (progress.phase, progress.frontier),
            (Phase::Undoing, steps - index)
        );
        assert!(
            absent(&old).unwrap(),
            "restore must consume its staged preimage"
        );
        assert!(Identity::file_at(&home.join(POINTER)).unwrap() == Some(pointer_preimage));
        assert!(Identity::file_at(&progress_file).unwrap() == Some(slots_preimage));
        assert!(Identity::file_at(&live_record).unwrap() == Some(record_preimage));
        drop(actor);
    }
    drop(Admission::open(home, None).unwrap());
    pair(home, false, false);
    assert!(!live_record.exists());
    assert!(!stage.exists());
}

#[test]
fn whole_marker_after_failed_flush_is_retried_and_never_undone() {
    for attempts in [1, 2] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        initialized(home, true);
        let mut admission = Admission::open(home, None).unwrap();
        let sealed = seal(&mut admission, false);
        let stage = sealed.0.path().to_owned();
        progress::fail_marker(progress::MarkerFault::Flush(attempts));
        let published = sealed.publish(|| Ok(()));
        if attempts == 1 {
            assert!(
                published.is_ok(),
                "a successful reflush confirms the commit"
            );
        } else {
            assert_eq!(
                published.unwrap_err(),
                at(&stage.join("progress"), progress::storage_full())
            );
            assert!(stage.join("pointer-old").is_file());
            assert!(home.join(RECORD).is_file());
        }
        pair(home, false, true);
        drop(admission);
        drop(Admission::open(home, None).unwrap());
        pair(home, false, true);
        assert!(!home.join(RECORD).exists() && !stage.exists());
    }
}
