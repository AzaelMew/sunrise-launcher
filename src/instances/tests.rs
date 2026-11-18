use super::*;
use std::collections::BTreeMap;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sunrise-launcher-instances-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Every file under `dir`, relative, with its bytes.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(p.strip_prefix(root).unwrap().into(), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A game home as a player has it before instances: worlds, options, packs, a session, an
/// imported modpack and a registry modpack with their own worlds.
fn player_home(home: &Path) {
    put(&home.join("saves/World/level.dat"), "world");
    put(&home.join("saves/World/region/r.0.0.mca"), "region");
    put(&home.join("saves/Other/level.dat"), "other");
    put(&home.join("options.txt"), "fov:90\n");
    put(&home.join("sunrise-player.txt"), "name:Player123\n");
    put(&home.join("resourcepacks/faithful.zip"), "pack");
    put(&home.join("logs/latest.log"), "log");
    put(&home.join("sessions/a.json"), "token");
    put(&home.join("telemetry/consent.json"), "{}");
    put(
        &home.join("ports/small-pack/launcher-status.json"),
        r#"{"format": 1, "pack": {"name": "Small Pack"}}"#,
    );
    put(
        &home.join("ports/small-pack/home/saves/Modded/level.dat"),
        "modded",
    );
    put(
        &home.join("sunrise-packs/cozy/pack.json"),
        r#"{"id":"cozy","name":"Cozy","version":"1.0.0","sha256":"00","mods":[]}"#,
    );
    put(
        &home.join("sunrise-packs/cozy/home/saves/Cozy/level.dat"),
        "cozy",
    );
}

#[test]
fn build_choices_start_balanced_and_are_remembered_per_instance() {
    use crate::native_mods::Profile;
    let root = scratch("native-build-choices");
    let game = root.join("game");
    let data = root.join("launcher");
    for id in ["first", "second"] {
        fs::create_dir_all(game.join("instances").join(id)).unwrap();
    }
    for instance in list_in(&game, &data) {
        assert_eq!(instance.native_profile, Profile::Optimized);
    }
    let choose = |id, profile| {
        edit_in(&game, &data, id, |meta| meta.native_profile = Some(profile)).unwrap()
    };
    choose("first", Profile::Optimized);
    choose("second", Profile::Optimized);
    let before = fs::read(data.join("instances.json")).unwrap();
    for unknown in [serde_json::json!("future"), serde_json::json!(42)] {
        let mut stored: serde_json::Value = serde_json::from_slice(&before).unwrap();
        stored["instances"]["first"]["native_profile"] = unknown;
        stored["instances"]["first"]["version"] = serde_json::json!("kept-version");
        fs::write(
            data.join("instances.json"),
            serde_json::to_vec(&stored).unwrap(),
        )
        .unwrap();
        edit_in(&game, &data, "second", |meta| {
            meta.name = Some("Still here".into())
        })
        .unwrap();
        let loaded = list_in(&game, &data);
        assert_eq!(
            find_in(&loaded, "first").unwrap().native_profile,
            Profile::Optimized
        );
        assert_eq!(
            find_in(&loaded, "first").unwrap().version.as_deref(),
            Some("kept-version")
        );
        let neighbour = find_in(&loaded, "second").unwrap();
        assert_eq!(neighbour.native_profile, Profile::Optimized);
        assert_eq!(neighbour.name, "Still here");
    }
    fs::write(data.join("instances.json"), before).unwrap();
    choose("second", Profile::Optimized);
    let again = list_in(&game, &data);
    assert_eq!(
        find_in(&again, "first").unwrap().native_profile,
        Profile::Optimized
    );
    assert_eq!(
        find_in(&again, "second").unwrap().native_profile,
        Profile::Optimized
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_worlds_stay_where_they_are_and_every_folder_becomes_an_instance() {
    let root = scratch("migrate");
    let (home, data) = (root.join("sunrise"), root.join("launcher"));
    player_home(&home);
    let before = files(&home);

    // A launcher with instances, on a player's existing folders: no instances.json yet.
    let all = list_in(&home, &data);
    let ids: Vec<&str> = all.iter().map(|i| i.id.as_str()).collect();
    // A pack an earlier launcher ported (`ports/`) is not listed: its folder is left as it is.
    assert_eq!(ids, [DEFAULT, "pack:cozy"]);
    assert_eq!(all[0].name, "Sunrise");
    assert_eq!(all[0].home, home);
    assert_eq!(all[1].home, home.join("sunrise-packs/cozy/home"));

    // Everything the player can do with instances, then not one byte of theirs has moved.
    let empty = create_in(&home, &data, "Survival", None).unwrap();
    let copy = create_in(&home, &data, "My copy", Some(&all[0])).unwrap();
    edit_in(&home, &data, DEFAULT, |m| m.name = Some("Main".into())).unwrap();
    edit_in(&home, &data, "Cozy", |m| {
        m.icon = Some(Icon::Texture("diamond".into()));
        m.group = Some("Modded".into());
    })
    .unwrap();
    let after = files(&home);
    for (path, bytes) in &before {
        assert_eq!(after.get(path), Some(bytes), "{} changed", path.display());
    }
    assert!(!data.join("settings.json").exists());

    // The empty one has only the player's options and name; the copy has the worlds and
    // packs, never the sessions, logs, data collection or other instances.
    assert_eq!(empty.home, home.join("instances/survival"));
    let empty_files: Vec<PathBuf> = files(&empty.home).into_keys().collect();
    assert_eq!(
        empty_files,
        [PathBuf::from("options.txt"), "sunrise-player.txt".into()]
    );
    let copied = files(&copy.home);
    for kept in [
        "saves/World/region/r.0.0.mca",
        "saves/Other/level.dat",
        "resourcepacks/faithful.zip",
    ] {
        assert_eq!(copied[Path::new(kept)], before[Path::new(kept)]);
    }
    assert!(copied.keys().all(|p| {
        let top = p.components().next().unwrap().as_os_str().to_str().unwrap();
        !NOT_COPIED.contains(&top)
    }));

    let all = list_in(&home, &data);
    assert_eq!(all[0].name, "Main");
    let cozy = find_in(&all, "pack:cozy").unwrap();
    assert_eq!(cozy.group.as_deref(), Some("Modded"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn names_stay_unique_and_packs_are_not_copied() {
    let root = scratch("names");
    let (home, data) = (root.join("sunrise"), root.join("launcher"));
    player_home(&home);
    create_in(&home, &data, "Survival", None).unwrap();
    assert_eq!(
        create_in(&home, &data, "survival", None).unwrap_err(),
        "There is already an instance called survival. Choose another name."
    );
    assert!(create_in(&home, &data, "  ", None).is_err());
    // Another name with the same folder name gets the next free folder.
    let second = create_in(&home, &data, "Survival!", None).unwrap();
    assert_eq!(second.id, "survival-2");
    assert_eq!(second.name, "Survival!");
    assert!(edit_in(&home, &data, "survival-2", |m| m.name =
        Some("SURVIVAL".into()))
    .is_err());
    let port = find_in(&list_in(&home, &data), "pack:cozy").unwrap();
    assert!(create_in(&home, &data, "Pack copy", Some(&port)).is_err());
    assert!(!home.join("instances/pack-copy").exists());
    // An empty group is no group.
    let grouped = edit_in(&home, &data, "Survival", |m| m.group = Some(" ".into())).unwrap();
    assert_eq!(grouped.group, None);
    // An import named like an instance: both can still change everything but their names.
    let mut store = Store::load(&data);
    store.instances.entry(port.id.clone()).or_default().name = Some("Survival".into());
    store.save(&data).unwrap();
    let launched = edit_in(&home, &data, "survival", |m| {
        m.launch.wrapper = "nice".into()
    })
    .unwrap();
    assert_eq!(launched.launch.wrapper, "nice");
    assert!(edit_in(&home, &data, "survival-2", |m| m.name =
        Some("survival".into()))
    .is_err());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_instance_follows_the_players_answers() {
    let root = scratch("follow");
    let (main, own) = (root.join("main"), root.join("own"));
    put(&main.join("options.txt"), "fov:90\n");
    put(&main.join("sunrise-player.txt"), "name:Player123\n");
    put(&main.join("telemetry/consent.json"), r#"{"version":1}"#);
    crate::no_ai::set(&main, true).unwrap();
    put(&own.join("options.txt"), "fov:70\n");
    follow_player(&main, &own);
    assert_eq!(
        fs::read_to_string(own.join("options.txt")).unwrap(),
        "fov:70\n"
    );
    assert_eq!(
        fs::read_to_string(own.join("sunrise-player.txt")).unwrap(),
        "name:Player123\n"
    );
    assert_eq!(
        fs::read_to_string(own.join("telemetry/consent.json")).unwrap(),
        r#"{"version":1}"#
    );
    assert!(crate::no_ai::is_on(&own));
    crate::no_ai::set(&main, false).unwrap();
    follow_player(&main, &own);
    assert!(!crate::no_ai::is_on(&own));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn icons_are_a_texture_name_or_a_colour() {
    assert_eq!(Icon::parse("#3FA7D6"), Ok(Icon::Colour("#3fa7d6".into())));
    assert_eq!(
        Icon::parse("grass_block"),
        Ok(Icon::Texture("grass_block".into()))
    );
    for bad in ["#12345", "../x", "Grass", "", "#zzzzzz"] {
        assert!(Icon::parse(bad).is_err(), "{bad}");
    }
    let root = scratch("icon");
    put(
        &root.join("assets/minecraft/textures/item/diamond.png"),
        "png",
    );
    let diamond = Icon::Texture("diamond".into()).texture(&root).unwrap();
    assert!(diamond.ends_with("item/diamond.png"));
    assert_eq!(Icon::Texture("stone".into()).texture(&root), None);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_instance_keeps_its_own_version_until_it_follows_updates_again() {
    let root = scratch("version");
    let (home, data) = (root.join("sunrise"), root.join("launcher"));
    player_home(&home);
    create_in(&home, &data, "Old world", None).unwrap();
    let pinned = edit_in(&home, &data, "Old world", |m| {
        m.version = Some("v0.1.0".into())
    })
    .unwrap();
    assert_eq!(pinned.version.as_deref(), Some("v0.1.0"));
    // Renaming keeps it; the other instances follow the launcher's version.
    let renamed = edit_in(&home, &data, "old-world", |m| m.name = Some("Older".into())).unwrap();
    assert_eq!(renamed.version.as_deref(), Some("v0.1.0"));
    assert!(list_in(&home, &data)
        .iter()
        .filter(|i| i.id != "old-world")
        .all(|i| i.version.is_none()));
    let follows = edit_in(&home, &data, "Older", |m| m.version = None).unwrap();
    assert_eq!(follows.version, None);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn play_time_adds_up_per_instance_and_survives_a_rename() {
    let root = scratch("play-time");
    let (home, data) = (root.join("home"), root.join("data"));
    fs::create_dir_all(home.join(OWN).join("skyblock")).unwrap();
    let ended = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    record_play_in(&data, "skyblock", Duration::from_secs(25 * 60 + 10), ended).unwrap();
    edit_in(&home, &data, "skyblock", |m| m.name = Some("Sky".into())).unwrap();
    let later = ended + Duration::from_secs(3600);
    record_play_in(&data, "skyblock", Duration::from_secs(40 * 60), later).unwrap();
    let all = list_in(&home, &data);
    let sky = find_in(&all, "Sky").unwrap();
    assert_eq!(sky.played, Duration::from_secs(65 * 60 + 10));
    assert_eq!(sky.last_played, Some(later));
    let default = find_in(&all, DEFAULT).unwrap();
    assert_eq!(
        (default.played, default.last_played),
        (Duration::ZERO, None)
    );
    // Never played: nothing written for it.
    let text = fs::read_to_string(data.join(FILE)).unwrap();
    assert!(!text.contains("sunrise"), "{text}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn play_time_reads_as_minutes_then_hours() {
    let t = |s| play_time(Duration::from_secs(s));
    assert_eq!(t(59), "under a minute");
    assert_eq!(t(12 * 60 + 59), "12 min");
    assert_eq!(t(3 * 3600 + 5 * 60), "3 h 05 min");
    assert_eq!(t(140 * 3600 + 7 * 60), "140 h");
}

/// An instance an earlier launcher made for Minecraft: Java Edition is not listed, and it is not
/// touched: its folder and its whole record in instances.json come through listing, creating and
/// editing other instances, and through a save, exactly as they were.
#[test]
fn a_java_instance_survives_listing_and_saving_unchanged() {
    let root = scratch("java");
    let (home, data) = (root.join("sunrise"), root.join("launcher"));
    put(
        &home.join("instances/vanilla/saves/World/level.dat"),
        "world",
    );
    put(&home.join("instances/vanilla/mods/a.jar"), "jar");
    put(&home.join("instances/vanilla/options.txt"), "fov:90\n");
    put(&home.join("instances/back/saves/Other/level.dat"), "other");
    let record = serde_json::json!({
        "instances": {
            "vanilla": {
                "name": "Vanilla 1.21.4", "icon": {"colour": "#3fa7d6"}, "group": "Old",
                "played_secs": 7200, "last_played": 1_700_000_000,
                "launch": {"wrapper": "nice"},
                "java": {
                    "minecraft": "1.21.4", "home": "/opt/java", "memory_mb": 6144,
                    "jvm_args": "-XX:+UseZGC", "window": [1280, 720],
                    "loader": {"kind": "Fabric", "version": "0.16.9", "future": true}
                }
            },
            "back": {
                "name": "Back to Sunrise",
                "java_kept": {"minecraft": "1.20.1", "memory_mb": 2048}
            }
        }
    });
    fs::create_dir_all(&data).unwrap();
    let file = data.join("instances.json");
    fs::write(&file, record.to_string()).unwrap();
    let folders = files(&home.join("instances"));

    // Listing skips the Java instance; one that only keeps Java settings aside is a Sunrise one.
    let all = list_in(&home, &data);
    let ids: Vec<&str> = all.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, [DEFAULT, "back"]);

    // Saving other instances rewrites the file: the Java record is the same value after it.
    create_in(&home, &data, "Survival", None).unwrap();
    edit_in(&home, &data, "back", |m| m.group = Some("Mine".into())).unwrap();
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        after["instances"]["vanilla"],
        record["instances"]["vanilla"]
    );
    assert_eq!(
        after["instances"]["back"]["java_kept"],
        record["instances"]["back"]["java_kept"]
    );
    assert_eq!(after["instances"]["back"]["group"], "Mine");
    // The Java instance's folder, with its worlds and mods, is byte for byte as it was, and a new
    // instance never takes its name or folder.
    let now = files(&home.join("instances"));
    for (path, bytes) in &folders {
        assert_eq!(now.get(path), Some(bytes), "{} changed", path.display());
    }
    assert_eq!(
        create_in(&home, &data, "Vanilla", None).unwrap().home,
        home.join("instances/vanilla-2")
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn deleted_instances_go_to_the_trash_and_are_forgotten() {
    let root = scratch("delete");
    let (home, data, trash) = (root.join("home"), root.join("data"), root.join("trash"));
    player_home(&home);
    let to_trash = |p: &Path| crate::desktop_seams::to_folder(p, &trash);
    let survival = create_in(&home, &data, "Survival", None).unwrap();
    put(&survival.home.join("saves/Base/level.dat"), "base");
    edit_in(&home, &data, "Survival", |m| m.group = Some("Mine".into())).unwrap();
    let gone = delete_in(&home, &data, "Survival", &to_trash).unwrap();
    assert_eq!(gone.id, "survival");
    assert_eq!(
        fs::read(trash.join("survival/saves/Base/level.dat")).unwrap(),
        b"base"
    );
    assert!(find_in(&list_in(&home, &data), "Survival").is_err());
    assert!(!Store::load(&data).instances.contains_key("survival"));
    // The game's home stays.
    assert!(delete_in(&home, &data, DEFAULT, &to_trash)
        .unwrap_err()
        .contains("can't be deleted"));
    assert!(home.join("saves/World/level.dat").is_file());
    // A folder removed by hand: its choices are dropped, a new one of that name starts clean.
    create_in(&home, &data, "Skyblock", None).unwrap();
    edit_in(&home, &data, "Skyblock", |m| m.group = Some("Old".into())).unwrap();
    fs::remove_dir_all(home.join(OWN).join("skyblock")).unwrap();
    assert!(find_in(&list_in(&home, &data), "Skyblock").is_err());
    assert!(!Store::load(&data).instances.contains_key("skyblock"));
    let again = create_in(&home, &data, "Skyblock", None).unwrap();
    assert_eq!(again.group, None);
    fs::remove_dir_all(&root).unwrap();
}

/// A chosen picture is kept scaled to 128 pixels in the instance's folder; removing it moves it
/// to the trash; a file that isn't an image is refused.
#[test]
fn a_picture_is_kept_small_and_goes_to_the_trash() {
    let root = scratch("picture");
    let (home, trash) = (root.join("home"), root.join("trash"));
    let mut photo = Vec::new();
    image::RgbaImage::from_pixel(600, 400, image::Rgba([200, 90, 40, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut photo),
            image::ImageFormat::Png,
        )
        .unwrap();
    let png = picture(photo).unwrap();
    let to_trash = |p: &Path| crate::desktop_seams::to_folder(p, &trash);
    set_picture(&home, Some(&png), &to_trash).unwrap();
    let kept = image::open(home.join(PICTURE)).unwrap();
    assert_eq!((kept.width(), kept.height()), (128, 85));
    set_picture(&home, None, &to_trash).unwrap();
    assert!(!home.join(PICTURE).exists());
    assert!(trash.join(PICTURE).is_file());
    // Nothing to remove is fine.
    set_picture(&home, None, &to_trash).unwrap();
    assert!(picture(b"not an image".to_vec()).is_err());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_instance_without_a_channel_is_nightly_and_an_unknown_one_too() {
    let meta: Meta = serde_json::from_str("{}").unwrap();
    assert_eq!(meta.channel.unwrap_or_default(), Channel::Nightly);
    let meta: Meta = serde_json::from_str(r#"{"channel":"stable","group":"g"}"#).unwrap();
    assert_eq!(meta.channel, Some(Channel::Stable));
    let meta: Meta = serde_json::from_str(r#"{"channel":"beta","group":"g"}"#).unwrap();
    assert_eq!((meta.channel, meta.group.as_deref()), (None, Some("g")));
}

/// A saved selection that names a ported pack (listed by an earlier launcher, its folder still in
/// `ports/`) is retired: a command with no `--instance` refuses and names it instead of acting
/// on Sunrise's setup, and the window clears it. Listed and unset selections are untouched.
#[test]
fn a_retired_selection_is_refused_not_replaced_by_sunrise() {
    let root = scratch("retired");
    let (home, data) = (root.join("sunrise"), root.join("launcher"));
    put(&home.join("ports/old-pack/saves/World/level.dat"), "world");
    let ported = files(&home.join("ports"));
    let all = list_in(&home, &data);
    assert!(all.iter().all(|i| !i.id.contains("old-pack")));

    let refused = selection_in(&all, Some("port:old-pack")).unwrap_err();
    assert!(refused.contains("old-pack"), "{refused}");
    assert!(refused.contains("instance select"), "{refused}");
    assert_eq!(
        retired_in(&all, Some("port:old-pack")).as_deref(),
        Some("port:old-pack")
    );

    let made = create_in(&home, &data, "Survival", None, None).unwrap();
    let all = list_in(&home, &data);
    assert_eq!(selection_in(&all, None).unwrap().id, DEFAULT);
    assert_eq!(selection_in(&all, Some(DEFAULT)).unwrap().id, DEFAULT);
    assert_eq!(selection_in(&all, Some(&made.id)).unwrap().id, made.id);
    assert_eq!(retired_in(&all, Some(&made.id)), None);
    assert_eq!(retired_in(&all, None), None);
    assert_eq!(
        files(&home.join("ports")),
        ported,
        "its files are untouched"
    );
    fs::remove_dir_all(&root).unwrap();
}
