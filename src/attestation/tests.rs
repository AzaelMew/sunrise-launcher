use super::*;
use serde_json::Value;

const RUFF_BUNDLE: &[u8] =
    include_bytes!("../../crates/attestation/tests/fixtures/ruff-source.sigstore.json");
const RUFF_DIGEST: [u8; 32] = [
    0x03, 0x78, 0xef, 0x60, 0x35, 0x6c, 0xaa, 0xad, 0x7b, 0x84, 0x65, 0x36, 0x91, 0xa5, 0xd5, 0x7a,
    0x1d, 0x8b, 0x57, 0xfd, 0x59, 0xd1, 0x73, 0xcf, 0xc2, 0x7b, 0x0f, 0xd4, 0xa9, 0x1a, 0x13, 0xf3,
];
const RUFF_DIGEST_HEX: &str = "0378ef60356caaad7b84653691a5d57a1d8b57fd59d173cfc27b0fd4a91a13f3";
const RUFF_REPO_ID: u64 = 523_043_277;
const RUFF_WORKFLOW: &str = "astral-sh/ruff/.github/workflows/release.yml@refs/heads/main";
const SUNRISE_WORKFLOW_V1: &str =
    "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1";
const SUNRISE_WORKFLOW_V2: &str =
    "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v2";

fn sample_attested(repository_id: u64, repository_name: &str, workflow_ref: &str) -> Attested {
    Attested {
        repository_id,
        owner_id: repository_id,
        repository_name: repository_name.to_owned(),
        signer_workflow_ref: workflow_ref.to_owned(),
        source_ref: "refs/tags/v1.0.0".to_owned(),
        source_commit: "a7ba68b4919e56b6af949a42b2da38fe4eec67a9".to_owned(),
        predicate_type: "https://slsa.dev/provenance/v1".to_owned(),
    }
}

#[test]
fn good_bundle_verifies_offline_and_decodes_hex_digest() {
    assert_eq!(parse_sha256_hex(RUFF_DIGEST_HEX), Some(RUFF_DIGEST));
    assert_eq!(
        parse_sha256_hex(&RUFF_DIGEST_HEX.to_ascii_uppercase()),
        Some(RUFF_DIGEST)
    );
    assert_eq!(parse_sha256_hex("deadbeef"), None);
    assert_eq!(parse_sha256_hex(&"z".repeat(64)), None);

    let attested = verify_local(RUFF_BUNDLE, RUFF_DIGEST).unwrap();
    assert_eq!(attested.repository_id, RUFF_REPO_ID);
    assert_eq!(attested.repository_name, "astral-sh/ruff");
    assert_eq!(attested.signer_workflow_ref, RUFF_WORKFLOW);
    assert_eq!(attested.source_ref, "refs/heads/main");
    assert_eq!(
        attested.source_commit,
        "a7ba68b4919e56b6af949a42b2da38fe4eec67a9"
    );
    assert_eq!(attested.predicate_type, "https://slsa.dev/provenance/v1");
}

#[test]
fn wrong_digest_is_rejected() {
    let expected = ExpectedRelease {
        repository_id: RUFF_REPO_ID,
        workflow: SUNRISE_WORKFLOW_V1,
    };
    let err = verify_release(RUFF_BUNDLE, [0u8; 32], &expected).unwrap_err();
    assert!(
        matches!(err, Error::Verify(ref inner) if matches!(inner.as_ref(), sunrise_attestation::Error::Unverified(_))),
        "unexpected error: {err:?}"
    );
}

#[test]
fn tampered_bundle_is_rejected() {
    let expected = ExpectedRelease {
        repository_id: RUFF_REPO_ID,
        workflow: SUNRISE_WORKFLOW_V1,
    };

    // Tampered DSSE signature
    let mut bundle: Value = serde_json::from_slice(RUFF_BUNDLE).unwrap();
    let sig = &mut bundle["dsseEnvelope"]["signatures"][0]["sig"];
    let mut changed = sig.as_str().unwrap().to_owned();
    let i = changed.len() - 8;
    changed.replace_range(i..i + 1, if &changed[i..i + 1] == "A" { "B" } else { "A" });
    *sig = changed.into();
    let tampered_sig = serde_json::to_vec(&bundle).unwrap();
    assert!(matches!(
        verify_release(&tampered_sig, RUFF_DIGEST, &expected),
        Err(Error::Verify(_))
    ));

    // Tampered Rekor inclusion proof
    let mut bundle: Value = serde_json::from_slice(RUFF_BUNDLE).unwrap();
    bundle["verificationMaterial"]["tlogEntries"][0]["inclusionProof"]["hashes"][0] =
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
    let tampered_log = serde_json::to_vec(&bundle).unwrap();
    assert!(matches!(
        verify_release(&tampered_log, RUFF_DIGEST, &expected),
        Err(Error::Verify(_))
    ));

    // Malformed JSON
    assert!(matches!(
        verify_release(b"{not valid json", RUFF_DIGEST, &expected),
        Err(Error::Verify(_))
    ));
}

