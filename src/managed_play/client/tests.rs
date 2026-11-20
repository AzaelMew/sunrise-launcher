use super::*;
use serde_json::json;
use std::{fs, sync::Arc};
use sunrise_launcher_smod::{index, manifest, mod_list};

#[test]
fn play_checks_hash_only_takedowns_after_proving_the_actual_package() {
    const TEST: &str = "managed_play::client::tests::play_checks_hash_only_takedowns_after_proving_the_actual_package";
    const BUILD: &str = "v0.1.7";
    if std::env::var_os("PLAY_CATALOGUE_FIXTURE").is_none() {
        let root = tempfile::tempdir().unwrap();
        assert!(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("PLAY_CATALOGUE_FIXTURE", "1")
            .env("SUNRISE_HOME", root.path().join("game"))
            .env("SUNRISE_LAUNCHER_HOME", root.path().join("launcher"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .status()
            .unwrap()
            .success());
        return;
    }
    let home = backend::game_home();
    let build = backend::version_dir(BUILD);
    let prepared = backend::resources(BUILD);
    fs::create_dir_all(&prepared).unwrap();
    fs::write(prepared.join("sunrise-resources.json"), b"{}").unwrap();
    fs::write(build.join(".installed"), []).unwrap();
    fs::write(build.join(backend::CLIENT), "unchanged client").unwrap();
    let row_path = home.join(crate::browser::installed::MANIFEST);
    fs::create_dir_all(row_path.parent().unwrap()).unwrap();
    let relative = format!("{}/lantern.smod", mod_list::DIR);
    let package = home.join(&relative);
    fs::create_dir_all(package.parent().unwrap()).unwrap();
    let write_package = |version| {
        let body = format!("id='lantern'\nname='Lantern'\nversion='{version}'\napi='{}'\ncapabilities=['registries']\n", manifest::API);
        fs::write(
            &package,
            manifest::Package::write_files(&body, &[]).unwrap(),
        )
        .unwrap();
        index::sha256_file(&package).unwrap()
    };
    let old = write_package("1.0.0");
    fs::write(
        &row_path,
        json!({"entries":[{
            "source":"sunrise", "project":"lantern", "slug":"lantern", "title":"Lantern",
            "kind":"mod", "version":"1.0.0", "number":"1.0.0", "file":relative,
            "checksum":format!("SHA-256:{old}"), "page":"", "dependency":false,
        }]})
        .to_string(),
    )
    .unwrap();
    fs::write(home.join(mod_list::FILE), b"mod.lantern:true\n").unwrap();
    let next = write_package("2.0.0");
    let instance = instances::selection().unwrap();
    let client = Client {
        instance: &instance,
        accepted_account: None,
        explicit_account: false,
        requested_build: None,
        piped: false,
    };
    let mut profile = Profile::open(&home, None).unwrap();
    let cancellation = native_mods::Cancellation::default();
    let revoked = |id: &str, hash| {
        let mut result = index::Index::default();
        result.revoked.push(index::Revoked {
            id: id.into(),
            sha256: Some(hash),
            reason: "hash-only takedown fixture".into(),
            date: String::new(),
        });
        Some(Arc::new(result))
    };
    let (active, _) = client
        .ready(
            &mut profile,
            || FirstBuild::installed(BUILD),
            |_| Ok(()),
            (&cancellation, None),
            |_| {},
            || {
                let binding = crate::profile_txn::references(&home).unwrap();
                assert!(binding.targets.contains(BUILD));
                assert!(binding.pointer.is_none());
                revoked("lantern", old)
            },
        )
        .unwrap();
    let row = fs::read_to_string(&row_path).unwrap();
    assert!(row.contains(&next), "{row}");

    let replaced = write_package("3.0.0");
    let error = client
        .ready(
            &mut profile,
            || panic!("already bound"),
            |_| Ok(()),
            (&cancellation, None),
            |_| {},
            || revoked("lantern", replaced),
        )
        .expect_err("actual archive was taken down");
    assert!(String::from(error).contains("hash-only takedown fixture"));
    assert_eq!(profile.active().unwrap(), Some(active.clone()));
    assert_eq!(fs::read_to_string(&row_path).unwrap(), row);

    // A package with no browser row can still be a taken-down compiler input.
    let native = home.join(mod_list::NATIVE_DIR).join("native_lantern.smod");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    let body = format!("id='native_lantern'\nname='Native lantern'\nversion='1.0.0'\napi='{}'\ncapabilities=['registries']\n[native]\ngame='26.3'\nsunrise='{}'\n", manifest::API, "a".repeat(40));
    fs::write(
        &native,
        manifest::Package::write_files(
            &body,
            &[
                (
                    "Cargo.toml",
                    b"[package]\nname='native_lantern'\nversion='1.0.0'\n",
                ),
                ("src/lib.rs", b"fn fixture() {}"),
            ],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(crate::native_status::enabled(&home).is_empty());
    fs::write(
        home.join(mod_list::FILE),
        b"mod.lantern:true\nmod.native_lantern:true\n",
    )
    .unwrap();
    assert_eq!(crate::native_status::enabled(&home).len(), 1);
    let error = client
        .ready(
            &mut profile,
            || panic!("already bound"),
            |_| Ok(()),
            (&cancellation, None),
            |step| assert!(!matches!(step, Progress::Native(_)), "started native work"),
            || revoked("native_lantern", index::sha256_file(&native).unwrap()),
        )
        .expect_err("native input was taken down");
    let error = String::from(error);
    assert!(error.contains("Native lantern was taken down"), "{error}");
    assert_eq!(profile.active().unwrap(), Some(active));
}
