use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
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
    let staging = owner
        .reserve(intent, BTreeSet::from(["nightly-new".into()]))
        .unwrap();
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

const FIRST_POINTER: &[u8] = b"{\"base\":\"nightly-original\",\"generation\":1}\n";

fn first_binding() -> Bootstrap {
    Bootstrap::new(
        "nightly-original".into(),
        "preferred-first-image".into(),
        [7; 16],
        [3; 32],
        serde_json::json!({"archive":"sunrise-linux-aarch64.tar.gz","revision":"original"}),
    )
}

struct Actor(Child);
impl Drop for Actor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn actor_command(home: &Path, action: &str) -> Command {
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
    command
}

fn pause_actor(home: &Path, action: &str) -> Actor {
    let mut child = Actor(actor_command(home, action).spawn().unwrap());
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
    match action.as_str() {
        "check-exclusion" => {
            pause_here();
            assert!(Admission::open(&home, None)
                .err()
                .unwrap()
                .contains("profile is in use"));
            return;
        }
        "staged-worker" => {
            let stage = read(&home)
                .unwrap()
                .unwrap()
                .operation()
                .unwrap()
                .stage
                .path(&home);
            fs::write(stage.join("worker.smod"), package("2.0.0")).unwrap();
            fs::create_dir(stage.join("worker-ready")).unwrap();
            await_fixture(|| stage.join("finish-worker").is_file());
            return;
        }
        _ => {}
    }
    let mut owner = Admission::claim(&home, None).unwrap();
    match action.as_str() {
        "launcher-with-staged-worker" => {
            use crate::process::{leases::StagedChild, owned::Owned};
            let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
            let _worker = Owned::spawn_staged(
                actor_command(&home, "staged-worker"),
                stage.handoff(),
                StagedChild::Prepare,
            )
            .unwrap();
            await_fixture(|| stage.path().join("worker-ready").is_dir());
            pause_here();
        }
        "bootstrap:prepare" | "bootstrap:pointer-renamed" => {
            let staging = owner.resume_bootstrap().unwrap();
            if action.ends_with("prepare") {
                fs::write(staging.path().join("incomplete-worker-output"), b"private").unwrap();
                pause_here();
                return;
            }
            let sealed = staging.seal(FIRST_POINTER.to_vec(), &[]).unwrap();
            sealed
                .0
                .record
                .publish_at(
                    &home,
                    || Ok(()),
                    |_, boundary| {
                        if boundary == Boundary::Renamed {
                            pause_here();
                        }
                        Ok(())
                    },
                )
                .unwrap();
        }
        "undo-pointer" | "undo-list" | "retired" => {
            let disk = read(&home).unwrap().unwrap();
            let last = disk.operation().unwrap().ready.as_ref().unwrap().steps() - 1;
            disk.complete(
                &home,
                |i| {
                    if (action == "undo-pointer" && i == last)
                        || (action == "undo-list" && i == last - 1)
                    {
                        pause_here();
                    }
                },
                || {
                    if action == "retired" {
                        pause_here();
                    }
                },
            )
            .unwrap();
        }
        _ => {
            let (intent, stage) = action.split_once(':').unwrap();
            let switching = intent == "switch";
            let sealed = seal(&mut owner, switching);
            let record = &sealed.0.record;
            let steps = record.operation().unwrap().ready.as_ref().unwrap().steps();
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

fn await_fixture(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the staged actor"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

mod admission;
mod commit;
mod recovery;
mod refusals;
mod restore;
