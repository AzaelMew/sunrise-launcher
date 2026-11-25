#[test]
fn non_files_with_smod_names_are_not_archive_candidates() {
    let home = tempfile::tempdir().unwrap();
    let native = home.path().join(native_mods::INSTALLED);
    let content = home.path().join(sunrise_launcher_smod::mod_list::DIR);
    for dir in [&native, &content] {
        fs::create_dir_all(dir.join("not-an-archive.smod")).unwrap();
    }
    assert!(hmods(home.path()).is_empty());
    assert!(content_files(home.path()).is_empty());
    assert!(enabled(home.path()).is_empty());
    assert!(untracked(home.path()).is_empty());
}

#[test]
fn an_unreadable_file_is_no_build_input_and_never_stands_for_a_mod_of_its_name() {
    let home = tempfile::tempdir().unwrap();
    let native = home.path().join(native_mods::INSTALLED);
    fs::create_dir_all(&native).unwrap();
    fs::write(native.join("ruby.smod"), b"not a package").unwrap();
    // Another mod called `ruby` is on, as its own file or its content says.
    mod_list::set_enabled(home.path(), "ruby", true).unwrap();
    assert!(enabled(home.path()).is_empty());
}

#[test]
fn a_native_file_the_build_cannot_read_is_listed_unreadable() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let native = home.path().join(native_mods::INSTALLED);
    fs::create_dir_all(&native).unwrap();
    // A well-formed package with no `[native]` table: the build reads it as no native mod.
    let mut zip = zip::ZipWriter::new(fs::File::create(native.join("plain.smod")).unwrap());
    zip.start_file("sunrise-mod.toml", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(
        b"id='plain'\nname='Plain'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n",
    )
    .unwrap();
    zip.finish().unwrap();
    mod_list::set_enabled(home.path(), "plain", true).unwrap();
    let found = untracked(home.path());
    assert_eq!(found.len(), 1);
    assert!(found[0].icon.is_err());
    assert!(enabled(home.path()).is_empty());
}

#[test]
fn the_uses_table_is_not_part_of_a_mods_digest() {
    let source = [("Cargo.toml", b"a".as_slice()), ("src/lib.rs", b"b")];
    let packed = [source[0], source[1], ("uses.json", b"{}".as_slice())];
    assert_eq!(
        archive_digest(packed.into_iter()),
        archive_digest(source.into_iter())
    );
    assert_eq!(
        archive_digest(source.into_iter()),
        native_mods::digest(source.into_iter())
    );
}

#[test]
fn only_the_changed_mod_needs_a_build() {
    let m = |id: &str, v: &str, d: Option<&str>| (id.into(), v.into(), d.map(Into::into));
    let built = [m("ores", "1.0", Some("a")), m("maps", "2.0", Some("b"))];
    // `maps` updated, `ores` untouched, `tools` new: only the changed and new ones build.
    let enabled = [
        m("ores", "1.0", Some("a")),
        m("maps", "2.1", Some("c")),
        m("tools", "1.0", None),
    ];
    assert_eq!(unbuilt(&enabled, Some(&built)), ["maps", "tools"]);
    // Same version, other files (a reinstalled local `.smod`): that one builds again.
    assert_eq!(
        unbuilt(&[m("ores", "1.0", Some("z"))], Some(&built)),
        ["ores"]
    );
    // An old record without digests still counts as built.
    assert!(unbuilt(
        &[m("ores", "1.0", Some("a"))],
        Some(&[m("ores", "1.0", None)])
    )
    .is_empty());
    assert_eq!(unbuilt(&enabled[..1], None), ["ores"]);
}

use super::*;

#[test]
fn a_kept_fingerprint_is_read_again_when_the_glance_moves_when_due_and_for_another_version() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let mods = home.join(sunrise_launcher_smod::mod_list::DIR);
    fs::create_dir_all(&mods).unwrap();
    fs::write(mods.join("a.smod"), b"one").unwrap();
    let mut kept = Fingerprints::default();
    let t0 = std::time::Instant::now();
    let first = kept.read_glanced(home, None, t0, "a".into());
    assert_eq!(first, fingerprint(home, None));

    // A mod rewritten where it lies: the full fingerprint moves, the glance doesn't.
    fs::write(mods.join("a.smod"), b"one, longer").unwrap();
    assert_ne!(fingerprint(home, None), first);
    assert_eq!(kept.read_glanced(home, None, t0, "a".into()), first, "kept");
    assert!(!kept.due(t0));
    // A glance that moved is read again at once.
    let moved = kept.read_glanced(home, None, t0, "b".into());
    assert_eq!(moved, fingerprint(home, None));

    // Rewritten again: kept until the full read is due, then read.
    fs::write(mods.join("a.smod"), b"one, longer still").unwrap();
    let later = t0 + FULL_EVERY;
    assert!(kept.due(later));
    assert_eq!(
        kept.read_glanced(home, None, later, "b".into()),
        fingerprint(home, None)
    );
    assert!(!kept.due(later), "read again at its time");

    // Only the version differs: the same folder, glance and clock are not enough.
    let versioned = kept.read_glanced(home, Some("26.3"), later, "b".into());
    assert_eq!(versioned, fingerprint(home, Some("26.3")));
    assert_ne!(versioned, fingerprint(home, None));
}