#[test]
fn wrong_repository_is_rejected() {
    let expected = ExpectedRelease {
        repository_id: RUFF_REPO_ID + 1,
        workflow: RUFF_WORKFLOW,
    };
    let err = verify_release(RUFF_BUNDLE, RUFF_DIGEST, &expected).unwrap_err();
    assert!(
        matches!(
            err,
            Error::WrongRepository {
                expected,
                actual,
                ref actual_name,
            } if expected == RUFF_REPO_ID + 1
                && actual == RUFF_REPO_ID
                && actual_name == "astral-sh/ruff"
        ),
        "unexpected error: {err:?}"
    );
    assert!(err.to_string().contains("astral-sh/ruff"));
}

#[test]
fn wrong_workflow_and_ref_are_rejected() {
    // Real bundle signed by astral-sh/ruff fails Sunrise's workflow policy even when expected matches it.
    let ruff_expected = ExpectedRelease {
        repository_id: RUFF_REPO_ID,
        workflow: RUFF_WORKFLOW,
    };
    assert!(matches!(
        verify_release(RUFF_BUNDLE, RUFF_DIGEST, &ruff_expected),
        Err(Error::UntrustedWorkflow { .. })
    ));

    // Valid Sunrise release workflow tags pass when matching expected.workflow.
    for workflow in [
        SUNRISE_WORKFLOW_V1,
        SUNRISE_WORKFLOW_V2,
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v42",
    ] {
        let attested = sample_attested(1234, "alice/mod-a", workflow);
        let verified = check_release_policy(
            attested,
            &ExpectedRelease {
                repository_id: 1234,
                workflow,
            },
        )
        .unwrap();
        assert_eq!(verified.workflow_path, RELEASE_WORKFLOW_PATH);
        assert!(verified
            .workflow_ref
            .starts_with(RELEASE_WORKFLOW_REF_PREFIX));
    }

    // Valid Sunrise workflow that does not match the release's listed workflow field is rejected.
    let attested = sample_attested(1234, "alice/mod-a", SUNRISE_WORKFLOW_V1);
    let err = check_release_policy(
        attested,
        &ExpectedRelease {
            repository_id: 1234,
            workflow: SUNRISE_WORKFLOW_V2,
        },
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            Error::WorkflowMismatch {
                ref expected,
                ref actual,
            } if expected == SUNRISE_WORKFLOW_V2 && actual == SUNRISE_WORKFLOW_V1
        ),
        "unexpected error: {err:?}"
    );

    // Wrong paths, branches, non-numeric tags, prefixes, or missing '@' are all rejected.
    for bad in [
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/heads/main",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/v1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1-rc1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1/extra",
        "sunrise-org/sunrise-mods/.github/workflows/other.yml@refs/tags/release-v1",
        "other-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1",
        "https://github.com/sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml",
    ] {
        let attested = sample_attested(1234, "alice/mod-a", bad);
        let err = check_release_policy(
            attested,
            &ExpectedRelease {
                repository_id: 1234,
                workflow: bad,
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::UntrustedWorkflow { .. }),
            "expected UntrustedWorkflow for {bad}, got {err:?}"
        );
    }
}

