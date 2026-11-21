//! Real CLI and MCP Update with the shared world-loss decision.
use super::*;
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