#[test]
fn the_glance_sees_the_lists_text_and_times_it_watches() {
    use std::time::{Duration, SystemTime};
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let set_time = |path: &Path, time: SystemTime| {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };
    // The on/off list edited to the same length, its time put back: still seen.
    let list = home.join(mod_list::MODS_LIST);
    fs::write(&list, "mod.a=true\n").unwrap();
    set_time(&list, at);
    let before = glance(home);
    fs::write(&list, "mod.b=true\n").unwrap();
    set_time(&list, at);
    assert_ne!(glance(home), before, "which mods are on changed");

    // The browser's list rewritten: its time moves.
    let browser = home.join("mod-browser.json");
    fs::write(&browser, b"{}").unwrap();
    set_time(&browser, at);
    let before = glance(home);
    set_time(&browser, at + Duration::from_secs(1));
    assert_ne!(glance(home), before);

    // A mod added to a folder moves the folder's time (set here, not waited for).
    #[cfg(unix)]
    {
        let mods = home.join(sunrise_launcher_smod::mod_list::DIR);
        fs::create_dir_all(&mods).unwrap();
        let set_dir = |time: SystemTime| {
            fs::File::open(&mods).unwrap().set_modified(time).unwrap();
        };
        set_dir(at);
        let before = glance(home);
        set_dir(at + Duration::from_secs(1));
        assert_ne!(glance(home), before);
    }
}

fn m(id: &str, version: &str) -> Mod {
    (id.into(), version.into(), None)
}

fn d(id: &str, version: &str, digest: &str) -> Mod {
    (id.into(), version.into(), Some(digest.into()))
}

#[test]
fn declared_paths_use_root_internals() {
    let manifest = sunrise_launcher_smod::Manifest::parse(
        "id='owner'\nname='Owner'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\ninternals=[{path='Game',hash='game'},{path='Game::world',kind='field',hash='world'}]\n[native]\ngame='26.3'\nsunrise='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\nmixins='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'\n",
    )
    .unwrap();
    assert_eq!(declared_internals(&manifest), ["Game", "Game::world"]);
}

#[test]
fn a_content_package_served_by_a_native_mod_is_listed_with_its_host() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(sunrise_launcher_smod::mod_list::DIR);
    fs::create_dir_all(&folder).unwrap();
    let bytes = sunrise_launcher_smod::manifest::Package::write_files(
        "id='runes'\nname='Runes'\nversion='1.0.0'\napi='0.2'\nserved_by='java_host'\n",
        &[],
    )
    .unwrap();
    fs::write(folder.join("runes.smod"), bytes).unwrap();
    let listed = untracked_content(home.path());
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "runes");
    assert_eq!(listed[0].served_by.as_deref(), Some("java_host"));
}

#[test]
fn content_package_changes_invalidate_the_local_mods_view() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(sunrise_launcher_smod::mod_list::DIR);
    fs::create_dir_all(&folder).unwrap();
    let package = folder.join("example.smod");
    fs::write(&package, b"a").unwrap();
    let before = fingerprint(home.path(), None);
    fs::write(&package, b"ab").unwrap();
    assert_ne!(before, fingerprint(home.path(), None));
}