#[test]
fn pin_allows_same_repo_and_workflow_path_across_release_tags_and_renames() {
    let dir = tempfile::tempdir().unwrap();
    let v1 = check_release_policy(
        sample_attested(1001, "alice/mod-a", SUNRISE_WORKFLOW_V1),
        &ExpectedRelease {
            repository_id: 1001,
            workflow: SUNRISE_WORKFLOW_V1,
        },
    )
    .unwrap();
    assert_eq!(
        check_pin_in(dir.path(), "Ab3dE9xQ", &v1.pin()).unwrap(),
        PinStatus::FirstInstall
    );
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &v1.pin()).unwrap(),
        PinStatus::FirstInstall
    );

    // Newer release-v2 tag on the same workflow path is not a publisher change.
    let v2 = check_release_policy(
        sample_attested(1001, "alice/mod-a", SUNRISE_WORKFLOW_V2),
        &ExpectedRelease {
            repository_id: 1001,
            workflow: SUNRISE_WORKFLOW_V2,
        },
    )
    .unwrap();
    assert_eq!(
        check_pin_in(dir.path(), "Ab3dE9xQ", &v2.pin()).unwrap(),
        PinStatus::Matched
    );
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &v2.pin()).unwrap(),
        PinStatus::Matched
    );

    // Repository rename with the same numeric repository_id is not a publisher change.
    let renamed = check_release_policy(
        sample_attested(1001, "alice/mod-a-renamed", SUNRISE_WORKFLOW_V2),
        &ExpectedRelease {
            repository_id: 1001,
            workflow: SUNRISE_WORKFLOW_V2,
        },
    )
    .unwrap();
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &renamed.pin()).unwrap(),
        PinStatus::Matched
    );
    let pins = load_pins_in(dir.path()).unwrap();
    assert_eq!(pins["Ab3dE9xQ"].repository_name, "alice/mod-a-renamed");

    // Another project is pinned independently.
    let other = PublisherPin {
        owner_id: None,
        repository_id: 2002,
        repository_name: "bob/mod-b".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    assert_eq!(
        pin_or_check_in(dir.path(), "Zz91kLmN", &other).unwrap(),
        PinStatus::FirstInstall
    );
    assert_eq!(load_pins_in(dir.path()).unwrap().len(), 2);
}

#[test]
fn pin_rejects_changed_repository_or_workflow_path_until_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let initial = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &initial).unwrap(),
        PinStatus::FirstInstall
    );

    // Different repository ID triggers PublisherChanged without overwriting disk.
    let moved_repo = PublisherPin {
        owner_id: None,
        repository_id: 2002,
        repository_name: "bob/mod-a-fork".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let err = pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved_repo).unwrap_err();
    let moved_repo_change = err
        .publisher_change()
        .expect("must return PublisherChanged")
        .clone();
    assert_eq!(moved_repo_change.project_id, "Ab3dE9xQ");
    assert!(moved_repo_change.repository_changed());
    assert!(!moved_repo_change.workflow_changed());
    assert_eq!(moved_repo_change.pinned, initial);
    assert_eq!(moved_repo_change.incoming, moved_repo);
    assert!(err.to_string().contains("alice/mod-a"));
    assert!(err.to_string().contains("1001"));
    assert!(err.to_string().contains("bob/mod-a-fork"));
    assert!(err.to_string().contains("2002"));
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], initial);

    // Different workflow path triggers PublisherChanged without overwriting disk.
    let moved_workflow = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: "sunrise-org/sunrise-mods/.github/workflows/release-v2.yml".to_owned(),
    };
    let err = check_pin_in(dir.path(), "Ab3dE9xQ", &moved_workflow).unwrap_err();
    let change = err
        .publisher_change()
        .expect("must return PublisherChanged");
    assert!(!change.repository_changed());
    assert!(change.workflow_changed());
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], initial);

    // Explicit acceptance updates the pin; subsequent installs match it.
    accept_pin_in(dir.path(), &moved_repo_change).unwrap();
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved_repo).unwrap(),
        PinStatus::Matched
    );
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], moved_repo);
}

