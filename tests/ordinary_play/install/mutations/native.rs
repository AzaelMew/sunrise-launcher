//! Run the real staged developer compiler protocol, then exercise typed hold authority.
use super::*;
use sha2::{Digest, Sha256};
use sunrise_launcher::managed_play::{Active, Held, HeldReason, HeldSource};

const ID: &str = "native_lantern";
const ARCHIVE: &str = "sunrise-native-mods/native_lantern.smod";

fn native_release(
    fixture: &Fixture,
    registry: &mut index::Index,
    version: &str,
    source: &[u8],
) -> Vec<u8> {
    let project = &mut registry.projects[0];
    let number = version.to_owned();
    let manifest = format!("id='{ID}'\nname='Native Lantern'\nversion='{version}'\napi='0.2'\nside='both'\ncapabilities=['registries']\n[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n", "a".repeat(40), "c".repeat(40));
    let cargo = format!("[package]\nname='{ID}'\nversion='{version}'\n");
    let bytes = Package::write_files(
        &manifest,
        &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", source)],
    )
    .unwrap();
    let name = format!("{ID}-{version}.smod");
    let path = fixture.root.join(&name);
    fs::write(&path, &bytes).unwrap();
    let facts = index::inspect(&index::Kind::NativeMod, &path).unwrap();
    let mut row = project.versions[0].clone();
    row.version = number;
    row.game = "26.3".into();
    row.api = facts.api;
    row.side = facts.side;
    row.capabilities = facts.capabilities.into();
    row.dependencies = Some(Vec::new());
    row.file = index::File {
        name,
        url: format!("file://{}", path.display()),
        size: bytes.len() as u64,
        sha256: index::sha256(&bytes),
        signature: None,
    };
    project
        .versions
        .retain(|version| version.version != row.version);
    project.versions.insert(0, row);
    bytes
}

fn configured() -> (Fixture, index::Index, Vec<u8>) {
    let fixture = Fixture::new();
    let sdk = fixture.root.join("sdk/runtime/bin");
    fs::create_dir_all(&sdk).unwrap();
    fs::copy(
        fixture
            .data
            .join("versions")
            .join(BUILD)
            .join("sunrise-client"),
        sdk.join("sunrise-client"),
    )
    .unwrap();
    let rust_version = include_str!("../../../../rust-version").trim();
    let rust = fixture
        .data
        .join(format!("rust/{rust_version}-x86_64-unknown-linux-gnu"));
    fs::create_dir_all(&rust).unwrap();
    fs::write(rust.join(".installed"), []).unwrap();
    fs::write(fixture.root.join("rust-version"), rust_version).unwrap();
    script(
        &fixture.root.join("sdk-tool"),
        include_str!("native_tool.py"),
    );
    let mut index = fixture.registry();
    let mut project = index.project("coords_hud").unwrap().clone();
    project.id = ID.into();
    project.name = "Native Lantern".into();
    project.kind = index::Kind::NativeMod;
    project.key = None;
    index.projects = vec![project];
    let bytes = native_release(
        &fixture,
        &mut index,
        "1.0.0",
        b"#[no_mangle]\npub extern \"C\" fn fixture_version() -> u32 { 1 }\n",
    );
    index.projects[0].versions.truncate(1);
    fixture.publish_registry(&index);
    fs::write(
        fixture.data.join("settings.json"),
        json!({"native_mods_allowed_at": 123, "native_consent_revision": 1}).to_string(),
    )
    .unwrap();
    let (ok, text) = native_client(&fixture, &["mods", "install", ID, "--source", "sunrise"]);
    assert!(
        ok,
        "{text}\n{}",
        fs::read_to_string(fixture.data.join("native/build.log")).unwrap_or_default()
    );
    assert!(selection::is_enabled(&fixture.home, ID));
    assert_eq!(fs::read(fixture.home.join(ARCHIVE)).unwrap(), bytes);
    assert_eq!(
        fs::read_to_string(fixture.root.join("compiled")).unwrap(),
        "1.0.0\n"
    );
    (fixture, index, bytes)
}

