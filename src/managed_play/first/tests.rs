use super::*;
use std::{
    fs,
    process::{Child, Stdio},
    time::{Duration, Instant},
};

const NAME: &str =
    "managed_play::first::tests::interrupted_prepare_retains_the_same_build_and_profile_exclusion";
const BUILD: &str = "v0.1.7";

#[test]
fn interrupted_prepare_retains_the_same_build_and_profile_exclusion() {
    let actor = |role| {
        let mut actor = Command::new(std::env::current_exe().unwrap());
        actor
            .args(["--exact", NAME, "--nocapture"])
            .env("FIRST_BINDING_ACTOR", role);
        actor
    };
    let Ok(role) = std::env::var("FIRST_BINDING_ACTOR") else {
        let temp = tempfile::tempdir().unwrap();
        let root = backend::plain(temp.path().canonicalize().unwrap());
        assert!(actor("scenario")
            .env("SUNRISE_HOME", root.join("game"))
            .env("SUNRISE_LAUNCHER_HOME", root.join("launcher"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .status()
            .unwrap()
            .success());
        return;
    };
    let home = backend::game_home();
    let fixture = home.with_file_name("prepare-ready");
    let finish = home.with_file_name("finish-prepare");
    match role.as_str() {
        "prepare" => {
            fs::create_dir(&fixture).unwrap();
            await_fixture(|| finish.is_file());
            let resources = backend::resources(BUILD);
            fs::create_dir_all(&resources).unwrap();
            fs::write(resources.join("sunrise-resources.json"), b"{}").unwrap();
        }
        "interrupted-launcher" => {
            let mut profile = Profile::open(&home, None).unwrap();
            let first = profile
                .begin_first(|| FirstBuild::installed(BUILD), |_| {})
                .unwrap();
            first.run_prepare(actor("prepare"), &|_| {}).unwrap();
            panic!("test should have killed this launcher while the preparer was held");
        }
        "already-open-launcher" => {
            fs::create_dir(home.with_file_name("contender-ready")).unwrap();
            await_fixture(|| home.with_file_name("try-admission").is_file());
            assert!(Profile::open(&home, None).is_err());
        }
        "scenario" => {
            fs::create_dir_all(&home).unwrap();
            let cache = backend::version_dir(BUILD);
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join(".installed"), b"fixture installed archive").unwrap();
            fs::write(cache.join(backend::CLIENT), b"fixture client bytes").unwrap();
            let mut contender = Running(
                actor("already-open-launcher")
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            await_fixture(|| home.with_file_name("contender-ready").is_dir());
            let mut interrupted = Running(
                actor("interrupted-launcher")
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            await_fixture(|| fixture.is_dir());
            let frozen = profile_txn::references(&home).unwrap();
            assert!(frozen.pointer.is_none());
            assert!(frozen.targets.contains(BUILD));
            interrupted.0.kill().unwrap();
            assert!(!interrupted.0.wait().unwrap().success());
            fs::write(home.with_file_name("try-admission"), []).unwrap();
            assert!(contender.0.wait().unwrap().success());
            fs::write(&finish, []).unwrap();
            let mut opened = None;
            await_fixture(|| {
                opened = Profile::open(&home, None).ok();
                opened.is_some()
            });
            let mut profile = opened.unwrap();
            let attempt = profile
                .begin_first(|| panic!("first build resolved a second time"), |_| {})
                .unwrap();
            assert_eq!(attempt.base(), BUILD);
            assert_eq!(attempt.preferred_image(), BUILD);
            attempt
                .prepare(&|_| panic!("a complete preparation was launched again"))
                .unwrap();
            let active = attempt.commit(BUILD, || Ok(())).unwrap();
            assert_eq!(profile.active().unwrap(), Some(active.clone()));
            active.verify_artifacts().unwrap();
            assert_eq!(
                fs::read(active.resource_path().join("sunrise-resources.json")).unwrap(),
                b"{}"
            );
        }
        role => panic!("unexpected role: {role}"),
    }
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn await_fixture(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "first-Play test stage timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