#[test]
fn stale_accept_does_not_overwrite_a_newer_pin() {
    let dir = tempfile::tempdir().unwrap();
    let pin_a = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let pin_b = PublisherPin {
        owner_id: None,
        repository_id: 2002,
        repository_name: "bob/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let pin_c = PublisherPin {
        owner_id: None,
        repository_id: 3003,
        repository_name: "carol/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &pin_a).unwrap(),
        PinStatus::FirstInstall
    );

    // Prompt observes A -> B while another install observes and accepts A -> C.
    let change_ab = pin_or_check_in(dir.path(), "Ab3dE9xQ", &pin_b)
        .unwrap_err()
        .publisher_change()
        .expect("must return A -> B")
        .clone();
    let change_ac = pin_or_check_in(dir.path(), "Ab3dE9xQ", &pin_c)
        .unwrap_err()
        .publisher_change()
        .expect("must return A -> C")
        .clone();
    accept_pin_in(dir.path(), &change_ac).unwrap();
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], pin_c);

    // Accepting the stale A -> B prompt must not replace C; it returns the current C -> B conflict.
    let err = accept_pin_in(dir.path(), &change_ab).unwrap_err();
    let current_conflict = err
        .publisher_change()
        .expect("must return current C -> B conflict")
        .clone();
    assert_eq!(current_conflict.project_id, "Ab3dE9xQ");
    assert_eq!(current_conflict.pinned, pin_c);
    assert_eq!(current_conflict.incoming, pin_b);
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], pin_c);

    // Confirming the current C -> B conflict replaces C with B, and re-accepting is idempotent.
    accept_pin_in(dir.path(), &current_conflict).unwrap();
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], pin_b);
    accept_pin_in(dir.path(), &change_ab).unwrap();
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], pin_b);

    // Durable replacement leaves no staging files behind.
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|name| name.starts_with(".txn-")));
}

#[test]
fn a_matching_pin_is_written_again_so_success_means_durable() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("nested/data/dir");
    let path = data_dir.join(PINS_FILE);
    let pin_a = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let pin_b = PublisherPin {
        owner_id: None,
        repository_id: 2002,
        ..pin_a.clone()
    };

    // Fail the flush right after the rename: the pin file becomes visible on disk, the call
    // reports an error, and the next matching install writes and flushes it cleanly.
    crate::profile_txn::durable::fail_next_replace_flush(&path);
    assert!(matches!(
        pin_or_check_in(&data_dir, "Ab3dE9xQ", &pin_a),
        Err(Error::PinStore(_))
    ));
    assert_eq!(
        load_pins_in(&data_dir).unwrap().get("Ab3dE9xQ"),
        Some(&pin_a)
    );
    assert_eq!(
        pin_or_check_in(&data_dir, "Ab3dE9xQ", &pin_a).unwrap(),
        PinStatus::Matched
    );

    // Same for a confirmed publisher change whose flush fails after the rename.
    let change = pin_or_check_in(&data_dir, "Ab3dE9xQ", &pin_b)
        .unwrap_err()
        .publisher_change()
        .expect("must return A -> B")
        .clone();
    crate::profile_txn::durable::fail_next_replace_flush(&path);
    assert!(matches!(
        accept_pin_in(&data_dir, &change),
        Err(Error::PinStore(_))
    ));
    assert_eq!(
        load_pins_in(&data_dir).unwrap().get("Ab3dE9xQ"),
        Some(&pin_b)
    );
    accept_pin_in(&data_dir, &change).unwrap();
    assert_eq!(
        load_pins_in(&data_dir).unwrap().get("Ab3dE9xQ"),
        Some(&pin_b)
    );
}

#[test]
fn publisher_change_text_includes_both_repository_ids_when_names_match() {
    let old = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let recreated = PublisherPin {
        owner_id: None,
        repository_id: 2002,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    let message = Error::PublisherChanged(Box::new(PublisherChange {
        project_id: "Ab3dE9xQ".to_owned(),
        pinned: old,
        incoming: recreated,
    }))
    .to_string();
    assert!(message.contains("alice/mod-a"), "{message}");
    assert!(message.contains("1001"), "{message}");
    assert!(message.contains("2002"), "{message}");
}

fn verified_from(owner_id: u64, repository_id: u64, repository_name: &str) -> VerifiedRelease {
    let mut attested = sample_attested(repository_id, repository_name, SUNRISE_WORKFLOW_V1);
    attested.owner_id = owner_id;
    check_release_policy(
        attested,
        &ExpectedRelease {
            repository_id,
            workflow: SUNRISE_WORKFLOW_V1,
        },
    )
    .unwrap()
}

#[test]
fn a_move_within_one_owner_keeps_the_pin_and_another_owner_asks() {
    let dir = tempfile::tempdir().unwrap();
    let first = verified_from(19_542_195, 1001, "arismoko/Jade-Sunrise").pin();
    pin_or_check_in(dir.path(), "Ab3dE9xQ", &first).unwrap();

    // Another repository of the same owner: no prompt, and the pin follows the move.
    let moved = verified_from(19_542_195, 2002, "arismoko/sunrise-ports").pin();
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved).unwrap(),
        PinStatus::Matched
    );
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], moved);

    // Another owner, even in a repository the project used before, asks first.
    let other_owner = verified_from(77, 1001, "someone/Jade-Sunrise").pin();
    let err = pin_or_check_in(dir.path(), "Ab3dE9xQ", &other_owner).unwrap_err();
    let change = err
        .publisher_change()
        .expect("must return PublisherChanged");
    assert!(change.owner_changed());
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], moved);

    // The same owner on another workflow asks too.
    let other_workflow = PublisherPin {
        workflow_path: "sunrise-org/sunrise-mods/.github/workflows/other.yml".to_owned(),
        ..moved.clone()
    };
    let err = check_pin_in(dir.path(), "Ab3dE9xQ", &other_workflow).unwrap_err();
    assert!(err.publisher_change().unwrap().workflow_changed());
}

