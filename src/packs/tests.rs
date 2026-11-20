use super::*;
use std::io::Write;

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sunrise-launcher-packs-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn zip_file(path: &Path, files: &[(&str, &[u8])]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

const META: &[u8] = "{\"pack\":{\"pack_format\":69,\"description\":[\"\",{\"text\":\"Smooth \u{a7}aglass\"},\"\\nand more\"]}}"
    .as_bytes();

const VALID_POST: &[u8] = b"fn post(input: PostInput) -> vec4<f32> { return input.color; }\n";
const MALFORMED_POST: &[u8] = b"fn post(input: PostInput) -> vec4<f32> { return input.color }\n";

const SHADER_META: &str = "schema = 1\nid = \"warm\"\nname = \"Warm Light\"\nversion = \"1.0\"\napi = \"0.1\"\n[[post]]\nfile = \"post.wgsl\"\n";

/// A game folder with three resource packs (one on), and shader packs of both kinds.
fn home(name: &str) -> PathBuf {
    let home = scratch(name);
    let rp = home.join("resourcepacks");
    zip_file(
        &rp.join("Glass.zip"),
        &[("pack.mcmeta", META), ("pack.png", b"png")],
    );
    zip_file(&rp.join("Broken.zip"), &[("readme.txt", b"no meta")]);
    fs::create_dir_all(rp.join("Folder Pack")).unwrap();
    fs::write(
        rp.join("Folder Pack/pack.mcmeta"),
        br#"{"pack":{"description":"Mine"}}"#,
    )
    .unwrap();
    zip_file(&rp.join("Zed.zip"), &[("pack.mcmeta", META)]);
    let sp = home.join("shaderpacks");
    fs::create_dir_all(sp.join("warm")).unwrap();
    fs::write(sp.join("warm/sunrise-shaders.toml"), SHADER_META).unwrap();
    zip_file(&sp.join("Classic.zip"), &[("shaders/final.fsh", b"")]);
    fs::write(sp.join("notes.txt"), b"").unwrap();
    fs::write(
        home.join("options.txt"),
        "version:4786\nresourcePacks:[\"vanilla\",\"file/Zed.zip\"]\nincompatibleResourcePacks:[\"file/Zed.zip\"]\nlang:fr_ch\n",
    )
    .unwrap();
    fs::write(
        home.join("sunrise-graphics.txt"),
        "preset:custom\npickedShaderPack:\nfxaa:true\n",
    )
    .unwrap();
    home
}

#[test]
fn lists_what_the_game_reads_and_which_are_on() {
    let home = home("list");
    let rp = list(&home, Kind::Resource);
    let names: Vec<(&str, bool)> = rp.iter().map(|p| (p.file.as_str(), p.enabled)).collect();
    assert_eq!(
        names,
        [
            ("Folder Pack", false),
            ("Glass.zip", false),
            ("Zed.zip", true)
        ],
        "by name; the pack without pack.mcmeta is left out, as the game does"
    );
    let glass = &rp[1];
    assert_eq!(glass.description, "Smooth glass and more");
    assert_eq!(glass.icon.as_deref(), Some(&b"png"[..]));
    let sp = list(&home, Kind::Shader);
    let names: Vec<&str> = sp.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(names, ["Classic.zip", "Warm Light"]);
    assert!(list(&home.join("none"), Kind::Shader).is_empty());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn turning_packs_on_and_off_writes_the_games_own_lines() {
    let home = home("toggle");
    let glass = find(&home, Kind::Resource, "Glass.zip").unwrap();
    set_enabled(&home, &glass, true).unwrap();
    let folder = find(&home, Kind::Resource, "folder pack").unwrap();
    set_enabled(&home, &folder, true).unwrap();
    assert!(
        fs::read_to_string(home.join("options.txt")).unwrap().contains(
            "resourcePacks:[\"vanilla\",\"file/Zed.zip\",\"file/Glass.zip\",\"file/Folder Pack\"]\n"
        ),
        "the newest on top"
    );
    let zed = find(&home, Kind::Resource, "Zed.zip").unwrap();
    set_enabled(&home, &zed, false).unwrap();
    assert_eq!(
        fs::read_to_string(home.join("options.txt")).unwrap(),
        "version:4786\nresourcePacks:[\"vanilla\",\"file/Glass.zip\",\"file/Folder Pack\"]\nincompatibleResourcePacks:[]\nlang:fr_ch\n"
    );

    let warm = find(&home, Kind::Shader, "Warm Light").unwrap();
    set_enabled(&home, &warm, true).unwrap();
    let classic = find(&home, Kind::Shader, "Classic.zip").unwrap();
    set_enabled(&home, &classic, true).unwrap();
    assert_eq!(
        fs::read_to_string(home.join("sunrise-graphics.txt")).unwrap(),
        "preset:custom\npickedShaderPack:Classic.zip\nfxaa:true\n",
        "one shader pack at a time"
    );
    set_enabled(
        &home,
        &find(&home, Kind::Shader, "Classic.zip").unwrap(),
        false,
    )
    .unwrap();
    assert!(list(&home, Kind::Shader).iter().all(|p| !p.enabled));

    // A fresh instance has neither file yet.
    let fresh = scratch("fresh");
    fs::create_dir_all(fresh.join("resourcepacks")).unwrap();
    fs::copy(
        home.join("resourcepacks/Zed.zip"),
        fresh.join("resourcepacks/Zed.zip"),
    )
    .unwrap();
    set_enabled(
        &fresh,
        &find(&fresh, Kind::Resource, "Zed.zip").unwrap(),
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(fresh.join("options.txt")).unwrap(),
        "resourcePacks:[\"vanilla\",\"file/Zed.zip\"]\n"
    );
    fs::remove_dir_all(&home).unwrap();
    fs::remove_dir_all(&fresh).unwrap();
}

#[test]
fn a_dropped_file_goes_to_its_folder_under_a_free_name() {
    let home = home("add");
    let drops = scratch("drops");
    zip_file(&drops.join("Zed.zip"), &[("pack.mcmeta", META)]);
    zip_file(&drops.join("Shiny.zip"), &[("shaders/composite.fsh", b"")]);
    zip_file(
        &drops.join("glow.sshader"),
        &[
            (SHADER_MANIFEST, SHADER_META.as_bytes()),
            ("post.wgsl", b""),
        ],
    );
    fs::write(drops.join("broken.sshader"), b"PK").unwrap();
    zip_file(&drops.join("random.zip"), &[("a.txt", b"")]);
    fs::write(drops.join("picture.png"), b"").unwrap();

    let added = add(&home, &drops.join("Zed.zip")).unwrap();
    assert_eq!(
        (added.kind, added.file.as_str(), added.enabled),
        (Kind::Resource, "Zed (2).zip", false)
    );
    assert_eq!(
        add(&home, &drops.join("Shiny.zip")).unwrap().kind,
        Kind::Shader
    );
    assert!(home.join("shaderpacks/Shiny.zip").is_file());
    assert_eq!(kind_of(&drops.join("glow.sshader")).unwrap(), Kind::Shader);
    assert!(add(&home, &drops.join("broken.sshader")).is_err());
    assert!(add(&home, &drops.join("random.zip")).is_err());
    assert!(add(&home, &drops.join("picture.png")).is_err());
    fs::remove_dir_all(&home).unwrap();
    fs::remove_dir_all(&drops).unwrap();
}

#[test]
fn native_shader_choice_follows_the_manifest_after_rename() {
    let home = home("native-identity");
    let pack = find(&home, Kind::Shader, "warm").unwrap();
    set_enabled(&home, &pack, true).unwrap();
    assert_eq!(
        line_value(
            &fs::read_to_string(home.join(GRAPHICS)).unwrap(),
            "pickedShaderPack"
        ),
        Some("sunrise:warm".into())
    );
    fs::rename(
        home.join("shaderpacks/warm"),
        home.join("shaderpacks/renamed"),
    )
    .unwrap();
    assert!(find(&home, Kind::Shader, "renamed").unwrap().enabled);
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "warm-1.0.sshader").unwrap();
    assert!(find(&home, Kind::Shader, "renamed").unwrap().enabled);
    fs::create_dir_all(home.join("shaderpacks/a-copy")).unwrap();
    fs::copy(
        home.join("shaderpacks/renamed/sunrise-shaders.toml"),
        home.join("shaderpacks/a-copy/sunrise-shaders.toml"),
    )
    .unwrap();
    let listed = list(&home, Kind::Shader);
    assert_eq!(
        listed
            .iter()
            .filter(|p| p.description == "Sunrise shader pack")
            .count(),
        1
    );
    assert!(listed.iter().any(|p| p.file == "a-copy" && p.enabled));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn toggling_native_shader_preserves_legacy_options_before_renaming() {
    for old in ["warm", "warm-1.0.sshader"] {
        let home = home(&format!("native-options-{old}"));
        let dir = home.join("shaderpacks");
        let legacy = dir.join(format!("{old}.txt"));
        fs::write(&legacy, "strength=1.5\n").unwrap();
        set_line(&home.join(GRAPHICS), "pickedShaderPack", old).unwrap();
        let pack = find(&home, Kind::Shader, "warm").unwrap();
        set_enabled(&home, &pack, false).unwrap();
        let canonical = dir.join(".sunrise-options/warm.txt");
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "strength=1.5\n");
        fs::rename(dir.join("warm"), dir.join("renamed")).unwrap();
        let renamed = find(&home, Kind::Shader, "renamed").unwrap();
        set_enabled(&home, &renamed, true).unwrap();
        fs::write(&legacy, "strength=2\n").unwrap();
        set_enabled(&home, &renamed, true).unwrap();
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "strength=1.5\n");
        assert_eq!(fs::read_to_string(legacy).unwrap(), "strength=2\n");
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn switching_to_classic_preserves_the_outgoing_native_options_after_update() {
    let home = home("native-switch");
    let dir = home.join("shaderpacks");
    fs::rename(dir.join("warm"), dir.join(".old-fixture")).unwrap();
    zip_file(
        &dir.join("warm-2.0.sshader"),
        &[(SHADER_MANIFEST, SHADER_META.as_bytes())],
    );
    let legacy = dir.join("warm-1.0.sshader.txt");
    fs::write(&legacy, "strength=1.5\n").unwrap();
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "warm-1.0.sshader").unwrap();
    let classic = find(&home, Kind::Shader, "Classic.zip").unwrap();
    set_enabled(&home, &classic, true).unwrap();
    let native = find(&home, Kind::Shader, "warm-2.0.sshader").unwrap();
    set_enabled(&home, &native, true).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join(".sunrise-options/warm.txt")).unwrap(),
        "strength=1.5\n"
    );
    assert_eq!(fs::read_to_string(legacy).unwrap(), "strength=1.5\n");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn invalid_native_manifest_does_not_hide_a_valid_duplicate() {
    let home = home("native-invalid-duplicate");
    let dir = home.join("shaderpacks");
    fs::rename(dir.join("warm"), dir.join(".old-fixture")).unwrap();
    let invalid = SHADER_META.replace("schema = 1", "schema = 99");
    zip_file(
        &dir.join("a.sshader"),
        &[(SHADER_MANIFEST, invalid.as_bytes())],
    );
    zip_file(
        &dir.join("z.sshader"),
        &[(SHADER_MANIFEST, SHADER_META.as_bytes())],
    );
    set_line(&home.join(GRAPHICS), "pickedShaderPack", "sunrise:warm").unwrap();
    let native = find(&home, Kind::Shader, "z.sshader").unwrap();
    assert!(native.enabled);
    let invalid = find(&home, Kind::Shader, "a.sshader").unwrap();
    assert!(!invalid.enabled);
    set_enabled(&home, &invalid, true).unwrap();
    assert_eq!(
        line_value(
            &fs::read_to_string(home.join(GRAPHICS)).unwrap(),
            "pickedShaderPack"
        ),
        Some("a.sshader".into())
    );
    fs::remove_dir_all(home).unwrap();
}

