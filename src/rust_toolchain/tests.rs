use super::*;
use std::io::{Read, Write};

#[test]
fn a_comspec_naming_no_file_is_replaced_by_the_system_cmd() {
    let root = tempfile::tempdir().unwrap();
    let system32 = root.path().join("System32");
    fs::create_dir_all(&system32).unwrap();
    let root_os = root.path().as_os_str();
    let missing = root.path().join("gone.exe");
    // No cmd.exe under the system root: nothing to offer.
    assert_eq!(comspec_fix(Some(missing.as_os_str()), Some(root_os)), None);
    fs::write(system32.join("cmd.exe"), b"").unwrap();
    let expected = system32.join("cmd.exe");
    assert_eq!(
        comspec_fix(Some(missing.as_os_str()), Some(root_os)),
        Some(expected.clone())
    );
    assert_eq!(comspec_fix(None, Some(root_os)), Some(expected.clone()));
    assert_eq!(
        comspec_fix(Some("".as_ref()), Some(root_os)),
        Some(expected.clone())
    );
    // A ComSpec that names a file stays the player's.
    assert_eq!(comspec_fix(Some(expected.as_os_str()), Some(root_os)), None);
    assert_eq!(comspec_fix(None, None), None);

    // What a build's command gets, through `command_with`: the replacement is set on
    // Windows only, a good ComSpec is not touched, and the scrubbing still applies.
    let toolchain = Toolchain {
        dir: root.path().join("toolchain"),
        host: &HOSTS[0],
    };
    let comspec_of = |windows: bool, comspec: Option<&std::ffi::OsStr>| {
        let cmd = toolchain.command_with(Path::new("tool"), windows, comspec, Some(root_os));
        cmd.get_envs()
            .find(|(name, _)| *name == "ComSpec")
            .map(|(_, value)| value.map(std::ffi::OsStr::to_owned))
    };
    assert_eq!(
        comspec_of(true, Some(missing.as_os_str())),
        Some(Some(expected.clone().into_os_string()))
    );
    assert_eq!(
        comspec_of(true, None),
        Some(Some(expected.clone().into_os_string()))
    );
    assert_eq!(comspec_of(true, Some(expected.as_os_str())), None);
    assert_eq!(comspec_of(false, Some(missing.as_os_str())), None);
    let cmd = toolchain.command_with(Path::new("tool"), true, None, Some(root_os));
    assert!(cmd
        .get_envs()
        .any(|(name, value)| name == "RUSTFLAGS" && value.is_none()));
}

