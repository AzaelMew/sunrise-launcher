use super::*;
use std::io::Write;
use sunrise_launcher_nbt::{self as nbt, Limits, Tag};

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sunrise-launcher-datapacks-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn zip_file(path: &Path, files: &[(&str, &str)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, text) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

const META: &str = r#"{"pack":{"pack_format":88,"description":"More trades"}}"#;

#[test]
fn lists_adds_and_removes_as_the_game_reads_them() {
    let root = scratch("all");
    let world = root.join("saves/My World");
    zip_file(
        &world.join("datapacks/Trades.zip"),
        &[
            ("pack.mcmeta", META),
            ("data/t/function/a.mcfunction", "say hi"),
        ],
    );
    fs::create_dir_all(world.join("datapacks/folder pack")).unwrap();
    fs::write(
        world.join("datapacks/folder pack/pack.mcmeta"),
        r#"{"pack":{"description":{"text":"Mine"}}}"#,
    )
    .unwrap();
    fs::write(world.join("datapacks/readme.txt"), "").unwrap();
    fs::write(
        world.join("datapacks.json"),
        r#"{"enabled":[],"disabled":["file/folder pack","file/Gone.zip"]}"#,
    )
    .unwrap();
    let packs = list(&world);
    let seen: Vec<(&str, &str, bool)> = packs
        .iter()
        .map(|p| (p.file.as_str(), p.description.as_str(), p.enabled))
        .collect();
    assert_eq!(
        seen,
        [
            ("folder pack", "Mine", false),
            ("Trades.zip", "More trades", true)
        ]
    );

    let drops = root.join("drops");
    zip_file(
        &drops.join("Gone.zip"),
        &[("pack.mcmeta", META), ("data/g/tags/x.json", "{}")],
    );
    zip_file(
        &drops.join("Trades.zip"),
        &[("pack.mcmeta", META), ("data/t/function/b.mcfunction", "")],
    );
    zip_file(
        &drops.join("Textures.zip"),
        &[
            ("pack.mcmeta", META),
            ("assets/minecraft/textures/a.png", ""),
        ],
    );
    // A pack disabled before it was taken out comes back on.
    let gone = add(&world, &drops.join("Gone.zip")).unwrap();
    assert!(gone.enabled);
    assert!(!fs::read_to_string(world.join("datapacks.json"))
        .unwrap()
        .contains("Gone.zip"));
    assert_eq!(
        add(&world, &drops.join("Trades.zip")).unwrap().file,
        "Trades (2).zip"
    );
    let refused = add(&world, &drops.join("Textures.zip")).unwrap_err();
    assert!(refused.contains("resource pack"), "{refused}");

    let folder = find(&world, "folder pack").unwrap();
    let trash = root.join("trash");
    remove(&world, &folder, Some(&trash)).unwrap();
    assert!(trash.join("folder pack/pack.mcmeta").is_file());
    assert!(!fs::read_to_string(world.join("datapacks.json"))
        .unwrap()
        .contains("folder pack"));
    assert_eq!(find(&world, "Trades").unwrap().file, "Trades.zip");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn coverage_says_what_runs() {
    let root = scratch("coverage");
    let file = root.join("p.zip");
    zip_file(
        &file,
        &[
            ("pack.mcmeta", META),
            ("data/a/function/x.mcfunction", ""),
            ("data/a/predicate/p.json", "{}"),
            ("data/a/item_modifier/m.json", "{}"),
            ("data/a/tags/item/i.json", "{}"),
            ("data/a/loot_table/l.json", "{}"),
            ("data/a/loot_table/k.json", "{}"),
            ("data/a/worldgen/biome/b.json", "{}"),
            ("data/a/worldgen/world_preset/w.json", "{}"),
            ("assets/a/lang/en_us.json", "{}"),
        ],
    );
    let c = coverage(&file).unwrap();
    assert_eq!(
        c.summary(),
        "Runs its 1 function, 1 item tag, 1 item modifier, 2 loot tables, 1 predicate, 1 \
         worldgen. Not read yet: 1 worldgen world preset."
    );
    let many = Coverage {
        read: Vec::new(),
        unread: (1..=8).map(|n| (format!("kind{n}"), n)).collect(),
    };
    assert_eq!(
        many.not_read(),
        "8 kind8s, 7 kind7s, 6 kind6s, 5 kind5s, 4 kind4s, 3 kind3s and 2 other kinds"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_data_pack_mcmeta_with_a_byte_order_mark_or_comments_is_listed() {
    let root = scratch("lenient-meta");
    let packs = root.join("datapacks");
    zip_file(
        &packs.join("Bom.zip"),
        &[("pack.mcmeta", "\u{feff}{\"pack\":{\"description\":\"B\"}}")],
    );
    fs::create_dir_all(packs.join("Note")).unwrap();
    fs::write(
        packs.join("Note/pack.mcmeta"),
        "{\"pack\":{\"description\":\"N\"} // trailing note\n}",
    )
    .unwrap();
    let seen: Vec<(String, String)> = list(&root)
        .into_iter()
        .map(|p| (p.file, p.description))
        .collect();
    assert_eq!(
        seen,
        [
            ("Bom.zip".to_string(), "B".to_string()),
            ("Note".to_string(), "N".to_string())
        ]
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_data_pack_without_a_description_is_not_listed() {
    let root = scratch("no-description");
    let packs = root.join("datapacks");
    zip_file(&packs.join("Bare.zip"), &[("pack.mcmeta", "{}")]);
    fs::create_dir_all(packs.join("NoText")).unwrap();
    fs::write(
        packs.join("NoText/pack.mcmeta"),
        r#"{"pack":{"pack_format":88}}"#,
    )
    .unwrap();
    fs::create_dir_all(packs.join("Fine")).unwrap();
    fs::write(packs.join("Fine/pack.mcmeta"), META).unwrap();
    let names: Vec<String> = list(&root).into_iter().map(|p| p.file).collect();
    assert_eq!(names, ["Fine"]);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_data_pack_without_a_description_is_refused_before_it_is_copied() {
    let root = scratch("no-description-add");
    let world = root.join("saves/W");
    fs::create_dir_all(&world).unwrap();
    let drops = root.join("drops");
    for (name, meta) in [("Bare.zip", r#"{"pack":{}}"#), ("Empty.zip", "{}")] {
        zip_file(
            &drops.join(name),
            &[("pack.mcmeta", meta), ("data/t/a.json", "{}")],
        );
        assert!(add(&world, &drops.join(name)).is_err());
    }
    let left = fs::read_dir(world.join("datapacks")).map_or(0, Iterator::count);
    assert_eq!(left, 0);
    zip_file(
        &drops.join("Fine.zip"),
        &[("pack.mcmeta", META), ("data/t/a.json", "{}")],
    );
    assert_eq!(
        add(&world, &drops.join("Fine.zip")).unwrap().file,
        "Fine.zip"
    );
    fs::remove_dir_all(&root).unwrap();
}

fn strings(ids: &[&str]) -> Tag {
    Tag::List(ids.iter().map(|id| Tag::String((*id).into())).collect())
}

/// A world with a gzip level.dat as the game writes it: `DataPacks` (when given) among other tags.
fn level_world(root: &Path, packs: Option<Tag>) -> PathBuf {
    let world = root.join("saves/W");
    fs::create_dir_all(&world).unwrap();
    let mut data = vec![
        ("LevelName", Tag::String("W".into())),
        ("version", Tag::Int(19133)),
        ("LastPlayed", Tag::Long(1_700_000_000_000)),
        ("Difficulty", Tag::Byte(2)),
        (
            "WorldGenSettings",
            Tag::compound([("seed", Tag::Long(-42)), ("bonus", Tag::Byte(1))]),
        ),
        (
            "Mods",
            Tag::List(vec![Tag::compound([("id", Tag::String("m".into()))])]),
        ),
    ];
    data.extend(packs.map(|p| ("DataPacks", p)));
    let bytes = nbt::write_named(
        &nbt::NamedTag {
            name: "".into(),
            tag: Tag::compound([("Data", Tag::compound(data))]),
        },
        Limits::default(),
    )
    .unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&bytes).unwrap();
    fs::write(world.join("level.dat"), gz.finish().unwrap()).unwrap();
    world
}

fn selection(enabled: &[&str], disabled: &[&str]) -> Tag {
    Tag::compound([
        ("Enabled", strings(enabled)),
        ("Disabled", strings(disabled)),
    ])
}

fn decoded(world: &Path, file: &str) -> Tag {
    crate::worlds::level_dat(&world.join(file)).unwrap()
}

fn pack(world: &Path, name: &str) {
    zip_file(
        &world.join("datapacks").join(name),
        &[("pack.mcmeta", META), ("data/t/function/a.mcfunction", "")],
    );
}

#[test]
fn a_pack_level_dat_lists_as_disabled_is_off_with_no_json() {
    let root = scratch("level-dat-off");
    let world = level_world(
        &root,
        Some(selection(&["vanilla", "file/b.zip"], &["file/a.zip"])),
    );
    pack(&world, "a.zip");
    pack(&world, "b.zip");
    let seen: Vec<(String, bool)> = list(&world)
        .into_iter()
        .map(|p| (p.file, p.enabled))
        .collect();
    assert_eq!(
        seen,
        [("a.zip".to_string(), false), ("b.zip".to_string(), true)]
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_world_level_dat_has_no_data_packs_for_has_every_pack_on() {
    let root = scratch("level-dat-none");
    let world = level_world(&root, None);
    pack(&world, "a.zip");
    assert!(list(&world)[0].enabled);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn datapacks_json_wins_over_level_dat_while_it_exists() {
    let root = scratch("json-wins");
    let world = level_world(&root, Some(selection(&["file/b.zip"], &["file/a.zip"])));
    pack(&world, "a.zip");
    pack(&world, "b.zip");
    fs::write(
        world.join("datapacks.json"),
        r#"{"enabled":[],"disabled":["file/b.zip"]}"#,
    )
    .unwrap();
    let seen: Vec<(String, bool)> = list(&world)
        .into_iter()
        .map(|p| (p.file, p.enabled))
        .collect();
    assert_eq!(
        seen,
        [("a.zip".to_string(), true), ("b.zip".to_string(), false)]
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn removing_and_adding_a_disabled_pack_drops_it_from_level_dat_and_changes_no_other_tag() {
    let root = scratch("forget-level-dat");
    let world = level_world(
        &root,
        Some(selection(
            &["vanilla", "file/b.zip", "file/a.zip"],
            &["file/a.zip", "file/c.zip"],
        )),
    );
    let before = decoded(&world, "level.dat");
    pack(&world, "a.zip");
    pack(&world, "b.zip");
    let a = find(&world, "a.zip").unwrap();
    assert!(!a.enabled);

    let trash = root.join("trash");
    let removed = remove(&world, &a, Some(&trash));
    removed.unwrap();

    // Expected: the same tags, with only `file/a.zip` gone from the two lists.
    let expected = |enabled: &[&str], disabled: &[&str]| {
        let mut root = before.clone();
        let Some(Tag::Compound(data)) = root.get_mut("Data") else {
            panic!("no Data")
        };
        data.insert("DataPacks".into(), selection(enabled, disabled));
        root
    };
    assert_eq!(
        decoded(&world, "level.dat"),
        expected(&["vanilla", "file/b.zip"], &["file/c.zip"])
    );
    // level.dat_old is what level.dat was.
    assert_eq!(decoded(&world, "level.dat_old"), before);

    let drops = root.join("drops");
    pack(&drops, "a.zip");
    let again = add(&world, &drops.join("datapacks/a.zip")).unwrap();
    assert_eq!((again.file.as_str(), again.enabled), ("a.zip", true));
    assert_eq!(
        decoded(&world, "level.dat"),
        expected(&["vanilla", "file/b.zip"], &["file/c.zip"])
    );
    assert!(!world.join("datapacks.json").exists());
    let temp_left: Vec<_> = fs::read_dir(&world)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("level") && n != "level.dat" && n != "level.dat_old")
        .collect();
    assert!(temp_left.is_empty(), "{temp_left:?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_json_that_exists_is_updated_beside_level_dat_and_none_is_created() {
    let root = scratch("forget-both");
    let world = level_world(&root, Some(selection(&[], &["file/a.zip"])));
    let drops = root.join("drops");
    pack(&drops, "a.zip");
    let none = add(&world, &drops.join("datapacks/a.zip")).unwrap();
    assert!(none.enabled);
    assert!(!world.join("datapacks.json").exists());

    fs::write(
        world.join("datapacks.json"),
        r#"{"enabled":["file/a.zip"],"disabled":["file/a.zip","file/x.zip"]}"#,
    )
    .unwrap();
    let again = add(&world, &drops.join("datapacks/a.zip")).unwrap();
    assert_eq!(again.file, "a (2).zip");
    forget(&world, "a (2).zip").unwrap();
    forget(&world, "a.zip").unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(world.join("datapacks.json")).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"enabled":[],"disabled":["file/x.zip"]})
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_world_the_game_holds_is_refused_and_level_dat_is_left_as_it_is() {
    let root = scratch("locked");
    let world = level_world(&root, Some(selection(&[], &["file/a.zip"])));
    pack(&world, "a.zip");
    let drops = root.join("drops");
    pack(&drops, "b.zip");
    let before = fs::read(world.join("level.dat")).unwrap();
    let a = find(&world, "a.zip").unwrap();
    let game = crate::worlds::lock_saved_dir(&world).unwrap();
    assert!(add(&world, &drops.join("datapacks/b.zip")).is_err());
    // Refused before anything moves, and never into the system's trash either way.
    assert!(remove(&world, &a, Some(&root.join("trash"))).is_err());
    assert!(a.path.is_file());
    drop(game);
    assert_eq!(fs::read(world.join("level.dat")).unwrap(), before);
    assert!(!world.join("level.dat_old").exists());
    assert!(!world.join("datapacks.json").exists());
    fs::remove_dir_all(&root).unwrap();
}