fn tree(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            out.push(e.path().to_string_lossy().into_owned());
            if e.path().is_dir() {
                walk(&e.path(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

#[test]
fn add_local_takes_only_the_chosen_kind_and_leaves_nothing_on_error() {
    let home = home("add-local");
    let drops = scratch("add-local-drops");
    zip_file(&drops.join("Zed.zip"), &[("pack.mcmeta", META)]);
    zip_file(&drops.join("Shiny.zip"), &[("shaders/composite.fsh", b"")]);
    zip_file(
        &drops.join("glow.sshader"),
        &[
            (
                SHADER_MANIFEST,
                SHADER_META.replace("\"warm\"", "\"glow\"").as_bytes(),
            ),
            ("post.wgsl", VALID_POST),
        ],
    );
    // Parses as a package, but its WGSL does not compile.
    zip_file(
        &drops.join("typo.sshader"),
        &[
            (SHADER_MANIFEST, SHADER_META.as_bytes()),
            ("post.wgsl", MALFORMED_POST),
        ],
    );
    // Data only: a data pack, not a resource pack; with assets/ too it is both.
    zip_file(
        &drops.join("Data.zip"),
        &[("pack.mcmeta", META), ("data/x/tags/a.json", b"{}")],
    );
    zip_file(&drops.join("Bad.zip"), &[("pack.mcmeta", b"{")]);
    zip_file(&drops.join(".hidden.zip"), &[("pack.mcmeta", META)]);
    zip_file(
        &drops.join("Both.zip"),
        &[
            ("pack.mcmeta", META),
            ("data/x/tags/a.json", b"{}"),
            ("assets/x/a.json", b"{}"),
        ],
    );
    // A manifest naming a file the package lacks: the shader package's own check fails it.
    zip_file(
        &drops.join("hollow.sshader"),
        &[(SHADER_MANIFEST, SHADER_META.as_bytes())],
    );
    fs::write(drops.join("broken.sshader"), b"PK").unwrap();
    let options = fs::read_to_string(home.join(OPTIONS)).unwrap();
    let graphics = fs::read_to_string(home.join(GRAPHICS)).unwrap();

    let before = tree(&home);
    for (kind, file) in [
        (Kind::Shader, "Zed.zip"),
        (Kind::Resource, "Shiny.zip"),
        (Kind::Resource, "glow.sshader"),
        (Kind::Shader, "hollow.sshader"),
        (Kind::Shader, "typo.sshader"),
        (Kind::Resource, "Data.zip"),
        (Kind::Resource, "Bad.zip"),
        (Kind::Resource, ".hidden.zip"),
        (Kind::Resource, "."),
        (Kind::Shader, "broken.sshader"),
        (Kind::Resource, "missing.zip"),
    ] {
        assert!(add_local(&home, kind, &drops.join(file)).is_err(), "{file}");
        assert_eq!(tree(&home), before, "{file} left something behind");
    }

    let first = add_local(&home, Kind::Resource, &drops.join("Zed.zip")).unwrap();
    assert_eq!(
        (first.file.as_str(), first.enabled),
        ("Zed (2).zip", false),
        "the same name never replaces a pack"
    );
    let again = add_local(&home, Kind::Resource, &drops.join("Zed.zip")).unwrap();
    assert_eq!(again.file, "Zed (3).zip");
    assert_eq!(
        fs::read(home.join("resourcepacks/Zed.zip")).unwrap(),
        fs::read(drops.join("Zed.zip")).unwrap()
    );
    assert_eq!(
        add_local(&home, Kind::Resource, &drops.join("Both.zip"))
            .unwrap()
            .file,
        "Both.zip"
    );
    let glow = add_local(&home, Kind::Shader, &drops.join("glow.sshader")).unwrap();
    assert!(!glow.enabled && home.join("shaderpacks/glow.sshader").is_file());
    assert!(
        tree(&home).iter().all(|p| !p.contains(".sunrise-add-")),
        "no temporary file stays"
    );
    assert_eq!(fs::read_to_string(home.join(OPTIONS)).unwrap(), options);
    assert_eq!(fs::read_to_string(home.join(GRAPHICS)).unwrap(), graphics);

    let empty = scratch("add-local-empty");
    assert!(add_local(&empty, Kind::Shader, &drops.join("Zed.zip")).is_err());
    // Bad content found only after the copy: the folder made for it goes too.
    assert!(add_local(&empty, Kind::Shader, &drops.join("typo.sshader")).is_err());
    assert!(add_local(&empty, Kind::Resource, &drops.join("Data.zip")).is_err());
    assert!(add_local(&empty, Kind::Resource, &drops.join("Bad.zip")).is_err());
    assert!(add_local(&empty, Kind::Resource, &drops.join(".hidden.zip")).is_err());
    assert!(
        add_local(&empty, Kind::Resource, &drops).is_err(),
        "a folder"
    );
    assert!(
        fs::read_dir(&empty).unwrap().next().is_none(),
        "a refused file makes no folder"
    );
    for dir in [home, drops, empty] {
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn resource_order_lists_enabled_packs_top_first_and_moves_adjacent_pairs() {
    let home = home("order");
    let rp = home.join("resourcepacks");
    for name in ["A.zip", "B.zip", "C.zip"] {
        zip_file(&rp.join(name), &[("pack.mcmeta", META)]);
    }
    let set = |list: &str| {
        fs::write(
            home.join(OPTIONS),
            format!("version:4786\nresourcePacks:{list}\nlang:fr_ch\n"),
        )
        .unwrap()
    };
    let line = || {
        line_value(
            &fs::read_to_string(home.join(OPTIONS)).unwrap(),
            "resourcePacks",
        )
        .unwrap()
    };
    set(
        r#"["vanilla","file/A.zip","mod_resources","file/gone.zip","file/B.zip","file/C.zip","file/Glass.zip"]"#,
    );
    fs::remove_file(rp.join("Glass.zip")).unwrap();
    assert_eq!(
        resource_order(&home),
        ["Glass.zip", "C.zip", "B.zip", "gone.zip", "A.zip"],
        "the raw list: a page ranks the packs it shows"
    );
    let shown = |home: &Path| -> Vec<String> {
        let on = list(home, Kind::Resource);
        resource_order(home)
            .into_iter()
            .filter(|n| on.iter().any(|p| &p.file == n))
            .collect()
    };
    assert_eq!(shown(&home), ["C.zip", "B.zip", "A.zip"]);

    // Adjacent among the packs shown, in either direction; the rest keeps its place.
    move_resource(&home, "A.zip", "B.zip").unwrap();
    assert_eq!(
        line(),
        r#"["vanilla","file/B.zip","mod_resources","file/gone.zip","file/A.zip","file/C.zip","file/Glass.zip"]"#
    );
    assert_eq!(shown(&home), ["C.zip", "A.zip", "B.zip"]);
    move_resource(&home, "C.zip", "A.zip").unwrap();
    assert_eq!(shown(&home), ["A.zip", "C.zip", "B.zip"]);

    // A stale page: not adjacent, not enabled, itself, unknown, or a pack listed twice.
    let before = fs::read_to_string(home.join(OPTIONS)).unwrap();
    for (a, b) in [
        ("A.zip", "B.zip"),
        ("A.zip", "A.zip"),
        ("Folder Pack", "A.zip"),
        ("gone.zip", "A.zip"),
        ("nope.zip", "A.zip"),
    ] {
        assert!(move_resource(&home, a, b).is_err(), "{a} {b}");
    }
    set(r#"["file/A.zip","file/B.zip","file/A.zip"]"#);
    let before_twice = fs::read_to_string(home.join(OPTIONS)).unwrap();
    assert!(move_resource(&home, "A.zip", "B.zip").is_err());
    assert_eq!(
        fs::read_to_string(home.join(OPTIONS)).unwrap(),
        before_twice
    );
    assert_ne!(before, before_twice);
    fs::remove_file(home.join(OPTIONS)).unwrap();
    assert!(resource_order(&home).is_empty());
    assert!(move_resource(&home, "A.zip", "B.zip").is_err());
    assert!(!home.join(OPTIONS).exists());
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn reordering_never_treats_an_unreadable_options_file_as_empty() {
    let home = home("order-unreadable");
    let rp = home.join("resourcepacks");
    for name in ["A.zip", "B.zip"] {
        zip_file(&rp.join(name), &[("pack.mcmeta", META)]);
    }
    // A folder where the file should be: reading it fails, and nothing may replace it.
    fs::remove_file(home.join(OPTIONS)).ok();
    fs::create_dir(home.join(OPTIONS)).unwrap();
    let error = move_resource(&home, "A.zip", "B.zip").unwrap_err();
    assert!(error.contains(OPTIONS), "{error}");
    assert!(home.join(OPTIONS).is_dir());
}

#[test]
fn reordering_replaces_options_whole_and_keeps_its_other_lines() {
    let home = home("order-atomic");
    let rp = home.join("resourcepacks");
    for name in ["A.zip", "B.zip"] {
        zip_file(&rp.join(name), &[("pack.mcmeta", META)]);
    }
    fs::write(
        home.join(OPTIONS),
        "version:4786\nresourcePacks:[\"vanilla\",\"file/A.zip\",\"file/B.zip\"]\nlang:fr_ch\n",
    )
    .unwrap();
    move_resource(&home, "A.zip", "B.zip").unwrap();
    let text = fs::read_to_string(home.join(OPTIONS)).unwrap();
    assert_eq!(
        text,
        "version:4786\nresourcePacks:[\"vanilla\",\"file/B.zip\",\"file/A.zip\"]\nlang:fr_ch\n"
    );
    let left: Vec<_> = fs::read_dir(&home)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".sunrise-options-")
        })
        .collect();
    assert!(left.is_empty(), "a temporary file was left behind");
}

#[test]
fn a_shader_pack_whose_id_is_installed_is_refused() {
    let home = home("add-same-id");
    let drops = scratch("drops-same-id");
    for file in ["warm-copy.sshader", "warm-new.sshader"] {
        zip_file(
            &drops.join(file),
            &[
                (SHADER_MANIFEST, SHADER_META.as_bytes()),
                ("post.wgsl", b""),
            ],
        );
    }
    // `home` already holds the pack with the id "warm"; the manifest above is that id.
    assert_eq!(
        shader_key(&drops.join("warm-copy.sshader")).as_deref(),
        Some("sunrise:warm")
    );
    let before = fs::read_dir(home.join("shaderpacks")).unwrap().count();
    for result in [
        add(&home, &drops.join("warm-copy.sshader")),
        add_local(&home, Kind::Shader, &drops.join("warm-new.sshader")),
    ] {
        let error = result.unwrap_err();
        assert!(
            error.contains("warm") && error.contains("already installed"),
            "{error}"
        );
    }
    assert_eq!(
        fs::read_dir(home.join("shaderpacks")).unwrap().count(),
        before
    );
    fs::remove_dir_all(&home).unwrap();
    fs::remove_dir_all(&drops).unwrap();
}

/// What a player reads goes through `tr` or `trf`: no `format!` of the pack code builds a
/// sentence of English words (a path and an error are not words).
#[test]
fn pack_errors_are_not_built_with_format() {
    for file in ["src/packs.rs", "src/mods/packs.rs"] {
        let source =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap();
        let code = source.split("#[cfg(test)]").next().unwrap();
        for (at, _) in code.match_indices("format!(") {
            let rest = &code[at + "format!(".len()..];
            let literal = rest
                .trim_start()
                .strip_prefix('"')
                .and_then(|r| r.split('"').next())
                .unwrap_or("");
            let mut bare = String::new();
            let mut skipping = false;
            for c in literal.chars() {
                match c {
                    '{' => skipping = true,
                    '}' => skipping = false,
                    _ if !skipping => bare.push(c),
                    _ => {}
                }
            }
            let words = bare
                .split_whitespace()
                .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2)
                .count();
            assert!(
                words < 2,
                "{file}: {literal:?} reads as a sentence: use tr or trf"
            );
        }
    }
}

#[test]
fn a_saved_shader_choice_names_one_pack_in_the_folder_or_nothing() {
    let home = scratch("picked");
    let graphics = home.join(GRAPHICS);
    for (saved, picked) in [
        ("Warm.zip", Some("Warm.zip")),
        ("sunrise:warm", Some("sunrise:warm")),
        ("", None),
        ("..", None),
        ("../outside.zip", None),
        ("/etc/passwd", None),
        ("folder/inside.zip", None),
        ("..\\outside.zip", None),
        ("sunrise:", None),
        ("sunrise:../warm", None),
    ] {
        fs::write(&graphics, format!("pickedShaderPack:{saved}\n")).unwrap();
        assert_eq!(picked_shader(&home).as_deref(), picked, "{saved:?}");
    }
    // A choice that leads out of the folder turns nothing on, whatever lies there.
    zip_file(&home.join("outside.zip"), &[("shaders/final.fsh", b"")]);
    zip_file(
        &home.join("shaderpacks/Plain.zip"),
        &[("shaders/final.fsh", b"")],
    );
    fs::write(&graphics, "pickedShaderPack:../outside.zip\n").unwrap();
    assert!(list(&home, Kind::Shader).iter().all(|p| !p.enabled));
}

#[test]
fn a_folder_pack_is_read_only_from_regular_files_of_its_own() {
    let home = scratch("regular");
    let rp = home.join("resourcepacks");
    let own = rp.join("Own");
    fs::create_dir_all(&own).unwrap();
    fs::write(own.join("pack.mcmeta"), META).unwrap();
    fs::write(own.join("pack.png"), b"png").unwrap();
    // Its metadata is a folder, not a file: not a pack, and nothing is opened.
    let odd = rp.join("Odd");
    fs::create_dir_all(odd.join("pack.mcmeta")).unwrap();
    assert_eq!(entry(&odd, "pack.mcmeta"), None);
    // A folder under a zip's name is not opened as one.
    fs::create_dir_all(rp.join("Folder.zip")).unwrap();
    assert!(regular(&rp.join("Folder.zip")).is_none());
    #[cfg(unix)]
    {
        // A link out of the pack is not followed.
        fs::write(home.join("secret.png"), b"secret").unwrap();
        fs::remove_file(own.join("pack.png")).unwrap();
        std::os::unix::fs::symlink(home.join("secret.png"), own.join("pack.png")).unwrap();
    }
    let packs = list(&home, Kind::Resource);
    assert_eq!(
        packs.iter().map(|p| p.file.as_str()).collect::<Vec<_>>(),
        ["Own"]
    );
    #[cfg(unix)]
    assert_eq!(packs[0].icon, None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_pipe_in_a_packs_place_is_never_opened() {
    let home = scratch("pipe");
    let rp = home.join("resourcepacks");
    let pack = rp.join("Piped");
    fs::create_dir_all(&pack).unwrap();
    let fifo = |path: &Path| {
        let made = std::process::Command::new("mkfifo").arg(path).status();
        made.is_ok_and(|s| s.success())
    };
    // Opening either to read would wait for a writer that never comes.
    if !fifo(&pack.join("pack.mcmeta")) || !fifo(&rp.join("Pipe.zip")) {
        return;
    }
    assert!(list(&home, Kind::Resource).is_empty());
}

#[test]
fn a_listing_reads_only_the_pictures_it_was_asked_for() {
    let home = home("pictures");
    fs::write(
        home.join("options.txt"),
        "resourcePacks:[\"vanilla\",\"file/Glass.zip\"]\n",
    )
    .unwrap();
    fs::write(home.join("resourcepacks/Folder Pack/pack.png"), b"png").unwrap();
    let pictured = |pictures| -> Vec<(String, bool)> {
        list_with(&home, Kind::Resource, pictures)
            .into_iter()
            .map(|p| (p.file, p.icon.is_some()))
            .collect()
    };
    let all = pictured(Pictures::All);
    let metadata_only = pictured(Pictures::None);
    assert_eq!(all.len(), 3);
    assert_eq!(metadata_only.len(), all.len());
    assert!(all.iter().any(|(_, has_picture)| *has_picture));
    for ((name, _), (also, has_picture)) in all.iter().zip(&metadata_only) {
        assert_eq!(name, also);
        assert!(!has_picture, "metadata listing read the picture for {name}");
    }
    assert_eq!(
        artwork(&home.join("resourcepacks/Folder Pack"), Kind::Resource).as_deref(),
        Some(&b"png"[..]),
    );
}

#[test]
fn a_resource_pack_file_is_listed_only_when_its_name_ends_in_zip() {
    let home = scratch("zip-name");
    let rp = home.join("resourcepacks");
    for name in ["Renamed.jar", "Paused.zip.disabled", "Upper.ZIP", "Plain.zip"] {
        zip_file(&rp.join(name), &[("pack.mcmeta", META)]);
    }
    fs::create_dir_all(rp.join("Folder.jar")).unwrap();
    fs::write(rp.join("Folder.jar/pack.mcmeta"), META).unwrap();
    let names: Vec<String> = list(&home, Kind::Resource)
        .into_iter()
        .map(|p| p.file)
        .collect();
    assert_eq!(names, ["Folder.jar", "Plain.zip"]);
    let drops = home.join("drops");
    zip_file(&drops.join("Dropped.ZIP"), &[("pack.mcmeta", META)]);
    assert!(add(&home, &drops.join("Dropped.ZIP")).is_err());
    assert!(!rp.join("Dropped.ZIP").exists());
    fs::remove_dir_all(&home).unwrap();
}
