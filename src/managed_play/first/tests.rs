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
            let cancellation = native_mods::Cancellation::default();
            let image = attempt
                .exact_image(native_mods::Profile::Optimized, &cancellation, |_| {}, None)
                .unwrap();
            let active = attempt.commit(&image, &cancellation).unwrap();
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

#[test]
fn game_replacements_repair_only_owned_row_fields_and_advance_the_same_base() {
    use serde_json::{json, Value};
    use sunrise_launcher_smod::{index, manifest, mod_list};
    const CASE: &str = "managed_play::first::tests::game_replacements_repair_only_owned_row_fields_and_advance_the_same_base";
    if std::env::var_os("PLAY_ADOPTION_FIXTURE").is_none() {
        let temp = tempfile::tempdir().unwrap();
        let root = backend::plain(temp.path().canonicalize().unwrap());
        assert!(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CASE, "--nocapture"])
            .env("PLAY_ADOPTION_FIXTURE", "1")
            .env("SUNRISE_HOME", root.join("game"))
            .env("SUNRISE_LAUNCHER_HOME", root.join("launcher"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .status()
            .unwrap()
            .success());
        return;
    }
    let home = backend::game_home();
    let folder = backend::version_dir(BUILD);
    let resources = backend::resources(BUILD);
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join("sunrise-resources.json"), b"{}").unwrap();
    fs::write(folder.join(".installed"), []).unwrap();
    fs::write(folder.join(backend::CLIENT), "unchanged client").unwrap();
    fs::create_dir_all(home.join(mod_list::DIR)).unwrap();
    let relative = format!("{}/lantern.smod", mod_list::DIR);
    let package = home.join(&relative);
    let write_package = |version| {
        let manifest = format!("id='lantern'\nname='Lantern'\nversion='{version}'\napi='{}'\ncapabilities=['registries']\n", manifest::API);
        fs::write(
            &package,
            manifest::Package::write_files(&manifest, &[]).unwrap(),
        )
        .unwrap();
    };
    write_package("1.0.0");
    let row = json!({
        "source": "sunrise", "project": "lantern", "slug": "lantern", "title": "Lantern",
        "kind": "mod", "version": "1.0.0", "number": "1.0.0", "file": relative,
        "checksum": format!("SHA-256:{}", index::sha256_file(&package).unwrap()),
        "page": "registry page", "future-field": {"keep": [1, 2]}
    });
    let opaque =
        json!({"source": "future", "file": "resourcepacks/other.zip", "opaque": [null, {}]});
    let path = home.join(crate::browser::installed::MANIFEST);
    let initial = json!({"entries": [opaque, row], "future-top": [false]});
    fs::write(&path, serde_json::to_vec(&initial).unwrap()).unwrap();
    let cancellation = native_mods::Cancellation::default();
    let mut profile = Profile::open(&home, None).unwrap();
    let first = profile
        .begin_first(|| FirstBuild::installed(BUILD), |_| {})
        .unwrap()
        .commit(BUILD, &cancellation)
        .unwrap();
    let mut previous = first;
    let mut expected = initial.clone();
    for version in ["1.1.0", "1.2.0"] {
        write_package(version);
        assert!(profile
            .verified_handoff::<()>(&previous, &cancellation, |_| panic!("stale game launched"))
            .is_err());
        let current = profile
            .refresh(native_mods::Profile::Optimized, &cancellation, |_| {}, None)
            .unwrap()
            .unwrap();
        assert_eq!(current.profile, previous.profile);
        assert_eq!(current.generation, previous.generation + 1);
        assert_eq!(current.base, previous.base);
        assert_eq!(current.resources, previous.resources);
        assert_eq!(profile.active().unwrap(), Some(current.clone()));
        assert_eq!(inputs(&home, None).unwrap(), current.inputs);
        profile
            .verified_handoff(&current, &cancellation, |_| Ok(()))
            .unwrap();
        expected["entries"][1]["version"] = version.into();
        expected["entries"][1]["number"] = version.into();
        expected["entries"][1]["checksum"] =
            format!("SHA-256:{}", index::sha256_file(&package).unwrap()).into();
        let actual: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, expected);
        previous = current;
    }
    write_package("1.3.0");
    // An opaque row naming a different hard link still claims the same exact file.
    let alias = home.join("same-package.smod");
    fs::hard_link(&package, alias).unwrap();
    expected["entries"][0]["file"] = "same-package.smod".into();
    let before = serde_json::to_vec(&expected).unwrap();
    fs::write(&path, &before).unwrap();
    assert!(profile
        .refresh(native_mods::Profile::Optimized, &cancellation, |_| {}, None)
        .is_err());
    assert_eq!(profile.active().unwrap(), Some(previous));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn first_play_preparer_subcommands_keep_admission_and_never_trust_an_unrecorded_cache() {
    use std::os::unix::fs::PermissionsExt;

    const CASE: &str = "managed_play::first::tests::first_play_preparer_subcommands_keep_admission_and_never_trust_an_unrecorded_cache";
    let actor = |role| {
        let mut actor = Command::new(std::env::current_exe().unwrap());
        actor
            .args(["--exact", CASE, "--nocapture"])
            .env("PREPARE_CASE_ROLE", role);
        actor
    };
    let Ok(role) = std::env::var("PREPARE_CASE_ROLE") else {
        for pass in ["needs", "extras", "data-file"] {
            let temp = tempfile::tempdir().unwrap();
            let root = backend::plain(temp.path().canonicalize().unwrap());
            assert!(actor("scenario")
                .env("PREPARE_CASE_PASS", pass)
                .env("SUNRISE_HOME", root.join("game"))
                .env("SUNRISE_LAUNCHER_HOME", root.join("launcher"))
                .env("XDG_CONFIG_HOME", root.join("config"))
                .status()
                .unwrap()
                .success());
        }
        return;
    };
    let pass = std::env::var("PREPARE_CASE_PASS").unwrap();
    let home = backend::game_home();
    let resources = backend::resources(BUILD);
    let root = home.parent().unwrap();
    let prepared = || {
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("sunrise-resources.json"), "{}").unwrap();
    };
    let admit = || Profile::open(&home, None);
    if role == "attempt" {
        let mut profile = admit().unwrap();
        let attempt = profile
            .begin_first(|| FirstBuild::installed(BUILD), |_| {})
            .unwrap();
        attempt.prepare(&|_| {}).unwrap();
        attempt
            .commit(BUILD, &native_mods::Cancellation::default())
            .unwrap();
        panic!("the scenario should kill Play while its preparer is running");
    }
    assert_eq!(role, "scenario");
    fs::create_dir_all(&home).unwrap();
    let cache = backend::version_dir(BUILD);
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(".installed"), []).unwrap();
    fs::write(cache.join(backend::CLIENT), b"local fixture client").unwrap();
    let binary = cache.join(crate::preparer::BINARY);
    fs::write(
        &binary,
        r#"#!/bin/sh
set -eu
cmd=$1
shift
out=
while [ "$#" -ne 0 ]; do
  if [ "$1" = --output ]; then out=$2; shift; fi
  shift
done
root=${SUNRISE_HOME%/*}
# Write the accelerator before announcing the interruption, while it still has no evidence.
if [ "$cmd" = data-file ]; then printf 'fixture data' > "$out/sunrise-data.bin"; fi
if [ "$cmd" = "$PREPARE_CASE_PASS" ] && [ ! -f "$root/finish" ]; then
  : > "$root/entered"
  n=0
  while [ ! -f "$root/finish" ] && [ "$n" -lt 1500 ]; do sleep 0.01; n=$((n+1)); done
  [ -f "$root/finish" ] || exit 1
fi
case "$cmd" in
  needs) printf '%s\n' '{"format":1,"server_jar":false,"jdk":false}';;
  missing-extras) printf '%s\n' '{"format":1,"missing":[{"kind":"sounds","what":"sounds"}]}';;
  extras) printf 'extra' > "$out/extra"; printf '%s\n' '{"format":1,"added":["sounds"]}';;
  data-file) printf '%s\n' '{"format":1,"written":true}';;
  *) exit 2;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    if pass != "needs" {
        prepared();
    }
    let mut attempt = Running(actor("attempt").spawn().unwrap());
    await_fixture(|| root.join("entered").is_file());
    attempt.0.kill().unwrap();
    assert!(!attempt.0.wait().unwrap().success());
    assert!(
        admit().is_err(),
        "the live {pass} child lost admission when Play died"
    );
    assert!(profile_txn::references(&home).unwrap().pointer.is_none());
    fs::write(root.join("finish"), []).unwrap();
    let mut resumed = None;
    await_fixture(|| {
        resumed = admit().ok();
        resumed.is_some()
    });
    if pass == "needs" {
        // The fixture does not implement Mojang input resolution. Supply prepared inputs
        // after proving the real `needs` call survived and held off the next launcher.
        prepared();
    }
    let mut profile = resumed.unwrap();
    let attempt = profile
        .begin_first(|| panic!("the unstamped local selection changed"), |_| {})
        .unwrap();
    let active = attempt
        .commit(BUILD, &native_mods::Cancellation::default())
        .unwrap();
    assert_eq!(
        fs::read(active.resource_path().join("extra")).unwrap(),
        b"extra"
    );
    assert_eq!(
        fs::read(active.resource_path().join(crate::preparer::DATA_FILE)).unwrap(),
        b"fixture data"
    );
    assert_eq!(active.allow_accelerator(), pass != "data-file");
    active.verify_artifacts().unwrap();
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