#[test]
fn archive_download_checks_pinned_length_as_well_as_hash() {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("archive");
    let mut oversized = vec![b'x'; 1 << 20];
    oversized[..4].copy_from_slice(b"rust");
    for (body, bytes, accepted) in [
        (b"rust".to_vec(), 4, true),
        (b"rust".to_vec(), 5, false),
        (oversized, 4, false),
        (b"oops".to_vec(), 4, false),
    ] {
        let mut reader = std::io::Cursor::new(body);
        let result = save_archive(
            &mut reader,
            "fixture",
            bytes,
            "521fe5c9ece1aa1f8b66228171598263574aefc6fa4ba06a61747ec81ee9f5a3",
            &archive,
            &|_| {},
        );
        assert_eq!(result.is_ok(), accepted, "{result:?}");
        assert_eq!(archive.exists(), accepted);
        if reader.get_ref().len() > 5 {
            assert_eq!(reader.position(), 5);
        }
        if accepted {
            fs::remove_file(&archive).unwrap();
        }
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn stopped_pinned_archive_is_never_published() {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("archive");
    let cancellation = crate::native_mods::Cancellation::default();
    let bytes = vec![b'x'; 1 << 17];
    let expected = sunrise_launcher_smod::index::sha256(&bytes);
    let result = save_archive(
        cancellation.reader(bytes.as_slice()),
        "fixture",
        bytes.len() as u64,
        &expected,
        &archive,
        &|_| cancellation.stop(),
    );
    assert!(result.is_err());
    assert!(cancellation.is_stopped());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn stop_interrupts_toolchain_download_stalled_before_headers() {
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = accepted_tx.send(());
        let _ = release_rx.recv();
        drop(stream);
    });

    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("archive");
    let cancellation = crate::native_mods::Cancellation::default();
    let stop = cancellation.clone();
    let stopper = std::thread::spawn(move || {
        accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        stop.stop();
    });

    let started = Instant::now();
    let result = download_archive(
        &format!("http://{addr}/toolchain.tar.xz"),
        16,
        &"00".repeat(32),
        &archive,
        &|_| {},
        &cancellation,
    );
    let elapsed = started.elapsed();

    let _ = release_tx.send(());
    stopper.join().unwrap();
    server.join().unwrap();

    assert!(result.is_err());
    assert!(cancellation.is_stopped());
    assert!(
        elapsed < Duration::from_secs(2),
        "Stop took too long while stalled before headers: {elapsed:?}"
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn stop_interrupts_toolchain_download_stalled_mid_body() {
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (sent_tx, sent_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut req = [0u8; 1024];
        let _ = stream.read(&mut req);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 65536\r\n\r\npart")
            .unwrap();
        stream.flush().unwrap();
        let _ = sent_tx.send(());
        let _ = release_rx.recv();
        drop(stream);
    });

    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("archive");
    let cancellation = crate::native_mods::Cancellation::default();
    let stop = cancellation.clone();
    let stopper = std::thread::spawn(move || {
        sent_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        stop.stop();
    });

    let started = Instant::now();
    let result = download_archive(
        &format!("http://{addr}/toolchain.tar.xz"),
        65536,
        &"00".repeat(32),
        &archive,
        &|_| {},
        &cancellation,
    );
    let elapsed = started.elapsed();

    let _ = release_tx.send(());
    stopper.join().unwrap();
    server.join().unwrap();

    assert!(result.is_err());
    assert!(cancellation.is_stopped());
    assert!(
        elapsed < Duration::from_secs(2),
        "Stop took too long while stalled mid-body: {elapsed:?}"
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn every_host_pins_each_component_once_with_a_sha256() {
    for host in HOSTS {
        let names: Vec<&str> = host.components.iter().map(|(name, ..)| *name).collect();
        assert_eq!(
            &names[..3],
            ["rustc", "cargo", "rust-std"],
            "{}",
            host.target
        );
        for &(name, sha, _) in host.components {
            assert!(
                sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name} {}",
                host.target
            );
        }
    }
    let x64 = HOSTS
        .iter()
        .find(|h| h.target == WINDOWS_X64_TARGET)
        .unwrap();
    let arm64 = HOSTS
        .iter()
        .find(|h| h.target == WINDOWS_ARM64_TARGET)
        .unwrap();
    assert!(x64.target.ends_with("-gnu"), "preserve the x64 toolchain");
    assert!(x64
        .components
        .iter()
        .any(|(name, ..)| *name == "rust-mingw"));
    assert!(arm64
        .components
        .iter()
        .any(|(name, ..)| *name == "rust-mingw"));
    // Only Windows brings a C toolchain; macOS and Linux use the system's.
    for host in HOSTS {
        assert_eq!(
            host.c_toolchain.is_some(),
            host.target.contains("windows"),
            "{}",
            host.target
        );
    }
    assert_eq!(x64.c_toolchain.unwrap().kind, CToolchainKind::WinLibs);
    assert_eq!(arm64.c_toolchain.unwrap().kind, CToolchainKind::LlvmMingw);
    for compiler in [x64.c_toolchain.unwrap(), arm64.c_toolchain.unwrap()] {
        assert!(
            compiler.sha256.len() == 64 && compiler.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "{}",
            compiler.version
        );
        assert!(compiler.url.starts_with("https://") && compiler.url.ends_with(".zip"));
    }
    assert!(x64.c_toolchain.unwrap().url.contains("msvcrt"));
    assert!(arm64.c_toolchain.unwrap().url.contains("ucrt"));
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(host().unwrap().target, "aarch64-apple-darwin");
    }
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        assert_eq!(host().unwrap().target, "x86_64-apple-darwin");
    }
}

#[test]
fn every_system_sunrise_ships_for_can_build_native_mods() {
    // An Intel Mac had a game and a launcher but no compiler (#1161): native builds stopped
    // with "the launcher has none for macos x86_64".
    for (os, arch, game, _) in backend::PLATFORMS {
        let host = host_for(os, arch).unwrap_or_else(|| panic!("no Rust for {game}"));
        let expected_arch = if arch == "aarch64" {
            "aarch64-"
        } else {
            "x86_64-"
        };
        assert!(
            host.target.starts_with(expected_arch),
            "{game}: {}",
            host.target
        );
    }
    assert_eq!(
        host_for("macos", "x86_64").unwrap().target,
        "x86_64-apple-darwin"
    );
}

#[test]
fn game_pe_architecture_selects_only_a_matching_managed_host() {
    assert_eq!(
        host_for("windows", "x86_64").unwrap().target,
        WINDOWS_X64_TARGET,
        "an x64 process under ARM64 emulation stays on the x64 compiler"
    );
    assert_eq!(
        host_for("windows", "aarch64").unwrap().target,
        WINDOWS_ARM64_TARGET
    );
    assert_eq!(windows_target_for_machine(0x8664), Some(WINDOWS_X64_TARGET));
    assert_eq!(
        windows_target_for_machine(0xaa64),
        Some(WINDOWS_ARM64_TARGET)
    );
    assert_eq!(
        windows_target_for_machine(0xa641),
        None,
        "ARM64EC is unsupported"
    );
    assert_eq!(
        windows_target_for_host(WINDOWS_X64_TARGET, WINDOWS_X64_MACHINE),
        Some(WINDOWS_X64_TARGET)
    );
    assert_eq!(
        windows_target_for_host(WINDOWS_ARM64_TARGET, WINDOWS_ARM64_MACHINE),
        Some(WINDOWS_ARM64_TARGET)
    );
    assert_eq!(
        windows_target_for_host(WINDOWS_X64_TARGET, WINDOWS_ARM64_MACHINE),
        None
    );
    assert_eq!(
        windows_target_for_host(WINDOWS_ARM64_TARGET, WINDOWS_X64_MACHINE),
        None
    );
}

#[test]
fn the_players_compiler_settings_stay_out_of_builds() {
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_INCREMENTAL",
        "CARGO_PROFILE_RELEASE_LTO",
        "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_TARGET_DIR",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTUP_TOOLCHAIN",
        "cargo_profile_release_debug",
    ] {
        assert!(scrubbed(name), "{name}");
    }
    for name in [
        "CARGO_BUILD_JOBS",
        "PATH",
        "HOME",
        "CARGO_HOME",
        "CARGO_NET_OFFLINE",
        "CARGO_HTTP_PROXY",
        "HTTPS_PROXY",
        "SSL_CERT_FILE",
    ] {
        assert!(!scrubbed(name), "{name}");
    }
}

