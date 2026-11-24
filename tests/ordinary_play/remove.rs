//! Real CLI/MCP Remove, the exact enabled graph, and world evidence at cutoff.
use super::*;
use std::collections::BTreeMap;
use sunrise_launcher::browser::mod_list as selection;
use sunrise_launcher_nbt::{self as nbt, Limits, NamedTag, Tag};
use sunrise_launcher_smod::{index, Package};

impl Fixture {
    /// Runs Play once, so the installed mods become the profile's committed inputs.
    fn commit_play(&self) {
        fs::write(self.home.join("finish"), []).unwrap();
        let out = self.launcher(&["play"]).output().unwrap();
        assert_eq!(out.status.code(), Some(7), "{out:?}");
        fs::remove_file(self.home.join("finish")).unwrap();
        fs::remove_file(self.home.join("args")).unwrap();
    }
}

fn world(home: &Path, folder: &str, ids: &[&str]) {
    let tags = ids
        .iter()
        .map(|id| {
            Tag::compound([
                ("ModId", Tag::String((*id).into())),
                ("ModVersion", Tag::String("1.0.0".into())),
            ])
        })
        .collect();
    world_with_evidence(
        home,
        folder,
        Tag::compound([("ContentModList", Tag::List(tags))]),
    );
}

fn world_with_evidence(home: &Path, folder: &str, sunrise: Tag) {
    let dest = home.join("saves").join(folder);
    fs::create_dir_all(&dest).unwrap();
    let root = Tag::compound([
        (
            "Data",
            Tag::compound([("LevelName", Tag::String(folder.into()))]),
        ),
        ("sunrise", sunrise),
    ]);
    fs::write(
        dest.join("level.dat"),
        nbt::write_named(
            &NamedTag {
                name: "".into(),
                tag: root,
            },
            Limits::default(),
        )
        .unwrap(),
    )
    .unwrap();
}

