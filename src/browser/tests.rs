use super::*;
use serde_json::json;
use sha2::Digest;
const GAME: &str = crate::backend::PINNED_GAME;

fn v(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn version(id: &str, games: &[&str], loaders: &[&str]) -> Version {
    Version {
        id: id.into(),
        number: id.into(),
        published: String::new(),
        game_versions: v(games),
        loaders: v(loaders),
        files: vec![FileRef {
            url: format!("https://cdn.modrinth.com/data/x/versions/{id}/{id}.zip"),
            name: format!("{id}.zip"),
            size: 3,
            sha512: Some("ab".into()),
            sha256: None,
            sha1: None,
            primary: true,
        }],
        required: Vec::new(),
        blocked: None,
    }
}

#[test]
fn support_by_kind() {
    assert_eq!(
        support(
            Kind::ResourcePack,
            &v(&["1.21.4", GAME]),
            &v(&["minecraft"]),
            GAME
        ),
        Support::Works
    );
    let old = support(
        Kind::ResourcePack,
        &v(&["1.20.1", "1.21.10", "1.21.9", "26.4-snapshot-1"]),
        &[],
        GAME,
    );
    assert_eq!(old.label(), "Partly");
    assert!(
        old.reason().unwrap().starts_with("Made for 1.21.10:"),
        "{old:?}"
    );
}

#[test]
fn picks_the_newest_version_that_runs() {
    let versions = [
        version("snapshot", &["26.4-snapshot-5"], &["minecraft"]),
        version("now", &["26.2", GAME], &["minecraft"]),
        version("old", &["1.21.1"], &["minecraft"]),
    ];
    assert_eq!(
        pick(Kind::ResourcePack, &versions, false, GAME).unwrap().id,
        "now"
    );
    let old = [version("old", &["1.21.1"], &["minecraft"])];
    assert!(pick(Kind::ResourcePack, &old, false, GAME).is_none());
    assert_eq!(
        pick(Kind::ResourcePack, &old, true, GAME).unwrap().id,
        "old"
    );

    let mut blocked = version("blocked", &[GAME], &["minecraft"]);
    blocked.blocked = Some("no".into());
    let mut empty = version("empty", &[GAME], &["minecraft"]);
    empty.files.clear();
    assert!(pick(Kind::ResourcePack, &[blocked, empty], true, GAME).is_none());
}

#[test]
fn iris_format_shader_packs_are_labelled_and_only_their_zip_is_picked() {
    let hit = json!({"project_id": "s", "project_type": "shader", "slug": "bsl",
        "versions": [GAME], "categories": ["iris", "optifine", "vanilla"]});
    let p = modrinth::parse_hit(&hit).unwrap();
    assert_eq!((p.kind, p.source), (Kind::Shader, SourceId::Modrinth));
    assert_eq!(
        p.support(GAME),
        Support::Partial("Iris-format pack: most work, some effects may not".into())
    );
    // Another loader's pack doesn't run; Sunrise's own are Works on their build, unchanged.
    assert!(!support(Kind::Shader, &v(&[GAME]), &v(&["optifine"]), GAME).installable());
    assert_eq!(
        support(Kind::Shader, &v(&[GAME]), &v(&[sunrise::LOADER]), GAME),
        Support::Works
    );

    let mut jar = version("jar", &[GAME], &["iris"]);
    jar.files[0].name = "pack.jar".into();
    let versions = [
        jar,
        version("other", &[GAME], &["optifine"]),
        version("old", &["1.21.4"], &["iris"]),
        version("pack", &[GAME], &["iris"]),
    ];
    assert_eq!(
        pick(Kind::Shader, &versions, false, GAME).unwrap().id,
        "pack"
    );
    assert!(pick(Kind::Shader, &versions[..3], false, GAME).is_none());
    assert_eq!(
        pick(Kind::Shader, &versions[..3], true, GAME).unwrap().id,
        "old"
    );

    let facets = modrinth::facets(&Query::new("", Kind::Shader)).unwrap();
    assert!(facets.contains("project_type:shader"), "{facets}");
    assert!(facets.contains("categories:iris"), "{facets}");
    assert!(facets.contains(&format!("versions:{}", Query::new("", Kind::Shader).game)));
}

#[test]
fn data_packs_are_their_own_kind() {
    // Most data packs on Modrinth also ship a mod build; only the data pack files are picked.
    let versions = [
        version("fabric", &[GAME], &["fabric", "neoforge"]),
        version("pack", &[GAME], &["datapack"]),
        version("old", &["1.21.4"], &["datapack"]),
    ];
    assert_eq!(
        pick(Kind::DataPack, &versions, false, GAME).unwrap().id,
        "pack"
    );
    assert!(pick(Kind::DataPack, &versions[2..], false, GAME).is_none());
    assert_eq!(
        pick(Kind::DataPack, &versions[2..], true, GAME).unwrap().id,
        "old"
    );
    assert!(pick(Kind::DataPack, &versions[..1], true, GAME).is_none());

    let now = support(Kind::DataPack, &v(&[GAME]), &v(&["datapack"]), GAME);
    assert_eq!(now.reason(), Some(datapack_support()));
    let old = support(Kind::DataPack, &v(&["1.21.4"]), &v(&["datapack"]), GAME);
    assert!(
        old.reason().unwrap().starts_with("Made for 1.21.4"),
        "{old:?}"
    );
    assert!(old.installable());

    assert_eq!(
        modrinth::facets(&Query::new("", Kind::DataPack)).as_deref(),
        Some(r#"[["project_type:datapack"],["versions:26.3"]]"#)
    );
    // A data pack's hit says `mod`: the search for data packs marks it.
    let hit = serde_json::json!({
        "project_id": "bVrj3hwG", "project_type": "mod", "slug": "terralith",
        "title": "Terralith", "categories": ["datapack", "fabric", "worldgen"],
        "versions": [GAME],
    });
    let p = modrinth::parse_hit(&hit).unwrap();
    assert_eq!(p.kind, Kind::Mod);
    let p = p.as_datapack();
    assert_eq!(
        (p.kind, p.page.as_str()),
        (Kind::DataPack, "https://modrinth.com/datapack/terralith")
    );
    assert_eq!(Kind::DataPack.folder(), None);
}

#[test]
fn file_names_must_be_plain() {
    for good in [
        "Faithful 32x.zip",
        "FreshAnimations_v1.11.1_26.1+.zip",
        "Faithful_v10.zip",
        "Console.zip",
        "COM10.jar",
        "nullify.jar",
        "Ünïcode pack.zip",
    ] {
        assert_eq!(safe_name(good), Some(good));
    }
    for bad in [
        "",
        "../x.zip",
        "a/b.zip",
        "a\\b.zip",
        ".hidden.zip",
        "c:x.zip",
        "x.zip.",
        "x\n.zip",
        "CON",
        "nul.jar",
        "Com1.zip",
        "LPT9",
        "AUX .zip",
        "COM¹.jar",
        "fake\u{202E}gpj.zip",
        "a\u{200B}b.zip",
    ] {
        assert_eq!(safe_name(bad), None, "{bad:?}");
    }
}

#[test]
fn modrinth_file_name_with_a_colon_is_saved_under_a_plain_name() {
    let version = parse_version_for_test("Yet Another Bingo: Ultimate-2.15.1.mrpack");
    let name = &version.files[0].name;
    assert_eq!(name, "Yet Another Bingo- Ultimate-2.15.1.mrpack");
    assert_eq!(safe_name(name), Some(name.as_str()));
    let hostile = parse_version_for_test("../a:b.zip");
    assert_eq!(hostile.files[0].name, "../a-b.zip");
    assert_eq!(safe_name(&hostile.files[0].name), None);
}

fn parse_version_for_test(filename: &str) -> Version {
    let json = json!({
        "id": "v1", "version_number": "1", "date_published": "2026-01-01T00:00:00Z",
        "files": [{"url": "https://cdn.modrinth.com/data/a/x.zip", "filename": filename,
                   "size": 1, "primary": true, "hashes": {"sha512": "ab"}}],
    });
    super::modrinth::parse_version(&json).expect("a version with one file")
}

#[test]
fn modrinth_search_asks_for_what_runs() {
    let q = Query::new("  faithful ", Kind::ResourcePack);
    assert_eq!(q.text, "faithful");
    assert_eq!(
        modrinth::facets(&q).as_deref(),
        Some(r#"[["project_type:resourcepack"],["versions:26.3"]]"#)
    );
    let url = modrinth::search_url(&q).unwrap();
    assert!(url.starts_with("https://api.modrinth.com/v2/search?facets=%5B%5B%22project_type"));
    assert!(
        url.contains("&index=relevance&") && url.ends_with("&query=faithful"),
        "{url}"
    );
    assert!(modrinth::search_url(&Query::new("", Kind::ResourcePack))
        .unwrap()
        .contains("&index=downloads&"));
    let everything = Query {
        everything: true,
        ..Query::new("", Kind::ResourcePack)
    };
    assert_eq!(
        modrinth::facets(&everything).as_deref(),
        Some(r#"[["project_type:resourcepack"]]"#)
    );
    // Modrinth is searched for packs only: never for Java mods or modpacks.
    assert_eq!(modrinth::facets(&Query::new("", Kind::Modpack)), None);
    assert_eq!(modrinth::search_url(&Query::new("", Kind::Mod)), None);
    assert!(modrinth::Modrinth
        .search(&Query::new("sodium", Kind::Mod))
        .is_err());
}

#[test]
fn java_mods_and_modpacks_do_not_run() {
    let versions = [
        version("now", &[GAME], &["neoforge"]),
        version("pack", &[GAME], &["fabric"]),
    ];
    for kind in [Kind::Mod, Kind::Modpack] {
        assert!(!support(kind, &v(&[GAME]), &v(&["neoforge"]), GAME).installable());
        assert!(pick(kind, &versions, true, GAME).is_none());
    }
    // Made for Sunrise: the registry's own mods and modpacks run.
    assert_eq!(
        support(Kind::Mod, &v(&[GAME]), &v(&[sunrise::LOADER]), GAME),
        Support::Works
    );
}

#[test]
fn modrinth_answers() {
    let hit = json!({
        "project_id": "50dA9Sha", "project_type": "resourcepack", "slug": "fresh-animations",
        "title": "Fresh Animations", "description": "Animated entities.", "author": "FreshLX",
        "downloads": 51404735, "icon_url": "https://cdn.modrinth.com/data/50dA9Sha/icon.webp",
        "license": "LicenseRef-See-Terms", "versions": ["1.21.11", "26.3"],
        "categories": ["16x", "minecraft"]
    });
    let p = modrinth::parse_hit(&hit).unwrap();
    assert_eq!(p.id, "50dA9Sha");
    assert_eq!(p.kind, Kind::ResourcePack);
    assert_eq!(p.page, "https://modrinth.com/resourcepack/fresh-animations");
    assert_eq!(p.support(GAME), Support::Works);
    assert_eq!(p.downloads, 51404735);
    // A Java mod is not one Sunrise runs.
    let jar = json!({"project_id": "AANobbMI", "project_type": "mod", "slug": "sodium",
        "versions": ["26.3"], "categories": ["fabric", "optimization"]});
    let sodium = modrinth::parse_hit(&jar).unwrap();
    assert_eq!(sodium.kind, Kind::Mod);
    assert_eq!(sodium.page, "https://modrinth.com/mod/sodium");
    assert!(!sodium.support(GAME).installable());
    // Plugins are not.
    let plugin = json!({"project_id": "x", "project_type": "plugin", "slug": "p"});
    assert!(modrinth::parse_hit(&plugin).is_none());

    let project = json!({
        "id": "HVnmMxH1", "project_type": "resourcepack", "slug": "fresh-animations",
        "title": "Fresh Animations", "description": "d",
        "license": {"id": "LicenseRef-Custom", "name": ""},
        "game_versions": ["26.3"], "loaders": ["minecraft"]
    });
    let p = modrinth::parse_project(&project).unwrap();
    assert_eq!(p.kind, Kind::ResourcePack);
    assert_eq!(p.license, "LicenseRef-Custom");
    assert_eq!(p.page, "https://modrinth.com/resourcepack/fresh-animations");
    assert!(p.support(GAME).installable());

    let version = json!({
        "id": "tvGVYMD4", "version_number": "1.11.1", "date_published": "2026-10-20T02:23:13Z",
        "game_versions": ["26.2", "26.3"], "loaders": ["minecraft"],
        "dependencies": [
            {"project_id": "need", "dependency_type": "required"},
            {"project_id": "maybe", "dependency_type": "optional"},
            {"version_id": "x", "project_id": null, "dependency_type": "required"}
        ],
        "files": [
            {"hashes": {"sha512": "AB12", "sha1": "cd34"}, "url": "https://cdn.modrinth.com/data/a/b.zip",
             "filename": "b.zip", "primary": false, "size": 10},
            {"hashes": {"sha512": "ef56", "sha1": "0789"}, "url": "https://cdn.modrinth.com/data/a/c.zip",
             "filename": "c.zip", "primary": true, "size": 20}
        ]
    });
    let ver = modrinth::parse_version(&version).unwrap();
    assert_eq!(ver.required, ["need"]);
    assert_eq!(ver.files[0].sha512.as_deref(), Some("ab12"));
    let file = ver.file().unwrap();
    assert_eq!(file.name, "c.zip");
    assert_eq!(file.checksum(), Some(("SHA-512", "ef56")));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sunrise-browser-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn file_for(body: &[u8], sha512: bool) -> FileRef {
    FileRef {
        url: "https://cdn.modrinth.com/data/a/versions/b/pack.zip".into(),
        name: "pack.zip".into(),
        size: body.len() as u64,
        sha512: sha512.then(|| hex(&sha2::Sha512::digest(body))),
        sha256: None,
        sha1: (!sha512).then(|| hex(&sha1::Sha1::digest(body))),
        primary: true,
    }
}

#[test]
fn a_download_is_kept_only_when_its_checksum_matches() {
    let dir = scratch("save");
    let body = b"a resource pack";
    for strong in [true, false] {
        let file = file_for(body, strong);
        let path = download::save(&file, &body[..], &dir, &|_| {}).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), body);
        std::fs::remove_file(path).unwrap();
    }
    // Another file than the one listed: refused, and nothing is left behind.
    let file = file_for(body, true);
    let other = b"a resource paCk";
    let e = download::save(&file, &other[..], &dir, &|_| {}).unwrap_err();
    assert!(e.contains("SHA-512 checksum doesn't match"), "{e}");
    let short = download::save(&file, &body[..4], &dir, &|_| {}).unwrap_err();
    assert!(short.starts_with("Got 4 bytes"), "{short}");
    let long = download::save(&file, &b"a resource pack and more"[..], &dir, &|_| {}).unwrap_err();
    assert!(long.contains("larger"), "{long}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn downloads_refused_before_any_byte() {
    let body = b"x";
    let mut file = file_for(body, true);
    file.url = "https://example.com/pack.zip".into();
    assert!(download::refuse(&file).unwrap().contains("file servers"));
    // A mod's own repository's release asset, which the registry's index lists with its SHA-256.
    let mut file = file_for(body, true);
    file.url = "https://github.com/acme/zoom/releases/download/v1.0.0/zoom-1.0.0.smod".into();
    assert_eq!(download::refuse(&file), None);
    for bad in [
        "http://github.com/acme/zoom/releases/download/v1/z.smod",
        "https://github.com/acme/zoom/archive/main.zip",
        "https://github.com/acme/zoom/releases/download/v1/../z.smod",
        "https://github.com/acme/zoom/releases/download/v1/z.smod?x=1",
        "https://github.com.evil.example/a/b/releases/download/v1/z.smod",
        "https://github.com/acme/zoom/releases/download/v1/a/z.smod",
    ] {
        let mut file = file_for(body, true);
        file.url = bad.into();
        assert!(download::refuse(&file).is_some(), "{bad}");
    }
    let mut file = file_for(body, true);
    file.url = "http://cdn.modrinth.com/data/a/pack.zip".into();
    assert!(download::refuse(&file).is_some());
    let mut file = file_for(body, true);
    file.name = "../../options.txt".into();
    assert!(download::refuse(&file).unwrap().contains("plain file name"));
    let mut file = file_for(body, true);
    file.sha512 = None;
    assert!(download::refuse(&file).unwrap().contains("no checksum"));
    let mut file = file_for(body, false);
    file.url = "https://edge.forgecdn.net/files/1/2/pack.zip".into();
    assert!(download::refuse(&file).unwrap().contains("file servers"));
}

/// Against the real Modrinth: `cargo test --lib browser::tests::live -- --ignored`.
#[test]
#[ignore = "needs the network"]
fn live_modrinth_search_pick_and_download() {
    let source = modrinth::Modrinth;
    for kind in Kind::ALL {
        let found = source.search(&Query::new("", kind)).unwrap();
        assert!(!found.is_empty(), "{kind:?}");
        assert!(found
            .iter()
            .all(|p| p.kind == kind && p.support(GAME).installable()));
    }
    let packs = source
        .search(&Query::new("fresh animations", Kind::ResourcePack))
        .unwrap();
    let p = packs.iter().find(|p| p.slug == "fresh-animations").unwrap();
    assert_eq!(p.support(GAME), Support::Works);
    let versions = source.versions(p).unwrap();
    let chosen = pick(p.kind, &versions, false, GAME).unwrap();
    assert!(chosen.game_versions.iter().any(|g| g == GAME));
    let dir = scratch("live");
    let path = download::fetch(chosen.file().unwrap(), &dir, &|_| {}).unwrap();
    assert!(path.is_file());
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(source.project(&p.id).unwrap().slug, p.slug);
}

#[test]
fn a_projects_own_licence_reads_as_words() {
    assert_eq!(
        license_label("LicenseRef-Polyform-Shield-1.0.0"),
        "Polyform Shield 1.0.0"
    );
    assert_eq!(
        license_label("LicenseRef-All-Rights-Reserved"),
        "All Rights Reserved"
    );
    // An SPDX id is what people search licences by: it stays as it is.
    assert_eq!(license_label("LGPL-3.0-only"), "LGPL-3.0-only");
    assert_eq!(license_label("LicenseRef-"), "LicenseRef-");
}