fn native_client(fixture: &Fixture, args: &[&str]) -> (bool, String) {
    let output = fixture
        .registry_client(args)
        .env("FIXTURE_NATIVE_ROOT", &fixture.root)
        .env(
            "SUNRISE_LAUNCHER_DEVELOPER_TOOL",
            fixture.root.join("sdk-tool"),
        )
        .env("SUNRISE_LAUNCHER_DEVELOPER_SDK", fixture.root.join("sdk"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (
        output.status.success(),
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

#[test]
fn only_the_exact_enabled_before_hold_can_restore_after_native_proof() {
    for case in ["exact", "digest", "location", "source", "already-off"] {
        let (fixture, mut index, old) = configured();
        let (ok, text) = run(&fixture, &["mods", "disable", ID]);
        assert!(ok, "{text}");
        fs::write(fixture.home.join("finish"), []).unwrap();
        let (ok, text) = run(&fixture, &["play"]);
        assert!(!ok && fixture.home.join("args").is_file(), "{text}");
        assert_eq!(
            Active::parse(&fixture.pointer()).unwrap().executable(),
            BUILD
        );
        let hold = Held::new(
            ID.into(),
            if case == "digest" {
                [7; 32]
            } else {
                Sha256::digest(&old).into()
            },
            if case == "location" {
                "sunrise-native-mods/other.smod"
            } else {
                ARCHIVE
            }
            .into(),
            if case == "source" {
                HeldSource::Local {
                    version: "1.0.0".into(),
                }
            } else {
                HeldSource::Registry {
                    project: ID.into(),
                    version: "1.0.0".into(),
                }
            },
            case != "already-off",
            HeldReason::NoFit {
                build: BUILD.into(),
            },
        )
        .unwrap();
        // Switch itself is a separate route. Seed exactly the committed typed authority it will publish.
        let mut pointer: Value = serde_json::from_slice(&fixture.pointer()).unwrap();
        pointer["receipts"] = json!([{ "id": ([8_u8; 16]), "waiting_for_update": [hold.clone()], "removed": [], "acknowledged": true }]);
        let pointer = serde_json::to_vec(&pointer).unwrap();
        let before = Active::parse(&pointer).unwrap();
        assert_eq!(before.holds().next(), Some(&hold));
        fs::write(fixture.home.join("mod-browser/active-build.json"), pointer).unwrap();
        let next = native_release(
            &fixture,
            &mut index,
            "1.1.0",
            b"#[no_mangle]\npub extern \"C\" fn fixture_version() -> u32 { 2 }\n",
        );
        fixture.publish_registry(&index);
        let (ok, text) = native_client(&fixture, &["mods", "update", ID]);
        assert!(
            ok,
            "{case}: {text}\n{}",
            fs::read_to_string(fixture.data.join("native/build.log")).unwrap_or_default()
        );
        let active = Active::parse(&fixture.pointer()).unwrap();
        let restored = case == "exact";
        assert_eq!(selection::is_enabled(&fixture.home, ID), restored, "{case}");
        assert_eq!(active.executable() != BUILD, restored, "{case}");
        assert_eq!(
            active.holds().next(),
            (!restored).then_some(&hold),
            "{case}"
        );
        assert_eq!(
            fs::read(fixture.home.join(ARCHIVE)).unwrap(),
            next,
            "{case}"
        );
        let built = fs::read_to_string(fixture.root.join("compiled")).unwrap();
        assert_eq!(
            built,
            if restored {
                "1.0.0\n1.1.0\n"
            } else {
                "1.0.0\n"
            },
            "{case}"
        );
    }
}

#[test]
fn failed_native_update_keeps_the_working_predecessor() {
    let (fixture, mut index, old) = configured();
    let pointer = fixture.pointer();
    assert_ne!(Active::parse(&pointer).unwrap().executable(), BUILD);
    let rows_path = fixture.home.join(index::browser_state::MANIFEST);
    let rows = fs::read(&rows_path).unwrap();
    let list_path = fixture.home.join(mod_list::FILE);
    let list = fs::read(&list_path).unwrap();
    native_release(
        &fixture,
        &mut index,
        "1.1.0",
        b"compile_error!(\"fixture compile error\");",
    );
    fixture.publish_registry(&index);
    let (ok, text) = native_client(&fixture, &["mods", "update", ID]);
    assert!(
        !ok && text.contains("Could not compile Native Lantern."),
        "{text}"
    );
    let log = fs::read_to_string(fixture.data.join("native/build.log")).unwrap();
    assert!(log.contains("fixture compile error"), "{log}");
    assert_eq!(
        fs::read_to_string(fixture.root.join("compiled")).unwrap(),
        "1.0.0\n1.1.0\n"
    );
    assert_eq!(fixture.pointer(), pointer);
    assert_eq!(fs::read(rows_path).unwrap(), rows);
    assert_eq!(fs::read(list_path).unwrap(), list);
    assert_eq!(fs::read(fixture.home.join(ARCHIVE)).unwrap(), old);
}