#[test]
fn component_files_land_under_the_prefix_and_nothing_else_does() {
    let at = |p: &str| component_path(Path::new(p));
    assert_eq!(
        at("rustc-1.99.1-x/rustc/bin/rustc"),
        Some(PathBuf::from("bin/rustc"))
    );
    assert_eq!(
        at("rust-std-1.99.1-x/rust-std-x/lib/rustlib/x/lib/libstd.rlib"),
        Some(PathBuf::from("lib/rustlib/x/lib/libstd.rlib"))
    );
    for skipped in [
        "rustc-1.99.1-x/install.sh",
        "rustc-1.99.1-x/components",
        "rustc-1.99.1-x/rustc/manifest.in",
        "rustc-1.99.1-x/rustc",
        "rustc-1.99.1-x/../../etc/passwd",
        "/rustc-1.99.1-x/rustc/bin/rustc",
    ] {
        assert_eq!(at(skipped), None, "{skipped}");
    }
}

#[test]
fn a_component_archive_unpacks_as_its_installer_would() {
    let dir = std::env::temp_dir().join(format!("horizon-rust-unpack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("cargo.tar.gz");
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        fs::File::create(&archive).unwrap(),
        flate2::Compression::fast(),
    ));
    for (path, body, mode) in [
        ("cargo-1.99.1-x/install.sh", "#!/bin/sh", 0o755),
        ("cargo-1.99.1-x/cargo/manifest.in", "file:bin/cargo", 0o644),
        ("cargo-1.99.1-x/cargo/bin/cargo", "binary", 0o755),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, path, body.as_bytes()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
    let prefix = dir.join("prefix");
    unpack(
        &archive,
        &prefix,
        &crate::native_mods::Cancellation::default(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(prefix.join("bin/cargo")).unwrap(),
        "binary"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(prefix.join("bin/cargo"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "cargo stays executable");
    }
    assert!(!prefix.join("install.sh").exists() && !prefix.join("manifest.in").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_command_uses_only_the_launchers_toolchain() {
    for host in HOSTS {
        let toolchain = Toolchain {
            dir: PathBuf::from("/rust/1.99.1-x"),
            host,
        };
        the_command_uses_only(&toolchain);
    }
}

fn the_command_uses_only(toolchain: &Toolchain) {
    let cmd = toolchain.command(&toolchain.cargo());
    let env: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let get = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    assert_eq!(get("RUSTUP_TOOLCHAIN"), Some(None));
    assert_eq!(get("RUSTC_WRAPPER"), Some(None));
    assert_eq!(
        PathBuf::from(get("RUSTC").flatten().unwrap()),
        toolchain.dir.join("bin").join(exe("rustc"))
    );
    assert_eq!(
        std::env::split_paths(&get("PATH").flatten().unwrap()).next(),
        Some(toolchain.dir.join("bin"))
    );
    assert!(get("CARGO_HOME").flatten().unwrap().ends_with("cargo"));
}

/// The tools' scratch dir is the build's own (#5189): MinGW-w64's gcc, as and ld can't
/// open a non-ASCII %TEMP%, like the one under a Cyrillic user name.
#[test]
fn the_command_uses_the_builds_own_temp_dir() {
    let tmp = backend::build_dir().join("tmp");
    for host in HOSTS {
        let toolchain = Toolchain {
            dir: PathBuf::from("/rust/1.99.1-x"),
            host,
        };
        let cmd = toolchain.command(&toolchain.cargo());
        for name in ["TMPDIR", "TMP", "TEMP"] {
            let value = cmd
                .get_envs()
                .find(|(k, _)| *k == name)
                .and_then(|(_, v)| v)
                .map(PathBuf::from);
            assert_eq!(
                value.as_deref(),
                Some(tmp.as_path()),
                "{}: {name}",
                host.target
            );
        }
    }
}

#[test]
fn mingw_files_land_under_mingw64_without_the_documentation() {
    let at = |p: &str| mingw_path(Path::new(p));
    assert_eq!(
        at("mingw64/bin/gcc.exe"),
        Some(PathBuf::from("mingw64/bin/gcc.exe"))
    );
    assert_eq!(
        at("mingw64/libexec/gcc/x86_64-w64-mingw32/16.2.0/cc1.exe"),
        Some(PathBuf::from(
            "mingw64/libexec/gcc/x86_64-w64-mingw32/16.2.0/cc1.exe"
        ))
    );
    for skipped in [
        "mingw64",
        "mingw64/share/doc/gcc.html",
        "other/bin/gcc.exe",
        "mingw64/../evil.exe",
        "/mingw64/bin/gcc.exe",
    ] {
        assert_eq!(at(skipped), None, "{skipped}");
    }
}

#[test]
fn a_mingw_zip_unpacks_its_programs_and_not_its_documentation() {
    let dir = std::env::temp_dir().join(format!("sunrise-mingw-unpack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("mingw.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("mingw64/bin/", options).unwrap();
    for (path, body) in [
        ("mingw64/bin/gcc.exe", "gcc"),
        ("mingw64/share/doc/readme.txt", "docs"),
        ("licenses/readme.txt", "outside"),
    ] {
        zip.start_file(path, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let prefix = dir.join("prefix");
    unpack_mingw(
        &archive,
        &prefix,
        &crate::native_mods::Cancellation::default(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(prefix.join("mingw64/bin/gcc.exe")).unwrap(),
        "gcc"
    );
    assert!(!prefix.join("mingw64/share").exists() && !prefix.join("licenses").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn llvm_mingw_paths_are_safe_with_or_without_a_versioned_root() {
    let at = |p: &str| llvm_mingw_path(Path::new(p));
    assert_eq!(at("bin/clang.exe"), Some(PathBuf::from("bin/clang.exe")));
    assert_eq!(
        at("llvm-mingw-20261104-ucrt-aarch64/bin/clang.exe"),
        Some(PathBuf::from("bin/clang.exe"))
    );
    assert_eq!(at("llvm-mingw-20261104-ucrt-aarch64"), None);
    assert_eq!(at("../outside.exe"), None);
    assert_eq!(at("/bin/clang.exe"), None);
}

#[test]
fn an_llvm_mingw_zip_unpacks_to_its_managed_folder() {
    let dir =
        std::env::temp_dir().join(format!("sunrise-llvm-mingw-unpack-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("llvm-mingw.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (path, body) in [
        ("llvm-mingw-20261104-ucrt-aarch64/bin/clang.exe", "clang"),
        ("llvm-mingw-20261104-ucrt-aarch64/LICENSE.txt", "license"),
        ("../outside.exe", "unsafe"),
    ] {
        zip.start_file(path, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let prefix = dir.join("prefix");
    unpack_llvm_mingw(
        &archive,
        &prefix,
        &crate::native_mods::Cancellation::default(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(prefix.join("llvm-mingw/bin/clang.exe")).unwrap(),
        "clang"
    );
    assert_eq!(
        fs::read_to_string(prefix.join("llvm-mingw/LICENSE.txt")).unwrap(),
        "license"
    );
    assert!(!prefix.join("outside.exe").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn arm64_toolchain_is_incomplete_without_each_required_c_and_link_tool() {
    let dir = std::env::temp_dir().join(format!("sunrise-arm64-tools-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let host = HOSTS
        .iter()
        .find(|h| h.target == WINDOWS_ARM64_TARGET)
        .unwrap();
    let toolchain = Toolchain {
        dir: dir.clone(),
        host,
    };
    let bin = toolchain.mingw_bin();
    fs::create_dir_all(&bin).unwrap();
    for name in [
        "aarch64-w64-mingw32-clang",
        "aarch64-w64-mingw32-clang++",
        "llvm-ar",
        "llvm-ranlib",
        "llvm-dlltool",
        "aarch64-w64-mingw32-ar",
        "aarch64-w64-mingw32-windres",
    ] {
        assert!(!toolchain.compiler_available(), "missing {name}");
        fs::write(bin.join(exe(name)), "tool").unwrap();
    }
    assert!(toolchain.compiler_available());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_command_puts_mingw_after_rust_when_the_toolchain_has_it() {
    let dir = std::env::temp_dir().join(format!("sunrise-mingw-path-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let host = HOSTS
        .iter()
        .find(|h| h.target == WINDOWS_X64_TARGET)
        .unwrap();
    let toolchain = Toolchain {
        dir: dir.clone(),
        host,
    };
    fs::create_dir_all(toolchain.mingw_bin()).unwrap();
    let mut cmd = toolchain.command(&toolchain.cargo());
    toolchain.configure_cargo(&mut cmd);
    let path = cmd
        .get_envs()
        .find(|(k, _)| *k == "PATH")
        .and_then(|(_, v)| v)
        .unwrap();
    let first: Vec<PathBuf> = std::env::split_paths(path).take(2).collect();
    assert_eq!(first, [dir.join("bin"), toolchain.mingw_bin()]);
    assert!(!cmd
        .get_envs()
        .any(|(key, _)| matches!(key.to_string_lossy().as_ref(), "AR" | "WINDRES")));
    assert!(cmd.get_args().next().is_none());
    assert!(!cmd
        .get_envs()
        .any(|(key, _)| key == "CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_arm64_command_selects_managed_llvm_c_and_link_tools() {
    let dir = std::env::temp_dir().join(format!(
        "sunrise-arm64-toolchain-{} with spaces",
        std::process::id()
    ));
    let host = HOSTS
        .iter()
        .find(|h| h.target == WINDOWS_ARM64_TARGET)
        .unwrap();
    let toolchain = Toolchain { dir, host };
    let cmd = toolchain.command(&toolchain.cargo());
    let env: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let value = |name: &str| {
        env.iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.as_deref())
    };
    let compiler = toolchain.mingw_bin().join(exe("aarch64-w64-mingw32-clang"));
    assert_eq!(
        value("CC_aarch64_pc_windows_gnullvm"),
        Some(compiler.to_str().unwrap())
    );
    assert_eq!(
        value("AR_aarch64_pc_windows_gnullvm"),
        Some(toolchain.mingw_bin().join(exe("llvm-ar")).to_str().unwrap())
    );
    let mut cargo = toolchain.command(&toolchain.cargo());
    toolchain.configure_cargo(&mut cargo);
    let args: Vec<String> = cargo
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let dlltool = toolchain.mingw_bin().join(exe("llvm-dlltool"));
    let rustflags = toml::Value::Array(vec![
        toml::Value::String("-C".into()),
        toml::Value::String("target-feature=+crt-static".into()),
        toml::Value::String("-C".into()),
        toml::Value::String(format!("dlltool={}", dlltool.display())),
    ]);
    assert_eq!(
        args[1],
        format!("target.{WINDOWS_ARM64_TARGET}.rustflags={rustflags}")
    );
    assert_eq!(
        value("CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER"),
        Some(
            toolchain
                .mingw_bin()
                .join(exe("aarch64-w64-mingw32-clang"))
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        value("AR"),
        Some(
            toolchain
                .mingw_bin()
                .join(exe("aarch64-w64-mingw32-ar"))
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        value("WINDRES"),
        Some(
            toolchain
                .mingw_bin()
                .join(exe("aarch64-w64-mingw32-windres"))
                .to_str()
                .unwrap()
        )
    );
}

#[test]
fn a_linux_build_names_what_is_missing_and_the_distributions_command() {
    // A system with `cc` and `pkg-config` but no development files, as Linux Mint ships.
    let no_dev = |program: &str, args: &[&str]| program != "pkg-config" || args[0] != "--exists";
    let mint = "NAME=\"Linux Mint\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
    let e = linux_needs(no_dev, mint, true).unwrap_err();
    assert!(
        e.contains("lacks them: ALSA's development files, libudev's development files."),
        "{e}"
    );
    assert!(e.ends_with("\nsudo apt install build-essential pkg-config libasound2-dev libudev-dev"));
    // The headless server build needs none of those: only a C compiler and its headers.
    assert_eq!(
        linux_needs(no_dev, mint, false),
        Ok(()),
        "headless links none"
    );
    // Nothing to check through without pkg-config, and nothing at all on SteamOS.
    let nothing = |_: &str, _: &[&str]| false;
    let deck = "ID=steamos\nID_LIKE=arch\n";
    let e = linux_needs(nothing, deck, true).unwrap_err();
    assert!(e.contains("a C compiler (cc), pkg-config, ALSA's"), "{e}");
    assert!(e.contains("SteamOS") && !e.contains("pacman"), "{e}");
    // Headless, the same system names only what it lacks: cc and the C library's headers.
    let e = linux_needs(nothing, deck, false).unwrap_err();
    assert!(e.contains("lacks them: a C compiler (cc)."), "{e}");
    assert!(!e.contains("pkg-config") && !e.contains("ALSA"), "{e}");
    // A C compiler alone is enough for the headless build.
    let cc_only = |program: &str, _: &[&str]| program == "cc";
    assert_eq!(linux_needs(cc_only, mint, false), Ok(()));
    // SteamOS 3's GCC runs but finds no `stdint.h` (#4770).
    let headless = |program: &str, args: &[&str]| program != "cc" || args == ["--version"];
    let e = linux_needs(headless, deck, false).unwrap_err();
    assert!(
        e.contains("lacks them: the C library's headers. SteamOS"),
        "{e}"
    );
    let arch = linux_needs(nothing, "ID=endeavouros\nID_LIKE=arch\n", true).unwrap_err();
    assert!(arch.contains("pacman -S --needed base-devel"), "{arch}");
    let fedora = linux_needs(nothing, "ID=fedora\n", true).unwrap_err();
    assert!(fedora.contains("dnf install"), "{fedora}");
    let other = linux_needs(nothing, "ID=gentoo\n", true).unwrap_err();
    assert!(
        other.contains("package manager") && other.contains("apt install"),
        "{other}"
    );
    // NixOS's packages are in the store: a shell that lists them is the only way in (#5248).
    let nixos = linux_needs(nothing, "NAME=NixOS\nID=nixos\n", true).unwrap_err();
    assert!(nixos.contains("nix-shell"), "{nixos}");
    assert!(
        nixos.contains("lacks them: a C compiler (cc), pkg-config"),
        "{nixos}"
    );
    assert!(!nixos.contains("apt install"), "{nixos}");
    assert_eq!(linux_needs(|_: &str, _: &[&str]| true, mint, true), Ok(()));
}

/// The Windows ARM64 CI invokes this ignored integration test on its real ARM64 runner. It
/// exercises the downloaded host compiler, C dependencies, Windows imports and final linker.
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
#[test]
#[ignore = "downloads the managed Windows ARM64 toolchain and builds Sunrise"]
fn managed_arm64_toolchain_builds_the_game() {
    let home = std::env::var_os("SUNRISE_LAUNCHER_HOME").map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "sunrise-managed-native-arm64-{}",
                std::process::id()
            ))
        },
        PathBuf::from,
    );
    let _ = fs::create_dir_all(&home);
    std::env::set_var("SUNRISE_LAUNCHER_HOME", &home);
    let target_dir = std::env::var_os("SUNRISE_NATIVE_TARGET_DIR").map_or_else(
        || backend::build_dir().join("managed-native-target"),
        PathBuf::from,
    );

    let rust = install(|_| {}).expect("managed ARM64 Rust and LLVM-MinGW install");
    assert_eq!(rust.host.target, WINDOWS_ARM64_TARGET);
    assert!(rust.compiler_available(), "managed LLVM-MinGW is complete");

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut cargo = rust.command(&rust.cargo());
    low_priority(&mut cargo);
    rust.configure_cargo(&mut cargo);
    cargo
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "--profile",
            "fast",
            "-p",
            "sunrise-client",
            "--bin",
            "sunrise-client",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env(
            "CARGO_BUILD_JOBS",
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .to_string(),
        );
    let output = cargo.output().expect("managed cargo starts on ARM64");
    assert!(
        output.status.success(),
        "managed ARM64 native-mod build failed\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let executable = target_dir.join("fast").join(crate::backend::CLIENT);
    assert_eq!(
        crate::native_mods::pe_machine(&executable).unwrap(),
        WINDOWS_ARM64_MACHINE,
        "managed native-mod game must be ARM64"
    );
}