#[test]
fn github_and_local_files_are_listed_once_and_drive_build_state() {
    use crate::browser::{installed, Kind, SourceId};
    use std::io::Write;
    let home = std::env::temp_dir().join(format!("native-list-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join(native_mods::INSTALLED)).unwrap();
    let pin = "a".repeat(40);
    for (file, id) in [
        ("registry", "registry"),
        ("github", "github"),
        ("renamed", "local"),
    ] {
        let path = home
            .join(native_mods::INSTALLED)
            .join(format!("{file}.smod"));
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let manifest = format!(
            "id = '{id}'\nname = '{id}'\nversion = '1.2'\napi = '0.2'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nsunrise = '{pin}'\nmixins = '{pin}'\n"
        );
        for (name, bytes) in [
            ("sunrise-mod.toml", manifest.as_str()),
            (
                "Cargo.toml",
                "[package]\nname = 'unrelated'\nversion = '9'\n",
            ),
            ("src/lib.rs", "pub fn hooks() {}\n"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    let record = crate::github_mods::Origin {
        id: "github".into(),
        version: "1.2".into(),
        owner: "example".into(),
        repo: "mods".into(),
        tag: None,
        commit: pin,
        asset: None,
        source: None,
        added_from: None,
        sha256: sunrise_launcher_smod::index::sha256_file(
            &home.join(native_mods::INSTALLED).join("github.smod"),
        )
        .unwrap(),
    };
    fs::write(
        home.join(native_mods::INSTALLED).join("github.origin.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    installed::Installed {
        entries: vec![installed::Entry {
            source: SourceId::Sunrise,
            project: "registry".into(),
            mod_id: None,
            slug: "registry".into(),
            title: "Registry mod".into(),
            kind: Kind::Mod,
            version: "1.2".into(),
            number: "1.2".into(),
            file: format!("{}/registry.smod", native_mods::INSTALLED),
            checksum: String::new(),
            page: String::new(),
            dependency: false,
            verdict: None,
            install_hold: None,
            repository: None,
        }],
    }
    .save(&home)
    .unwrap();
    let files = untracked(&home);
    let seen: Vec<_> = files
        .iter()
        .map(|m| (m.id.as_str(), m.version.as_str(), m.origin.is_some()))
        .collect();
    assert_eq!(seen, [("github", "1.2", true), ("local", "1.2", false)]);
    assert!(files.iter().all(|m| matches!(&m.icon, Ok(None))));
    for id in ["registry", "github", "local"] {
        mod_list::set_enabled(&home, id, true).unwrap();
    }
    assert_eq!(state(&enabled(&home), None), State::NotBuilt);
    let mut pins = pinning(&home, &"a".repeat(40));
    pins.sort();
    assert_eq!(
        pins,
        ["github", "local", "registry"],
        "fit reads the enabled flat packages, including a renamed local file"
    );
    assert!(pinning(&home, &"b".repeat(40)).is_empty());
    for id in ["registry", "github", "local"] {
        mod_list::set_enabled(&home, id, false).unwrap();
    }
    assert_eq!(state(&enabled(&home), None), State::NoMods);
    assert!(
        bundles_on(&home).is_empty(),
        "disabled packages pin nothing"
    );
    assert_eq!(
        untracked(&home).len(),
        2,
        "disabled mods remain in the list"
    );
    let before = fingerprint(&home, None);
    fs::remove_file(home.join(&files[0].file)).unwrap();
    assert_ne!(before, fingerprint(&home, None));
    assert_eq!(
        untracked(&home).len(),
        1,
        "a stale origin is not an installed mod"
    );
    fs::write(
        home.join(native_mods::INSTALLED).join("broken.smod"),
        b"broken",
    )
    .unwrap();
    let files = untracked(&home);
    assert_eq!(
        files[0].id, "broken",
        "unreadable files can still be removed"
    );
    assert!(files[0].icon.is_err(), "a refusal is not an absent icon");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_build_made_in_another_mode_needs_a_build() {
    use native_mods::Profile::Optimized;
    assert_eq!(in_mode(State::UpToDate, None, Optimized), State::Outdated);
    assert_eq!(
        in_mode(State::UpToDate, Some(Optimized), Optimized),
        State::UpToDate
    );
    assert_eq!(in_mode(State::NoMods, None, Optimized), State::NoMods);
}

#[test]
fn a_build_is_needed_until_the_version_holds_the_mods_turned_on() {
    let on = [m("veinminer", "1.0"), m("zoom", "2.1")];
    assert_eq!(state(&[], None), State::NoMods);
    assert_eq!(
        state(&[], Some(&on)),
        State::NoMods,
        "nothing on: nothing to build"
    );
    assert_eq!(state(&on, None), State::NotBuilt, "not a native build");
    let reordered = [m("zoom", "2.1"), m("veinminer", "1.0")];
    assert_eq!(state(&on, Some(&reordered)), State::UpToDate);
    assert_eq!(
        state(&on, Some(&[m("veinminer", "1.0")])),
        State::Outdated,
        "a mod turned on since"
    );
    assert_eq!(
        state(&on[..1], Some(&on)),
        State::Outdated,
        "a mod turned off since"
    );
    assert_eq!(
        state(&on, Some(&[m("veinminer", "1.1"), m("zoom", "2.1")])),
        State::Outdated,
        "a mod updated since"
    );
}

#[test]
fn the_same_version_with_other_files_needs_a_build() {
    let on = [d("zoom", "2.1", "aa")];
    assert_eq!(state(&on, Some(&[d("zoom", "2.1", "aa")])), State::UpToDate);
    assert_eq!(
        state(&on, Some(&[d("zoom", "2.1", "bb")])),
        State::Outdated,
        "reinstalled without a version bump"
    );
    assert_eq!(
        state(&on, Some(&[m("zoom", "2.1")])),
        State::UpToDate,
        "a build from before the digest was kept"
    );
}

#[test]
fn native_mods_json_gives_the_built_mods() {
    let about = serde_json::json!({
        "base": "v0.1.0", "sunrise": "abc", "rust": "1.99.1",
        "mods": [{"id": "zoom", "version": "2.1"}, {"id": "x"}, {"id": "y", "version": "1", "sha256": "ab"}],
    });
    assert_eq!(parse_built(&about), [m("zoom", "2.1"), d("y", "1", "ab")]);
    assert!(parse_built(&serde_json::json!({})).is_empty());
    let about = serde_json::json!({
        "mods": [{"id": "zoom", "version": "2.1"}],
        "left_out": [{"id": "hitbox_plus", "version": "1.3.0", "sha256": "aa", "why": "other-version"}],
    });
    let build = parse_built(&about);
    let on = [m("zoom", "2.1"), d("hitbox_plus", "1.3.0", "aa")];
    assert_eq!(state(&on, Some(build.as_slice())), State::UpToDate);
    let on = [m("zoom", "2.1"), m("hitbox_plus", "1.3.1")];
    assert_eq!(state(&on, Some(build.as_slice())), State::Outdated);
    let on = [m("zoom", "2.1"), d("hitbox_plus", "1.3.0", "bb")];
    assert_eq!(state(&on, Some(build.as_slice())), State::Outdated);
    assert_eq!(built("v0.1.0"), None, "not a native build");
}

#[test]
fn launch_requires_a_complete_inventory_and_an_exact_enabled_match() {
    use serde_json::json;
    let hash = "a".repeat(64);
    let linked = json!({"id": "zoom", "version": "2.1", "sha256": hash});
    let included = parse_included(json!({
        "mods": [linked],
        "left_out": [{"id": "hitbox_plus", "version": "1.3.0"}],
    }))
    .unwrap();
    // Left-out mods don't run, so they don't need to be installed or enabled.
    assert_eq!(included.len(), 1);
    assert!(compiled_match(
        &included,
        &[("zoom".into(), "2.1".into(), Some(hash.clone()))]
    ));
    // An empty profile, missing digest, or wrong version or digest must not play the binary.
    assert!(!compiled_match(&included, &[]));
    for (version, sha) in [
        ("2.1", None),
        ("2.2", Some(hash)),
        ("2.1", Some("b".repeat(64))),
    ] {
        assert!(!compiled_match(
            &included,
            &[("zoom".into(), version.into(), sha)]
        ));
    }
    for record in [
        json!({}),
        json!({"mods": []}),
        json!({"mods": [{"id": "zoom", "version": "2.1"}]}),
        json!({"mods": [{"id": "zoom", "version": "2.1", "sha256": "xyz"}]}),
        json!({"mods": [linked, linked]}),
        json!({"mods": [linked, null]}),
    ] {
        assert!(parse_included(record).is_none());
    }
}

#[test]
fn builds_name_the_sunrise_they_were_built_on() {
    assert_eq!(sunrise_label("v0.2.93"), "v0.2.93");
    assert_eq!(
        sunrise_label("nightly-2026-10-29-4baa571"),
        "Nightly 2026-10-29"
    );
    assert_eq!(sunrise_label("nightly-2026-10-29"), "Nightly 2026-10-29");
    let build = Build {
        base: "nightly-2026-10-28-0c02067".into(),
        ..Build::default()
    };
    assert_eq!(build.sunrise(), "Nightly 2026-10-28");
    assert_eq!(build.sunrise_short(), "Nightly 10-28");
    let release = Build {
        base: "v0.2.93".into(),
        ..Build::default()
    };
    assert_eq!(release.sunrise_short(), "v0.2.93");
    assert_eq!(Build::read("v0.1.0"), None, "not a native build");
}

/// A build from before pins were recorded that compiled another commit than its Nightly's
#[test]
fn only_hmods_in_the_native_folder_are_native() {
    assert!(is_native_file("sunrise-native-mods/veinminer.smod"));
    assert!(!is_native_file("sunrise-native-mods-old/x.smod"));
    assert!(!is_native_file("sunrise-mods/zoom.smod"));
    assert!(!is_native_file("resourcepacks/x.zip"));
}
