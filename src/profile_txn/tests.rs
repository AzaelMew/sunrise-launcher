use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use sunrise_launcher_smod::{manifest, Package};

const HOME: &str = "SUNRISE_PROFILE_JOURNAL_TEST_HOME";
const ACTION: &str = "SUNRISE_PROFILE_JOURNAL_TEST_ACTION";
const PACK: &str = "sunrise-mods/lantern.smod";
const LIST: &str = "sunrise-mods.txt";
const KEEP: &str = "sunrise-native-mods/unchanged.smod";
const OLD: &[u8] = b"{\"base\":\"nightly-old\",\"generation\":1,\"receipts\":[\"old\"]}\n";
const SWITCHED: &[u8] =
    b"{\"base\":\"nightly-new\",\"generation\":2,\"receipts\":[\"old\",\"new\"]}\n";
const INSTALLED: &[u8] = b"{\"base\":\"nightly-old\",\"generation\":2,\"receipts\":[\"old\"]}\n";

fn package(version: &str) -> Vec<u8> {
    Package::write_files(&format!("id='lantern'\nname='Lantern'\nversion='{version}'\napi='{}'\ncapabilities=['registries']\n", manifest::API), &[]).unwrap()
}

fn initialized(home: &Path, installed: bool) {
    fs::create_dir(home.join("mod-browser")).unwrap();
    fs::create_dir(home.join("sunrise-native-mods")).unwrap();
    fs::write(home.join(POINTER), OLD).unwrap();
    fs::write(home.join(LIST), "mod.lantern:false\n").unwrap();
    fs::write(home.join(KEEP), package("1.0.0")).unwrap();
    if installed {
        fs::create_dir(home.join("sunrise-mods")).unwrap();
        fs::write(home.join(PACK), package("1.0.0")).unwrap();
    }
}

fn seal(owner: &mut Admission, switching: bool) -> Sealed<'_> {
    let intent = if switching {
        Intent::Switch
    } else {
        Intent::Install
    };
    let pointer = if switching { SWITCHED } else { INSTALLED };
    let staging = owner.reserve(intent).unwrap();
    let package_path = staging.path().join("candidate.smod");
    let list = staging.path().join("candidate-list");
    fs::write(&package_path, package("2.0.0")).unwrap();
    fs::write(&list, "mod.lantern:true\n").unwrap();
    staging
        .seal(
            pointer.to_vec(),
            &[(PACK.into(), Some(package_path)), (LIST.into(), Some(list))],
        )
        .unwrap()
}

fn pair(home: &Path, switching: bool, committed: bool) {
    assert_eq!(fs::read(home.join(KEEP)).unwrap(), package("1.0.0"));
    if committed {
        assert_eq!(
            fs::read(home.join(POINTER)).unwrap(),
            if switching { SWITCHED } else { INSTALLED }
        );
        assert_eq!(fs::read(home.join(PACK)).unwrap(), package("2.0.0"));
        assert_eq!(fs::read(home.join(LIST)).unwrap(), b"mod.lantern:true\n");
    } else {
        assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
        assert_eq!(fs::read(home.join(LIST)).unwrap(), b"mod.lantern:false\n");
        if switching {
            assert!(!home.join("sunrise-mods").exists());
        } else {
            assert_eq!(fs::read(home.join(PACK)).unwrap(), package("1.0.0"));
        }
    }
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
}

#[test]
fn no_space_at_marker_reverses_pointer_and_files_using_existing_allocations() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let mut admission = Admission::open(home, None).unwrap();
    let sealed = seal(&mut admission, false);
    let disk = read(home).unwrap().unwrap();
    let stage = disk.stage(home);
    assert_eq!(fs::read(stage.join("pointer-old")).unwrap(), OLD);
    let progress_file = stage.join("progress");
    let bytes_reserved = fs::metadata(&progress_file).unwrap().len();
    let error = disk
        .publish_at(
            home,
            || Ok(()),
            |_, boundary| {
                if boundary == Boundary::BeforeMarker {
                    pair(home, false, true);
                    assert_eq!(fs::metadata(&progress_file).unwrap().len(), bytes_reserved);
                    return Err(io::Error::from(io::ErrorKind::StorageFull).to_string());
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(
        error,
        io::Error::from(io::ErrorKind::StorageFull).to_string()
    );
    drop(sealed);
    drop(admission);
    drop(Admission::open(home, None).unwrap());
    pair(home, false, false);
    assert!(!home.join(RECORD).exists());
    assert!(!stage.exists());
}

#[test]
fn unowned_bytes_and_unknown_records_are_never_replaced_or_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, false);
    fs::write(home.join(RECORD), b"{\"version\":1}").unwrap();
    assert!(Admission::open(home, None).is_err());
    assert_eq!(fs::read(home.join(RECORD)).unwrap(), b"{\"version\":1}");
    fs::remove_file(home.join(RECORD)).unwrap();
    let mut admission = Admission::open(home, None).unwrap();
    let sealed = seal(&mut admission, true);
    fs::create_dir(home.join("sunrise-mods")).unwrap();
    fs::write(home.join(PACK), package("9.0.0")).unwrap();
    assert!(sealed
        .publish::<()>(|| panic!("occupied target must be rejected before cutoff"))
        .is_err());
    assert_eq!(fs::read(home.join(PACK)).unwrap(), package("9.0.0"));
    assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
    let staging = admission.reserve(Intent::Install).unwrap();
    let outside = home.join("outside");
    fs::write(&outside, b"keep").unwrap();
    assert!(staging
        .seal(
            INSTALLED.to_vec(),
            &[(("../outside").into(), Some(outside.clone()))]
        )
        .is_err());
    assert_eq!(fs::read(outside).unwrap(), b"keep");
}

