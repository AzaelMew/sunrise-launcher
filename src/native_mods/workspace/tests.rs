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
fn private_sdk_slices_keep_each_file_s_modified_time() {
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let cached = Package::at(&dir.path().join("cache"));
    fs::create_dir_all(cached.api.join("deps")).unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    for file in [cached.api.join("top"), cached.api.join("deps/nested")] {
        fs::write(&file, b"slice").unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    // A read-only slice file keeps its time too.
    let top = cached.api.join("top");
    let mut readonly = fs::metadata(&top).unwrap().permissions();
    readonly.set_readonly(true);
    fs::set_permissions(&top, readonly).unwrap();
    let private = stage.package(&cached).unwrap();
    // The tool's build folder name hashes these times: the same every attempt.
    for file in [private.api.join("top"), private.api.join("deps/nested")] {
        assert_eq!(
            fs::metadata(&file).unwrap().modified().unwrap(),
            old,
            "{file:?}"
        );
    }
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
    let work = stage.work();
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
    let work = stage.work();
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

/// Since sunrise#5136 a library's identity also names its mod's files and, for each native mod it
/// links, the SHA-256 of the identity that mod's library was built with.
#[test]
fn a_library_names_the_identities_of_the_mods_it_links() {
    use sunrise_launcher_smod::index::library_identity::library_identity;
    let dir = tempfile::tempdir().unwrap();
    let stage = Workspace::new(dir.path()).unwrap();
    let package = Package::at(&stage.path().join("package"));
    let rust = crate::rust_toolchain::Toolchain {
        dir: dir.path().join("rust"),
        host: crate::rust_toolchain::hosts().first().unwrap(),
    };
    let work = stage.work();
    let bin = package.runtime.join("bin");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let binary = bin.join(crate::backend::CLIENT);
    fs::write(&binary, b"bytes").unwrap();
    let rust_version = crate::rust_toolchain::VERSION;
    // As the tool stamps them: `base` first, then `dependent`, which links it.
    let base = format!(
        "{} unwind",
        library_identity(
            ["fixture-build", "base", "1.0.0", rust_version],
            "s1+g1",
            []
        )
    );
    let dependent = |base: &str| {
        format!(
            "{} unwind",
            library_identity(
                ["fixture-build", "dependent", "2.0.0", rust_version],
                "s2",
                [("base", base)]
            )
        )
    };
    let mut built = tool::Built {
        route: tool::CheckRoute::PrebuiltApi,
        binary,
        libraries: Vec::new(),
        mods: vec!["base".into(), "dependent".into()],
        records: Vec::new(),
        internals: Default::default(),
    };
    for (id, identity) in [("base", base.clone()), ("dependent", dependent(&base))] {
        let lib = work.join(format!("{id}.dll"));
        fs::write(&lib, id.as_bytes()).unwrap();
        built.records.push(tool::LibraryRecord {
            file: format!("{id}.dll"),
            sha256: sha256_file(&lib).unwrap(),
            kind: tool::LibraryKind::Mod,
            mod_id: id.into(),
            identity,
        });
        built.libraries.push(lib);
    }
    let manifest = |id: &str, version: &str| {
        Manifest::parse(&format!(
            "id = '{id}'\nname = 'M'\nversion = '{version}'\n[native]\ngame = '26.3'\nsunrise = 'a123456789012345678901234567890123456789'\n"
        ))
        .unwrap()
    };
    let manifests = [manifest("base", "1.0.0"), manifest("dependent", "2.0.0")];
    let verify = |built: &tool::Built| {
        stage.verify_outputs(
            built,
            &package,
            &rust,
            Some(tool::CheckRoute::PrebuiltApi),
            "fixture-build",
            manifests.iter(),
        )
    };
    assert!(verify(&built).is_ok());
    let mut refused = |identity: String, why: &str| {
        let kept = std::mem::replace(&mut built.records[1].identity, identity);
        assert!(verify(&built).is_err(), "{why}");
        built.records[1].identity = kept;
    };
    refused(
        dependent(&base.replace("s1+g1", "s1b+g1")),
        "built against another build of base",
    );
    refused(
        dependent(&base).replace("base=", "absent="),
        "links a mod this build did not make",
    );
    refused(
        dependent(&base).replace(" base=", " base"),
        "a malformed dependency list",
    );
    refused(
        dependent(&base).replace(" unwind", " abort"),
        "another panic strategy",
    );
    refused(
        dependent(&base).replace("fixture-build", "other-build"),
        "another game build",
    );
    refused(
        dependent(&base).replace(" 2.0.0 ", " 2.0.1 "),
        "another mod version",
    );
    refused(
        format!(
            "{} unwind extra",
            dependent(&base).trim_end_matches(" unwind")
        ),
        "an eighth field",
    );
    // An SDK from before sunrise#5136 stamps the five-field identity.
    for (row, (id, version)) in built
        .records
        .iter_mut()
        .zip([("base", "1.0.0"), ("dependent", "2.0.0")])
    {
        row.identity = format!("fixture-build {id} {version} {rust_version} unwind");
    }
    assert!(verify(&built).is_ok());
}

#[test]
fn a_build_too_deep_for_windows_is_found_before_it_starts() {
    let home = |user: &str| {
        std::path::PathBuf::from(format!(
            r"C:\Users\{user}\AppData\Roaming\sunrise\launcher\native\.nb-a1B2c3"
        ))
    };
    // #396's player, eight letters: 265 characters in the old folders, 232 now.
    assert_eq!(too_deep(&home("abcdefgh")), None);
    assert_eq!(too_deep(&home(&"u".repeat(35))), None);
    assert_eq!(too_deep(&home(&"u".repeat(36))), Some(260));
}
