use super::*;

#[test]
fn selected_jdk_reaches_external_preparers() {
    const CHILD: &str = "SUNRISE_LAUNCHER_JAVA_TEST";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = PathBuf::from(root);
        let home = java(25, true, &[], &root.join("missing.json"), &|_| {}).unwrap();
        assert_eq!(
            fs::canonicalize(&home).unwrap(),
            fs::canonicalize(root.join("jdk25")).unwrap()
        );
        // An older preparer invokes Java tools by name; both must now resolve to this JDK.
        for name in ["java", "javac"] {
            let mut cmd = crate::process::command(name);
            with_java(&mut cmd, &home);
            let output = cmd.arg("-version").output().unwrap();
            assert!(output.status.success());
            let text = String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr);
            assert!(text.contains("25.0.1"), "{name}: {text}");
            assert_eq!(
                cmd.get_envs()
                    .find(|(key, _)| *key == "JAVA_HOME")
                    .unwrap()
                    .1,
                Some(home.as_os_str())
            );
        }
        return;
    }

    let root = std::env::temp_dir().join(format!("sunrise-launcher-java-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let executable =
        |bin: &Path, name: &str| bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let probe = executable(&root, "probe");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/java_probe.rs");
    let output = crate::process::command("rustc")
        .arg(source)
        .arg("-o")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (home, compiler, runtime) in [
        ("jre21", None, "21.0.1"),
        ("jdk25", Some("25.0.1"), "25.0.1"),
    ] {
        let bin = root.join(home).join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (name, version) in [("javac", compiler), ("java", Some(runtime))] {
            if let Some(version) = version {
                fs::copy(&probe, executable(&bin, name)).unwrap();
                fs::write(bin.join(format!("{name}.version")), version).unwrap();
            }
        }
    }
    let output = crate::process::command(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "backend::java_tests::selected_jdk_reaches_external_preparers",
            "--nocapture",
        ])
        .env(CHILD, &root)
        .env_remove("JAVA_HOME")
        .env(
            "PATH",
            std::env::join_paths([root.join("jre21/bin"), root.join("jdk25/bin")]).unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_downloaded_macos_runtime_is_found_inside_its_bundle() {
    let program = if cfg!(windows) { "java.exe" } else { "java" };
    let manifest_dir = tempfile::tempdir().unwrap();
    let manifest = manifest_dir.path().join("version.json");
    fs::write(
        &manifest,
        r#"{"javaVersion":{"component":"java-runtime-test"}}"#,
    )
    .unwrap();
    // A stand-in for `jdk::find`: it accepts a candidate only with `bin/java` directly in it.
    let find = |_: i64, more: &[PathBuf]| {
        let java = more
            .first()
            .ok_or_else(|| "no java".to_string())?
            .join("bin")
            .join(program);
        if java.is_file() {
            Ok(crate::jdk::Jdk {
                javac: java.clone(),
                java,
            })
        } else {
            Err("no java".to_string())
        }
    };
    for bundle in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let expected = if bundle {
            dir.path().join("jre.bundle/Contents/Home")
        } else {
            dir.path().to_path_buf()
        };
        fs::create_dir_all(expected.join("bin")).unwrap();
        fs::write(expected.join("bin").join(program), b"").unwrap();
        let download = |component: &str, _: &str| {
            assert_eq!(component, "java-runtime-test");
            Ok(dir.path().to_path_buf())
        };
        assert_eq!(java_with(find, 21, &[], &manifest, &download), Ok(expected));
    }
}