struct Actor(Child);
impl Drop for Actor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pause_actor(home: &Path, action: &str, prepare: impl FnOnce(&mut Command)) -> Actor {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "profile_txn::tests::journal_actor",
            "--nocapture",
        ])
        .env(HOME, home)
        .env(ACTION, action)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    prepare(&mut command);
    let mut child = Actor(command.spawn().unwrap());
    let stdout = child.0.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line == "txn-paused" {
                let _ = send.send(());
            }
        }
    });
    recv.recv_timeout(Duration::from_secs(20))
        .expect("actor reached the journal boundary");
    child
}

fn pause_here() {
    println!("txn-paused");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).unwrap();
    assert_eq!(
        input, "release\n",
        "parent should stop the child or release the inherited lease"
    );
}

#[test]
fn journal_actor() {
    let Some(home) = std::env::var_os(HOME).map(PathBuf::from) else {
        return;
    };
    let action = std::env::var(ACTION).unwrap();
    if action == "hold" {
        return pause_here();
    }
    let mut owner = Admission::claim(&home, None).unwrap();
    match action.as_str() {
        "undo-pointer" | "undo-list" => {
            let disk = read(&home).unwrap().unwrap();
            let last = disk.operation.ready.as_ref().unwrap().steps() - 1;
            disk.complete(&home, |i| {
                if (action == "undo-pointer" && i == last)
                    || (action == "undo-list" && i == last - 1)
                {
                    pause_here();
                }
            })
            .unwrap();
        }
        _ => {
            let (intent, stage) = action.split_once(':').unwrap();
            let switching = intent == "switch";
            let sealed = seal(&mut owner, switching);
            let record = &sealed.0.record;
            let steps = record.operation.ready.as_ref().unwrap().steps();
            let index = usize::from(switching);
            let stop = match stage {
                "directory-before" => (0, Boundary::BeforeRename),
                "directory-renamed" => (0, Boundary::Renamed),
                "file-before" => (index, Boundary::BeforeRename),
                "file-renamed" => (index, Boundary::Renamed),
                "file-durable" => (index, Boundary::Durable),
                "pointer-before" => (steps - 1, Boundary::BeforeRename),
                "pointer-renamed" => (steps - 1, Boundary::Renamed),
                "pointer-durable" => (steps - 1, Boundary::Durable),
                "marker-before" => (steps, Boundary::BeforeMarker),
                "marker-written" => (steps, Boundary::WrittenMarker),
                "marker-durable" => (steps, Boundary::DurableMarker),
                _ => panic!("unknown boundary {stage}"),
            };
            record
                .publish_at(
                    &home,
                    || Ok(()),
                    |i, event| {
                        if (i, event) == stop {
                            pause_here();
                        }
                        Ok(())
                    },
                )
                .unwrap();
        }
    }
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
        let actor = pause_actor(home, &format!("{intent}:{boundary}"), |_| {});
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
                let actor = pause_actor(home, undo, |_| {});
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
#[cfg(unix)]
fn a_child_inherits_the_profile_gate_and_blocks_an_already_open_launcher() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let owner = Admission::open(home, None).unwrap();
    let mut child = pause_actor(home, "hold", |cmd| owner.inherit(cmd).unwrap());
    drop(owner);
    assert!(Admission::open(home, None).is_err());
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert!(child.0.wait().unwrap().success());
    assert!(Admission::open(home, None).is_ok());
}
