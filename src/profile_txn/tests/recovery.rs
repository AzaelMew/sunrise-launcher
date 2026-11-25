//! Recovering after a kill: a file publication, another process's view of a generation, and retirement.

use super::*;

#[test]
fn move_recovers_a_killed_file_publication_before_copying() {
    use crate::data_location::{self, Roots};
    let root = tempfile::tempdir().unwrap();
    let source = Roots {
        launcher: root.path().join("launcher"),
        game: root.path().join("game"),
        build: root.path().join("build"),
    };
    for dir in [&source.launcher, &source.build, &source.game] {
        fs::create_dir(dir).unwrap();
    }
    initialized(&source.game, true);
    // This is a first file plan, with no previous committed Active pointer.
    fs::remove_file(source.game.join(POINTER)).unwrap();
    let mut actor = pause_actor(&source.game, "install:file-renamed");
    assert_eq!(fs::read(source.game.join(PACK)).unwrap(), package("2.0.0"));
    assert!(source.game.join(RECORD).is_file());
    actor.0.kill().unwrap();
    assert!(!actor.0.wait().unwrap().success());

    let mut reached_copy = false;
    let moved = data_location::migrate(
        &source,
        &root.path().join("destination"),
        &root.path().join("location.json"),
        |progress| {
            if progress == "Copying and verifying launcher data" {
                reached_copy = true;
                assert_eq!(fs::read(source.game.join(PACK)).unwrap(), package("1.0.0"));
                assert!(!source.game.join(RECORD).exists());
            }
        },
    )
    .unwrap();
    assert!(reached_copy);
    assert_eq!(fs::read(moved.game.join(PACK)).unwrap(), package("1.0.0"));
    assert_eq!(
        fs::read(moved.game.join(LIST)).unwrap(),
        b"mod.lantern:false\n"
    );
    assert!(!moved.game.join(RECORD).exists());
    assert!(!moved.game.join(POINTER).exists());
}

#[test]
fn another_process_sees_only_the_exact_old_or_committed_generation_after_kills() {
    let ordinary = [
        "file-before",
        "file-renamed",
        "file-durable",
        "pointer-before",
        "pointer-renamed",
        "pointer-durable",
        "marker-before",
        "marker-written",
        "marker-durable",
    ];
    for (intent, boundary) in ["install", "switch"]
        .into_iter()
        .flat_map(|intent| ordinary.map(|at| (intent, at)))
        .chain([
            ("switch", "directory-before"),
            ("switch", "directory-renamed"),
        ])
    {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let switching = intent == "switch";
        initialized(home, !switching);
        let actor = pause_actor(home, &format!("{intent}:{boundary}"));
        // The observer is already running and tries admission on both sides of the kill.
        assert!(Admission::open(home, None).is_err(), "{intent}:{boundary}");
        let pointer_replaced = matches!(
            boundary,
            "pointer-renamed"
                | "pointer-durable"
                | "marker-before"
                | "marker-written"
                | "marker-durable"
        );
        assert_eq!(
            fs::read(home.join(POINTER)).unwrap(),
            if pointer_replaced {
                if switching {
                    SWITCHED
                } else {
                    INSTALLED
                }
            } else {
                OLD
            }
        );
        let expected_package = if matches!(
            boundary,
            "directory-before" | "directory-renamed" | "file-before"
        ) {
            (!switching).then(|| package("1.0.0"))
        } else {
            Some(package("2.0.0"))
        };
        assert_eq!(fs::read(home.join(PACK)).ok(), expected_package);
        drop(actor);
        if switching && boundary == "pointer-renamed" {
            for undo in ["undo-pointer", "undo-list"] {
                let actor = pause_actor(home, undo);
                assert!(Admission::open(home, None).is_err());
                assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
                assert_eq!(fs::read(home.join(PACK)).unwrap(), package("2.0.0"));
                drop(actor);
            }
        }
        drop(Admission::open(home, None).unwrap());
        // A process kill leaves a whole cached marker visible; the next admission flushes it.
        // A torn marker (e.g. interrupted write) is exercised separately in the slot test.
        pair(
            home,
            switching,
            matches!(boundary, "marker-written" | "marker-durable"),
        );
        assert!(!home.join(RECORD).exists());
    }
}

#[test]
fn a_killed_retirement_reclaims_its_exact_tree_but_never_a_loose_one() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let unrelated = home.join("mod-browser/.install-txn-unowned");
    fs::create_dir(&unrelated).unwrap();
    fs::write(unrelated.join("keep"), b"not part of the journal").unwrap();
    drop(pause_actor(home, "install:marker-durable"));
    let stage = read(home)
        .unwrap()
        .unwrap()
        .operation()
        .unwrap()
        .stage
        .path(home);
    let cleanup = pause_actor(home, "retired");
    assert!(stage.is_dir());
    assert!(home.join(RETIRED).exists());
    assert!(Admission::open(home, None).is_err());
    drop(cleanup);
    drop(Admission::open(home, None).unwrap());
    assert!(!stage.exists());
    assert!(!home.join(RETIRED).exists());
    assert_eq!(
        fs::read(unrelated.join("keep")).unwrap(),
        b"not part of the journal"
    );
    pair(home, false, true);
}

#[test]
fn retirement_retries_a_failed_post_rename_barrier_before_erasing_preimages() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut owner = Admission::open(home, None).unwrap();
    let sealed = seal(&mut owner, false);
    let record = &sealed.0.record;
    let stage = record.operation().unwrap().stage.path(home);
    let retired = home.join(RETIRED);
    record.publish_at(home, || Ok(()), |_, _| Ok(())).unwrap();
    durable::fail_next_retirement_barrier(&retired);
    assert!(record
        .complete(home, |_| {}, || {})
        .unwrap_err()
        .contains("retirement barrier failure"));
    assert!(retired.is_file() && !home.join(RECORD).exists());
    assert_eq!(fs::read(stage.join("pointer-old")).unwrap(), OLD);
    let refs = references(home).unwrap();
    assert_eq!(refs.before.as_deref(), Some(OLD));
    assert_eq!(refs.after.as_deref(), Some(INSTALLED));
    drop(sealed);
    drop(owner);

    durable::fail_next_retirement_barrier(&retired);
    drop(Admission::open(home, None).unwrap());
    assert!(
        retired.is_file() && stage.is_dir(),
        "a retry failure must keep both sources of proof"
    );
    assert_eq!(fs::read(stage.join("pointer-old")).unwrap(), OLD);
    drop(Admission::open(home, None).unwrap());
    assert!(!retired.exists() && !stage.exists());
    pair(home, false, true);
}
