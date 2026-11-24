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
fn sdk_slice_copies_keep_their_modified_times() {
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let cached = Package::at(&dir.path().join("cache"));
    fs::create_dir_all(cached.runtime.join("lib")).unwrap();
    let file = cached.runtime.join("lib/input");
    fs::write(&file, b"slice").unwrap();
    let then = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(then)
        .unwrap();
    let private = stage.package(&cached).unwrap();
    let copied = fs::metadata(private.runtime.join("lib/input")).unwrap();
    assert_eq!(copied.modified().unwrap(), then);
}

#[test]
fn registry_library_aliases_keep_their_exact_names_and_bytes_inside_private_roots() {
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let root = stage.path();
    let package = Package::at(&root.join("package"));
    let rust = crate::rust_toolchain::Toolchain {
        dir: dir.path().join("rust"),
        host: crate::rust_toolchain::hosts().first().unwrap(),
    };
    let work = root.join("work");
    let target = work.join("prebuilt-build/registry-fixture/target");
    let release = target.join(rust.host.target).join("release");
    let binary = work
        .join("prebuilt-package/bin")
        .join(crate::backend::CLIENT);
    let patch = release.join("libsunrise_patch.so");
    let mod_output = release.join("libgreeter.so");
    let output = target.join("deps/libfrom_target.so.2");
    let dependency = root.join("cargo/registry/src/thing/libfrom_cargo.so.1");
    let outside = root.join("tmp/libfrom_cargo.so.1");
    for file in [&binary, &patch, &mod_output, &output, &dependency, &outside] {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, b"compiled bytes").unwrap();
    }
    let rust_version = crate::rust_toolchain::VERSION;
    let make_record = |file: &Path, kind, id: &str, version: &str| tool::LibraryRecord {
        file: file.file_name().unwrap().to_str().unwrap().into(),
        sha256: sha256_file(file).unwrap(),
        kind,
        mod_id: id.into(),
        identity: format!("fixture-build {id} {version} {rust_version} unwind"),
    };
    let mut built = tool::Built {
        route: tool::CheckRoute::Prebuilt,
        binary,
        libraries: vec![
            mod_output.clone(),
            patch.clone(),
            output,
            dependency.clone(),
        ],
        records: vec![
            make_record(&mod_output, tool::LibraryKind::Mod, "greeter", "1.0.0"),
            make_record(&patch, tool::LibraryKind::Patch, "sunrise_patch", "0"),
        ],
        mods: vec!["greeter".into()],
        internals: Default::default(),
    };
    let manifest = Manifest::parse("id = 'greeter'\nname = 'Greeter'\nversion = '1.0.0'\napi = '0.2'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nsunrise = 'a123456789012345678901234567890123456789'\n").unwrap();
    let verify = |built: &tool::Built| {
        stage.verify_outputs(
            built,
            &package,
            &rust,
            Some(tool::CheckRoute::Prebuilt),
            "fixture-build",
            [&manifest].into_iter(),
        )
    };
    #[cfg(unix)]
    {
        let alias = dependency.with_extension("");
        std::os::unix::fs::symlink(dependency.file_name().unwrap(), &alias).unwrap();
        built.libraries.push(alias);
    }
    let accepted = verify(&built).unwrap();
    let installed = dir.path().join("private-install");
    fs::create_dir(&installed).unwrap();
    for library in &accepted {
        library.copy_into(&installed).unwrap();
    }
    assert_eq!(
        fs::read(installed.join("libfrom_cargo.so.1")).unwrap(),
        b"compiled bytes"
    );
    assert_eq!(
        fs::read(installed.join("libfrom_target.so.2")).unwrap(),
        b"compiled bytes"
    );
    #[cfg(unix)]
    assert!(!fs::symlink_metadata(installed.join("libfrom_cargo.so"))
        .unwrap()
        .file_type()
        .is_symlink());
    fs::write(&dependency, b"substituted after verification").unwrap();
    let library = accepted
        .iter()
        .find(|row| row.name == "libfrom_cargo.so.1")
        .unwrap();
    assert!(library.copy_into(&installed).is_err());
    built.libraries[3] = outside.clone();
    assert!(
        verify(&built).is_err(),
        "neither the rest of the private stage nor outside is a Cargo root"
    );
    #[cfg(unix)]
    {
        let escaping = dependency.with_file_name("escaping.so.1");
        std::os::unix::fs::symlink(&outside, &escaping).unwrap();
        built.libraries[3] = escaping;
        assert!(
            verify(&built).is_err(),
            "a Cargo-home alias cannot escape its private roots"
        );
    }
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