#[test]
fn a_pin_without_an_owner_gains_it_from_its_own_repository_and_asks_once_after_a_move() {
    let dir = tempfile::tempdir().unwrap();
    // A pin as launchers before owner IDs wrote it.
    fs::write(
        dir.path().join(PINS_FILE),
        format!(
            r#"{{"schema":1,"projects":{{"Ab3dE9xQ":{{"repository_id":1001,"repository_name":"arismoko/Jade-Sunrise","workflow_path":"{RELEASE_WORKFLOW_PATH}"}}}}}}"#
        ),
    )
    .unwrap();
    assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"].owner_id, None);

    // The next release verified from the pinned repository fills the owner in.
    let same_repo = verified_from(19_542_195, 1001, "arismoko/Jade-Sunrise").pin();
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &same_repo).unwrap(),
        PinStatus::Matched
    );
    assert_eq!(
        load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"].owner_id,
        Some(19_542_195)
    );
    let moved = verified_from(19_542_195, 2002, "arismoko/sunrise-ports").pin();
    assert_eq!(
        pin_or_check_in(dir.path(), "Zz91kLmN", &same_repo).unwrap(),
        PinStatus::FirstInstall
    );
    assert_eq!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved).unwrap(),
        PinStatus::Matched
    );

    // An old pin that never saw its owner asks once when the project has already moved,
    // and once accepted remembers the owner.
    let old = PublisherPin {
        owner_id: None,
        repository_id: 3003,
        repository_name: "arismoko/Earshot-Sunrise".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };
    pin_or_check_in(dir.path(), "Ear5h0tX", &old).unwrap();
    let moved = verified_from(19_542_195, 2002, "arismoko/sunrise-ports").pin();
    let err = pin_or_check_in(dir.path(), "Ear5h0tX", &moved).unwrap_err();
    let change = err.publisher_change().unwrap().clone();
    assert!(change.repository_changed());
    // The old pin says nothing of its owner, so this is not an owner change.
    assert!(!change.owner_changed());
    accept_pin_in(dir.path(), &change).unwrap();
    let next = verified_from(19_542_195, 4004, "arismoko/elsewhere").pin();
    assert_eq!(
        pin_or_check_in(dir.path(), "Ear5h0tX", &next).unwrap(),
        PinStatus::Matched
    );
}

#[test]
fn corrupt_or_unsupported_pin_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let pin = PublisherPin {
        owner_id: None,
        repository_id: 1001,
        repository_name: "alice/mod-a".to_owned(),
        workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
    };

    fs::write(dir.path().join(PINS_FILE), b"{corrupt").unwrap();
    assert!(matches!(
        check_pin_in(dir.path(), "Ab3dE9xQ", &pin),
        Err(Error::PinStore(_))
    ));
    assert!(matches!(
        pin_or_check_in(dir.path(), "Ab3dE9xQ", &pin),
        Err(Error::PinStore(_))
    ));

    fs::write(
        dir.path().join(PINS_FILE),
        br#"{"schema": 99, "projects": {}}"#,
    )
    .unwrap();
    assert!(matches!(load_pins_in(dir.path()), Err(Error::PinStore(_))));
}