fn run(fixture: &Fixture, args: &[&str]) -> (bool, String) {
    let out = fixture
        .launcher(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Writes content mod `id` (titled `name`, with `definitions`) into the profile's mods folder.
fn content_mod(
    fixture: &Fixture,
    id: &str,
    name: &str,
    dependencies: &str,
    definitions: &[(&str, &[u8])],
) -> Vec<u8> {
    let archive = Package::write_files(
        &format!("id='{id}'\nname='{name}'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n{dependencies}"),
        definitions,
    )
    .unwrap();
    let directory = fixture.home.join(mod_list::DIR);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(format!("{id}.smod")), &archive).unwrap();
    archive
}

/// Lanterns (root_mod) and the Lantern Core it requires, turned on and committed by Play.
fn install_root(
    fixture: &Fixture,
    root_definitions: &[(&str, &[u8])],
) -> BTreeMap<String, Vec<u8>> {
    let archives = BTreeMap::from([
        (
            "root_mod".to_owned(),
            content_mod(
                fixture,
                "root_mod",
                "Lanterns",
                "[dependencies]\nrequired_mod = '^1'",
                root_definitions,
            ),
        ),
        (
            "required_mod".to_owned(),
            content_mod(
                fixture,
                "required_mod",
                "Lantern Core",
                "",
                &[(
                    "data/required_mod/sunrise/block/core.json",
                    br#"{"properties":{"lit":[false,true]}}"#,
                )],
            ),
        ),
    ]);
    // Registry rows, as an install from Sunrise's registry records them.
    let entries: Vec<Value> = [("root_mod", "Lanterns"), ("required_mod", "Lantern Core")]
        .into_iter()
        .map(|(id, title)| {
            json!({
                "source": "sunrise",
                "project": id,
                "slug": id,
                "title": title,
                "kind": "mod",
                "version": "1.0.0",
                "number": "1.0.0",
                "file": format!("{}/{id}.smod", mod_list::DIR),
                "checksum": format!("SHA-256:{}", index::sha256(&archives[id])),
                "page": "",
                "dependency": id == "required_mod",
            })
        })
        .collect();
    let rows = fixture.home.join(index::browser_state::MANIFEST);
    fs::create_dir_all(rows.parent().unwrap()).unwrap();
    fs::write(rows, json!({ "entries": entries }).to_string()).unwrap();
    selection::set_all_enabled(&fixture.home, &["root_mod", "required_mod"], true).unwrap();
    fixture.commit_play();
    archives
}

fn live_state(fixture: &Fixture) -> [Vec<u8>; 4] {
    [
        fixture.pointer(),
        fs::read(fixture.home.join(index::browser_state::MANIFEST)).unwrap(),
        fs::read(fixture.home.join(mod_list::FILE)).unwrap(),
        fs::read(fixture.home.join(mod_list::DIR).join("root_mod.smod")).unwrap(),
    ]
}

#[test]
fn namespace_only_and_malformed_worlds_cannot_bypass_the_decision() {
    let fixture = Fixture::new();
    let defs = [("data/root_mod/sunrise/block/amber.json", b"{}".as_slice())];
    install_root(&fixture, &defs);
    let old = live_state(&fixture);
    for evidence in [
        Tag::compound([(
            "BlockNamespaces",
            Tag::List(vec![Tag::String("root_mod".into())]),
        )]),
        Tag::compound([(
            "ContentModList",
            Tag::List(vec![Tag::compound([
                ("ModId", Tag::String("unrelated".into())),
                ("ModVersion", Tag::Int(7)),
            ])]),
        )]),
    ] {
        world_with_evidence(&fixture.home, "Same world", evidence);
        let (ok, text) = run(&fixture, &["mods", "remove", "root_mod"]);
        assert!(
            !ok && text.contains("1 saved world") && text.contains("--confirm-world-loss"),
            "{text}"
        );
        assert_eq!(live_state(&fixture), old);
    }
}

#[test]
fn explicit_remove_refuses_changed_worlds_or_archive_bytes_at_cutoff() {
    for mode in ["add-world", "replace-world", "replace-archive"] {
        let fixture = Fixture::new();
        let archives = install_root(&fixture, &[]);
        world(&fixture.home, "First world", &["root_mod"]);
        let old = live_state(&fixture);
        let required = fixture.home.join(mod_list::DIR).join("required_mod.smod");
        let mut worker = Command::new(std::env::current_exe().unwrap());
        for (key, value) in fixture.launcher(&[]).get_envs() {
            worker.env(key, value.unwrap());
        }
        let output = worker
            .args(["--exact", "remove::remove_cutoff_worker"])
            .env("FIXTURE_REMOVE_CUTOFF", mode)
            .output()
            .unwrap();
        assert!(output.status.success(), "{mode}: {output:?}");
        match mode {
            "replace-world" => assert_eq!(
                fs::read(fixture.home.join("saves/First world/level.dat")).unwrap(),
                fs::read(fixture.home.join("previous-world/level.dat")).unwrap(),
            ),
            "add-world" => assert!(fixture.home.join("saves/New world/level.dat").is_file()),
            _ => (),
        }
        assert_eq!(live_state(&fixture), old);
        let expected = if mode == "replace-archive" {
            replacement()
        } else {
            archives["required_mod"].clone()
        };
        assert_eq!(fs::read(required).unwrap(), expected);
    }
}

fn replacement() -> Vec<u8> {
    Package::write_files(
        "id='required_mod'\nname='A different file'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n",
        &[],
    ).unwrap()
}

#[test]
fn remove_cutoff_worker() {
    let Ok(mode) = std::env::var("FIXTURE_REMOVE_CUTOFF") else {
        return;
    };
    use sunrise_launcher::{browser::sunrise::*, instances, managed_play};
    let instance = instances::selection().unwrap();
    let identity = install_target(&instance).unwrap();
    let archive = instance.home.join(mod_list::DIR).join("required_mod.smod");
    let digest = index::sha256_file(&archive).unwrap();
    let decide = |impact: &ChangeImpact| {
        let detail = impact.detail();
        assert!(detail.contains("1 saved world"), "{detail}");
        assert!(detail.contains("Lanterns (root_mod)"), "{detail}");
        assert!(
            detail.contains("sunrise-mods/required_mod.smod") && detail.contains(&digest),
            "{detail}"
        );
        match mode.as_str() {
            "replace-archive" => fs::write(&archive, replacement()).unwrap(),
            "replace-world" => {
                fs::rename(
                    instance.home.join("saves/First world"),
                    instance.home.join("previous-world"),
                )
                .unwrap();
                world(&instance.home, "First world", &["root_mod"]);
            }
            _ => world(&instance.home, "New world", &["root_mod"]),
        }
        true
    };
    let hooks = InstallHooks {
        progress: &|_| {},
        continue_after_failure: &|_| panic!("content-only"),
        cancellation: None,
    };
    let error = remove_mods(
        &instance,
        identity,
        &["required_mod".into()],
        || managed_play::headless_first(&instance),
        hooks,
        &decide,
    )
    .unwrap_err();
    let expected = if mode == "replace-archive" {
        "Sunrise couldn't verify the profile's saved game build (managed_play/first/edits.rs:"
    } else {
        "The affected worlds changed after review. Nothing was changed; try again."
    };
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn remove_names_dependents_even_without_worlds_and_requires_compatible_activation() {
    let fixture = Fixture::new();
    install_root(&fixture, &[]);
    let (ok, text) = run(&fixture, &["mods", "remove", "required_mod"]);
    assert!(ok && text.contains("Lanterns (root_mod)"), "{text}");
    assert!(!text.contains("saved world"), "{text}");
    assert_eq!(text.matches("will be turned off").count(), 1, "{text}");
    assert!(!selection::is_enabled(&fixture.home, "root_mod"));
    let (ok, text) = run(&fixture, &["mods", "enable", "root_mod"]);
    assert!(!ok && text.contains("needs required_mod"), "{text}");
    content_mod(&fixture, "required_mod", "Lantern Core", "", &[]);
    selection::set_enabled(&fixture.home, "required_mod", true).unwrap();
    assert!(!selection::is_enabled(&fixture.home, "root_mod"));
    let (ok, text) = run(&fixture, &["mods", "enable", "root_mod"]);
    assert!(ok && !text.contains("saved world"), "{text}");
    assert!(selection::is_enabled(&fixture.home, "root_mod"));
}

fn local(fixture: &Fixture, id: &str, required: &str, native: bool) -> (PathBuf, Vec<u8>) {
    let native_manifest = if native {
        format!("[native]\nsunrise='{}'\ngame='26.3'\n", "a".repeat(40))
    } else {
        String::new()
    };
    let text = format!("id='{id}'\nname='{id} title'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n[dependencies]\n{required}\n{native_manifest}");
    let cargo = format!("[package]\nname='{id}'\nversion='1.0.0'\n");
    let native_files: &[(&str, &[u8])] = &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", b"")];
    let archive = Package::write_files(&text, if native { native_files } else { &[] }).unwrap();
    let directory = fixture.home.join(if native {
        mod_list::NATIVE_DIR
    } else {
        mod_list::DIR
    });
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{id}.smod"));
    fs::write(&path, &archive).unwrap();
    (path, archive)
}

#[test]
fn remove_accepts_local_and_github_archives_but_not_unpacked_folders() {
    let fixture = Fixture::new();
    install_root(&fixture, &[]);
    let loose = local(&fixture, "local_lamp", "", false);
    let github = local(&fixture, "github_lamp", "", true);
    let sidecar = github.0.with_extension("origin.json");
    fs::write(&sidecar, json!({"id":"github_lamp", "version":"1.0.0", "owner":"fixture", "repo":"lanterns", "commit":"a".repeat(40), "sha256":index::sha256(&github.1)}).to_string()).unwrap();
    let unpacked = fixture.home.join(mod_list::DIR).join("unpacked_lamp");
    fs::create_dir(&unpacked).unwrap();
    let manifest = unpacked.join("sunrise-mod.toml");
    fs::write(&manifest, "id='unpacked_lamp'\nname='Unpacked Lamp'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n").unwrap();
    let expected = live_state(&fixture);
    let (ok, text) = run(&fixture, &["mods", "remove", "unpacked_lamp"]);
    assert!(!ok && text.contains("Nothing was removed"), "{text}");
    assert!(manifest.is_file());
    assert_eq!(live_state(&fixture), expected);

    for (id, (path, bytes)) in [("github_lamp", github), ("local_lamp", loose)] {
        let (ok, text) = run(&fixture, &["mods", "remove", id]);
        assert!(ok && text.contains(&index::sha256(&bytes)), "{text}");
        assert!(!path.exists());
    }
    assert!(!sidecar.exists());
    assert!(manifest.is_file());
    assert_eq!(
        fs::read(fixture.home.join(index::browser_state::MANIFEST)).unwrap(),
        expected[1]
    );
}

#[test]
fn remove_cleans_renamed_legacy_github_receipts_and_windows_browser_rows() {
    let fixture = Fixture::new();
    install_root(&fixture, &[]);

    // A legacy GitHub receipt sits at <id>.origin.json even when the .smod was renamed.
    let (canonical, github_bytes) = local(&fixture, "renamed_lamp", "", true);
    let renamed = canonical.with_file_name("custom-lamp-copy.smod");
    fs::rename(&canonical, &renamed).unwrap();
    let receipt = canonical.with_extension("origin.json");
    fs::write(
        &receipt,
        json!({
            "id": "renamed_lamp",
            "version": "1.0.0",
            "owner": "fixture",
            "repo": "lanterns",
            "commit": "a".repeat(40),
            "sha256": index::sha256(&github_bytes),
        })
        .to_string(),
    )
    .unwrap();

    // Another browser entry claiming the receipt path must still block removing it.
    let manifest_path = fixture.home.join(index::browser_state::MANIFEST);
    let saved_manifest = fs::read(&manifest_path).unwrap();
    let mut conflict: Value = serde_json::from_slice(&saved_manifest).unwrap();
    conflict["entries"].as_array_mut().unwrap().push(json!({
        "source": "sunrise",
        "project": "other_row",
        "slug": "other_row",
        "title": "Other Row",
        "kind": "mod",
        "version": "1.0.0",
        "number": "1.0.0",
        "file": format!("{}/renamed_lamp.origin.json", mod_list::NATIVE_DIR),
        "checksum": format!("SHA-256:{}", "0".repeat(64)),
        "page": "",
        "dependency": false,
    }));
    fs::write(&manifest_path, serde_json::to_vec(&conflict).unwrap()).unwrap();
    let (ok, text) = run(&fixture, &["mods", "remove", "renamed_lamp"]);
    assert!(!ok && text.contains("Other Row"), "{text}");
    assert!(renamed.is_file());
    assert!(receipt.is_file());

    // Without the conflicting claim, Remove deletes both the renamed archive and its receipt.
    fs::write(&manifest_path, &saved_manifest).unwrap();
    let (ok, text) = run(&fixture, &["mods", "remove", "renamed_lamp"]);
    assert!(ok && text.contains(&index::sha256(&github_bytes)), "{text}");
    assert!(!renamed.exists());
    assert!(!receipt.exists());

    // A Windows-style backslash path in mod-browser.json is matched and removed with the archive.
    let (win_path, win_bytes) = local(&fixture, "win_lamp", "", false);
    let mut rows: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    rows["entries"].as_array_mut().unwrap().push(json!({
        "source": "sunrise",
        "project": "win_lamp",
        "slug": "win_lamp",
        "title": "Win Lamp",
        "kind": "mod",
        "version": "1.0.0",
        "number": "1.0.0",
        "file": format!("{}\\win_lamp.smod", mod_list::DIR),
        "checksum": format!("SHA-256:{}", index::sha256(&win_bytes)),
        "page": "",
        "dependency": false,
    }));
    fs::write(&manifest_path, serde_json::to_vec(&rows).unwrap()).unwrap();
    let (ok, text) = run(&fixture, &["mods", "remove", "win_lamp"]);
    assert!(ok && text.contains(&index::sha256(&win_bytes)), "{text}");
    assert!(!win_path.exists());
    let after: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    assert!(
        !after["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["project"] == "win_lamp"),
        "Windows row survived removal: {after}"
    );
}

#[test]
fn remove_refuses_running_game_and_disables_exact_dependents_in_place_in_one_commit() {
    let fixture = Fixture::new();
    let archives = install_root(&fixture, &[]);
    let mut play = Running(fixture.launcher(&["play"]).spawn().unwrap());
    assert_resource_args(&fixture, &fixture.game_arguments());
    let old = live_state(&fixture);
    let (ok, text) = run(
        &fixture,
        &["mods", "remove", "required_mod", "--confirm-world-loss"],
    );
    assert!(
        !ok && text.contains("This profile is in use. Close the game"),
        "{text}"
    );
    assert_eq!(live_state(&fixture), old);
    fs::write(fixture.home.join("finish"), []).unwrap();
    assert_eq!(play.0.wait().unwrap().code(), Some(7));
    fs::remove_file(fixture.home.join("args")).unwrap();

    let transitive = local(&fixture, "local_native", "root_mod='^1'", true);
    let github = local(&fixture, "github_native", "local_native='^1'", true);
    let origin = github.0.with_extension("origin.json");
    let origin_bytes = serde_json::to_vec(&json!({"id":"github_native", "version":"1.0.0", "owner":"fixture", "repo":"native", "commit":"a".repeat(40), "sha256":index::sha256(&github.1)})).unwrap();
    fs::write(&origin, &origin_bytes).unwrap();
    let optional = local(
        &fixture,
        "optional_mod",
        "required_mod={version='^1',optional=true}",
        false,
    );
    let off = local(&fixture, "off_mod", "required_mod='^1'", false);
    selection::set_all_enabled(
        &fixture.home,
        &["local_native", "github_native", "optional_mod"],
        true,
    )
    .unwrap();
    world(&fixture.home, "Native world", &["local_native"]);
    fs::create_dir(fixture.home.join("backups")).unwrap();
    fs::write(fixture.home.join("backups/unchanged.zip"), b"older backup").unwrap();
    let old = live_state(&fixture);
    let (ok, text) = run(&fixture, &["mods", "remove", "required_mod"]);
    assert!(
        !ok && text.contains("1 saved world") && text.contains("--confirm-world-loss"),
        "{text}"
    );
    for name in [
        "Lanterns (root_mod)",
        "local_native title (local_native)",
        "github_native title (github_native)",
    ] {
        assert!(text.contains(name), "{text}");
    }
    assert!(
        !text.contains("optional_mod") && !text.contains("off_mod"),
        "{text}"
    );
    assert_eq!(live_state(&fixture), old);

    let mut mcp = Running(
        fixture
            .launcher(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = mcp.0.stdin.take().unwrap();
    let mut output = BufReader::new(mcp.0.stdout.take().unwrap());
    let mut rpc = |id, method, params| mcp_request(&mut input, &mut output, id, method, params);
    rpc(1, "initialize", json!({}));
    let declined = rpc(
        2,
        "tools/call",
        json!({"name":"mods_remove", "arguments":{"project":"required_mod","confirm":true}}),
    );
    assert_eq!(declined["isError"], true, "{declined}");
    assert!(declined.to_string().contains("1 saved world"), "{declined}");
    assert_eq!(live_state(&fixture), old);
    let accepted = rpc(
        3,
        "tools/call",
        json!({"name":"mods_remove", "arguments":{"project":"required_mod","confirm":true,"confirm_world_loss":true}}),
    );
    assert_ne!(accepted["isError"], true, "{accepted}");
    assert!(!fixture
        .home
        .join(mod_list::DIR)
        .join("required_mod.smod")
        .exists());
    assert_eq!(
        fs::read(fixture.home.join(mod_list::DIR).join("root_mod.smod")).unwrap(),
        archives["root_mod"]
    );
    assert_eq!(fs::read(origin).unwrap(), origin_bytes);
    for (path, archive) in [transitive, github, optional, off] {
        assert_eq!(fs::read(path).unwrap(), archive);
    }
    for id in ["root_mod", "local_native", "github_native", "off_mod"] {
        assert!(!selection::is_enabled(&fixture.home, id));
    }
    assert!(selection::is_enabled(&fixture.home, "optional_mod"));
    assert_eq!(
        fs::read(fixture.home.join("backups/unchanged.zip")).unwrap(),
        b"older backup"
    );
    let reduced = fixture.pointer();
    let (ok, text) = run(&fixture, &["mods", "enable", "root_mod"]);
    assert!(!ok && text.contains("needs required_mod"), "{text}");
    assert_eq!(fixture.pointer(), reduced);
    let mut play = Running(fixture.launcher(&["play"]).spawn().unwrap());
    assert_resource_args(&fixture, &fixture.game_arguments());
    assert_eq!(play.0.wait().unwrap().code(), Some(7));
    assert_eq!(fixture.pointer(), reduced);
}
