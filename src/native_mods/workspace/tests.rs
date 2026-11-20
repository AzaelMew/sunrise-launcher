use super::*;

#[cfg(unix)]
#[test]
#[ignore = "daily"]
fn an_escaped_worker_keeps_only_its_old_writable_paths() {
    use std::{
        io::Write,
        os::unix::process::CommandExt,
        process::{Command, Stdio},
    };
    let dir = tempfile::tempdir().unwrap();
    let mut first = Workspace::new(dir.path()).unwrap();
    let paths = |root: &Path| {
        [
            root.join("cargo/writer"),
            root.join("target/writer"),
            root.join("tmp/writer"),
        ]
    };
    let first_paths = paths(first.path());
    let mut command = Command::new("/bin/sh");
    command.args(["-ec", r#"read done; printf old > "$CARGO_HOME/writer"; printf old > "$CARGO_TARGET_DIR/writer"; printf old > "$TMPDIR/writer""#]).stdin(Stdio::piped());
    first.isolate(&mut command).unwrap();
    // SAFETY: setsid is async-signal-safe and this child was not made a process group leader.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    first.allow_worker();
    let mut old = command.spawn().unwrap();
    drop(first);
    let retry = Workspace::new(dir.path()).unwrap();
    retry.isolate(&mut Command::new("/bin/sh")).unwrap();
    let retry_paths = paths(retry.path());
    for path in &retry_paths {
        fs::write(path, "new").unwrap();
    }
    old.stdin.take().unwrap().write_all(b"continue\n").unwrap();
    assert!(old.wait().unwrap().success());
    for (old, new) in first_paths.iter().zip(&retry_paths) {
        assert_ne!(old, new);
        assert_eq!(fs::read_to_string(old).unwrap(), "old");
        assert_eq!(fs::read_to_string(new).unwrap(), "new");
    }
}

#[test]
fn writable_sdk_slices_do_not_share_the_cached_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let cached = Package::at(&dir.path().join("cache"));
    fs::create_dir_all(&cached.runtime).unwrap();
    fs::write(cached.runtime.join("input"), b"original").unwrap();
    let private = stage.package(&cached).unwrap();
    fs::write(private.runtime.join("input"), b"compiler scratch").unwrap();
    assert_eq!(fs::read(cached.runtime.join("input")).unwrap(), b"original");
}

#[test]
fn an_sdk_can_only_supply_its_own_files_and_the_exact_requested_mods() {
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let package = Package::at(&stage.path().join("package"));
    let rust = crate::rust_toolchain::Toolchain {
        dir: dir.path().join("rust"),
        host: crate::rust_toolchain::hosts().first().unwrap(),
    };
    let work = stage.path().join("work");
    let bin = package.runtime.join("bin");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let binary = bin.join(crate::backend::CLIENT);
    let lib = work.join("greeter.dll");
    let outside = dir.path().join("outside.dll");
    for file in [&binary, &lib, &outside] {
        fs::write(file, b"bytes").unwrap();
    }
    let mut built = tool::Built {
        route: tool::CheckRoute::PrebuiltApi,
        binary: binary.clone(),
        libraries: vec![lib.clone()],
        mods: vec!["greeter".into()],
        records: vec![tool::LibraryRecord {
            file: "greeter.dll".into(),
            sha256: sha256_file(&lib).unwrap(),
            kind: tool::LibraryKind::Mod,
            mod_id: "greeter".into(),
            identity: format!(
                "fixture-build greeter 1.0.0 {} unwind",
                crate::rust_toolchain::VERSION
            ),
        }],
        internals: Default::default(),
    };
    let manifest = Manifest::parse(
        "id = 'greeter'\nname = 'Greeter'\nversion = '1.0.0'\napi = '0.2'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nsunrise = 'a123456789012345678901234567890123456789'\n",
    )
    .unwrap();
    let verify = |built: &tool::Built| {
        stage.verify_outputs(
            built,
            &package,
            &rust,
            Some(tool::CheckRoute::PrebuiltApi),
            "fixture-build",
            [&manifest].into_iter(),
        )
    };
    assert!(verify(&built).is_ok());
    built.route = tool::CheckRoute::Prebuilt;
    assert!(
        verify(&built).is_err(),
        "checked API route cannot publish an engine result"
    );
    built.route = tool::CheckRoute::PrebuiltApi;
    let fake_binary = work.join("another-executable");
    fs::write(&fake_binary, b"wrong executable").unwrap();
    built.binary = fake_binary;
    assert!(
        verify(&built).is_err(),
        "API result must use the SDK runtime executable"
    );
    built.binary = binary;
    fs::write(&lib, b"different output").unwrap();
    assert!(
        verify(&built).is_err(),
        "library record digest binds the copied output"
    );
    fs::write(&lib, b"bytes").unwrap();
    built.records[0].file = "another.dll".into();
    assert!(
        verify(&built).is_err(),
        "a library record names exactly one supplied path"
    );
    built.records[0].file = "greeter.dll".into();
    let identity = std::mem::replace(&mut built.records[0].identity, "wrong identity".into());
    assert!(
        verify(&built).is_err(),
        "records must name the exact build and mod version"
    );
    built.records[0].identity = identity;
    assert!(verify(&built).is_ok());
    let records = std::mem::take(&mut built.records);
    assert!(
        verify(&built).is_err(),
        "every requested mod needs its own record"
    );
    built.records = records;
    built.mods.push("unexpected".into());
    assert!(verify(&built).is_err());
    built.mods.pop();
    built.libraries.push(outside.clone());
    assert!(verify(&built).is_err());
    built.libraries[1] = lib;
    assert!(verify(&built).is_err());
    #[cfg(unix)]
    {
        let linked = work.join("linked.dll");
        std::os::unix::fs::symlink(outside, &linked).unwrap();
        built.libraries = vec![linked];
        assert!(verify(&built).is_err());
    }
}
