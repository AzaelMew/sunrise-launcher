//! Real CLI/MCP Update and Remove, the exact enabled graph, and world evidence at cutoff.
use super::*;
use sunrise_launcher::browser::mod_list as selection;
use sunrise_launcher_nbt::{self as nbt, Limits, NamedTag, Tag};

fn world(home: &Path, folder: &str, ids: &[&str]) {
    let dest = home.join("saves").join(folder);
    fs::create_dir_all(&dest).unwrap();
    let tags = ids
        .iter()
        .map(|id| {
            Tag::compound([
                ("ModId", Tag::String((*id).into())),
                ("ModVersion", Tag::String("1.0.0".into())),
            ])
        })
        .collect();
    let root = Tag::compound([
        (
            "Data",
            Tag::compound([("LevelName", Tag::String(folder.into()))]),
        ),
        (
            "sunrise",
            Tag::compound([("ContentModList", Tag::List(tags))]),
        ),
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

fn release(
    fixture: &Fixture,
    registry: &mut index::Index,
    id: &str,
    version: &str,
    files: &[(&str, &[u8])],
) -> Vec<u8> {
    let project = registry.projects.iter_mut().find(|p| p.id == id).unwrap();
    let mut row = project.versions[0].clone();
    row.version = version.into();
    let deps = match id {
        "root_mod" => "[dependencies]\nrequired_mod='^1'\n",
        _ => "",
    };
    let package = Package::write_files(&format!("id='{id}'\nname='{}'\nversion='{version}'\napi='0.2'\nside='both'\ncapabilities=['registries']\n{deps}", project.name), files).unwrap();
    let path = fixture.root.join(format!("{id}-{version}.smod"));
    fs::write(&path, &package).unwrap();
    row.file = index::File {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        url: format!("file://{}", path.display()),
        size: package.len() as u64,
        sha256: index::sha256(&package),
        signature: None,
    };
    project.versions.retain(|v| v.version != version);
    project.versions.insert(0, row);
    package
}

fn run(fixture: &Fixture, args: &[&str]) -> (bool, String) {
    let out = fixture
        .registry_client(args)
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

fn install_root(fixture: &Fixture, registry: &index::Index) {
    fixture.publish_registry(registry);
    let (ok, text) = run(
        fixture,
        &["mods", "install", "root_mod", "--source", "sunrise"],
    );
    assert!(ok, "{text}");
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
fn changed_world_evidence_at_cutoff_keeps_live_update_and_remove_unchanged() {
    for action in ["update", "remove"] {
        let fixture = Fixture::new();
        let (mut registry, _) = fixture.content_index();
        let defs: &[(&str, &[u8])] = &[("data/root_mod/sunrise/item/amber.json", b"{}")];
        release(&fixture, &mut registry, "root_mod", "1.0.0", defs);
        install_root(&fixture, &registry);
        release(&fixture, &mut registry, "root_mod", "1.1.0", &[]);
        fixture.publish_registry(&registry);
        world(&fixture.home, "First world", &["root_mod"]);
        let old = live_state(&fixture);
        let required = fixture.home.join(mod_list::DIR).join("required_mod.smod");
        let required_bytes = fs::read(&required).unwrap();
        let mut worker = Command::new(std::env::current_exe().unwrap());
        for (key, value) in fixture.registry_client(&[]).get_envs() {
            worker.env(key, value.unwrap());
        }
        let output = worker
            .args(["--exact", "install::mutations::world_cutoff_worker"])
            .env("FIXTURE_WORLD_CUTOFF", action)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(fixture.home.join("saves/New world/level.dat").is_file());
        assert_eq!(live_state(&fixture), old);
        assert_eq!(fs::read(required).unwrap(), required_bytes);
    }
}

#[test]
fn world_cutoff_worker() {
    let Ok(action) = std::env::var("FIXTURE_WORLD_CUTOFF") else {
        return;
    };
    use sunrise_launcher::{browser::sunrise::*, instances, managed_play};
    let instance = instances::selection().unwrap();
    let identity = install_target(&instance).unwrap();
    let decide = |impact: &ChangeImpact| {
        let detail = impact.detail();
        assert!(detail.contains("1 saved world"), "{detail}");
        if action == "remove" {
            assert!(detail.contains("Lanterns (root_mod)"), "{detail}");
        }
        world(&instance.home, "New world", &["root_mod"]);
        true
    };
    let hooks = InstallHooks {
        progress: &|_| {},
        native: &|_| panic!("content-only"),
        registry_crates: None,
        continue_after_failure: &|_| panic!("content-only"),
    };
    let choose = || managed_play::headless_first(&instance, None);
    let error = if action == "remove" {
        remove_mods(
            &instance,
            identity,
            &["required_mod".into()],
            choose,
            hooks,
            &decide,
        )
        .unwrap_err()
    } else {
        update_registry(
            &instance,
            identity,
            Some(vec!["root_mod".into()]),
            choose,
            hooks,
            &decide,
        )
        .unwrap_err()
    };
    assert!(
        error
            .to_string()
            .contains("The affected worlds changed after review. Nothing was changed; try again."),
        "{error}"
    );
}

#[test]
fn remove_names_dependents_even_without_worlds_and_requires_compatible_activation() {
    let fixture = Fixture::new();
    let (registry, _) = fixture.content_index();
    install_root(&fixture, &registry);
    let (ok, text) = run(&fixture, &["mods", "remove", "required_mod"]);
    assert!(ok && text.contains("Lanterns (root_mod)"), "{text}");
    assert!(!text.contains("saved world"), "{text}");
    assert_eq!(text.matches("will be turned off").count(), 1, "{text}");
    assert!(!selection::is_enabled(&fixture.home, "root_mod"));
    let (ok, text) = run(&fixture, &["mods", "enable", "root_mod"]);
    assert!(!ok && text.contains("needs required_mod"), "{text}");
    let (ok, text) = run(
        &fixture,
        &["mods", "install", "required_mod", "--source", "sunrise"],
    );
    assert!(ok, "{text}");
    assert!(!selection::is_enabled(&fixture.home, "root_mod"));
    let (ok, text) = run(&fixture, &["mods", "enable", "root_mod"]);
    assert!(ok && !text.contains("saved world"), "{text}");
    assert!(selection::is_enabled(&fixture.home, "root_mod"));
}

#[test]
fn update_all_asks_once_only_for_destructive_or_unproved_definitions_and_preserves_cancel() {
    let fixture = Fixture::new();
    let (mut registry, _) = fixture.content_index();
    let defs: &[(&str, &[u8])] = &[("data/root_mod/sunrise/item/amber.json", b"{}")];
    let first = release(&fixture, &mut registry, "root_mod", "1.0.0", defs);
    let second_def: &[(&str, &[u8])] = &[("data/required_mod/sunrise/item/glowstone.json", b"{}")];
    release(&fixture, &mut registry, "required_mod", "1.0.0", second_def);
    install_root(&fixture, &registry);
    let path = fixture.home.join(mod_list::DIR).join("root_mod.smod");
    assert_eq!(fs::read(&path).unwrap(), first);
    world(&fixture.home, "Uses Lantern", &["root_mod"]);
    world(&fixture.home, "Unrelated", &["something_else"]);
    let retained = release(&fixture, &mut registry, "root_mod", "1.1.0", defs);
    fixture.publish_registry(&registry);
    let (ok, text) = run(&fixture, &["mods", "update", "root_mod"]);
    assert!(ok, "{text}");
    assert!(!text.contains("saved world"), "{text}");
    assert_eq!(fs::read(&path).unwrap(), retained);

    let lost = release(&fixture, &mut registry, "root_mod", "1.2.0", &[]);
    let required = release(&fixture, &mut registry, "required_mod", "1.1.0", &[]);
    fixture.publish_registry(&registry);
    let old = live_state(&fixture);
    let (ok, text) = run(&fixture, &["mods", "update", "root_mod"]);
    assert!(
        !ok && text.contains("1 saved world") && text.contains("--confirm-world-loss"),
        "{text}"
    );
    assert_eq!(live_state(&fixture), old);

    world(&fixture.home, "Uses Both", &["required_mod", "root_mod"]);
    let unreadable = fixture.home.join("saves/Unreadable");
    fs::create_dir(&unreadable).unwrap();
    fs::write(unreadable.join("level.dat"), b"invalid data").unwrap();
    let (ok, text) = run(&fixture, &["mods", "update"]);
    assert!(
        !ok && text.contains("3 saved worlds") && text.contains("couldn't check"),
        "{text}"
    );
    assert_eq!(
        text.matches("may remove modded content").count(),
        1,
        "{text}"
    );
    assert_eq!(live_state(&fixture), old);

    let mut mcp = Running(
        fixture
            .registry_client(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = mcp.0.stdin.take().unwrap();
    let mut output = BufReader::new(mcp.0.stdout.take().unwrap());
    let mut rpc = |id, method, params| mcp_request(&mut input, &mut output, id, method, params);
    rpc(1, "initialize", json!({}));
    let denied = rpc(
        2,
        "tools/call",
        json!({"name":"mods_update", "arguments":{}}),
    );
    assert_eq!(denied["isError"], true, "{denied}");
    assert!(denied.to_string().contains("3 saved worlds"), "{denied}");
    assert_eq!(live_state(&fixture), old);
    let accepted = rpc(
        3,
        "tools/call",
        json!({"name":"mods_update", "arguments":{"confirm_world_loss":true}}),
    );
    assert_ne!(accepted["isError"], true, "{accepted}");
    assert_eq!(fs::read(&path).unwrap(), lost);
    assert_eq!(
        fs::read(fixture.home.join(mod_list::DIR).join("required_mod.smod")).unwrap(),
        required
    );
    assert_ne!(fixture.pointer(), old[0]);
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
fn remove_refuses_running_game_and_disables_exact_dependents_in_place_in_one_commit() {
    let fixture = Fixture::new();
    let (registry, archives) = fixture.content_index();
    install_root(&fixture, &registry);
    let mut play = Running(fixture.registry_client(&["play"]).spawn().unwrap());
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
            .registry_client(&["mcp"])
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
    let mut play = Running(fixture.registry_client(&["play"]).spawn().unwrap());
    assert_resource_args(&fixture, &fixture.game_arguments());
    assert_eq!(play.0.wait().unwrap().code(), Some(7));
    assert_eq!(fixture.pointer(), reduced);
}
