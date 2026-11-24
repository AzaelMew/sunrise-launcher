use super::*;
use sunrise_launcher::browser::{installed::Entry, mod_list};

const GAME: &str = "26.3";

#[test]
fn source_links_only_trim_repositories_on_github() {
    for (source, expected) in [
        (
            "https://github.com/Hollowfen/Trail-Markers.git/tree/main/mods?tab=readme#install",
            Some(SourceLink::GitHub(
                "https://github.com/Hollowfen/Trail-Markers".into(),
            )),
        ),
        (
            "https://github.com/Hollowfen/Trail-Markers/blob/main/README.md",
            Some(SourceLink::GitHub(
                "https://github.com/Hollowfen/Trail-Markers".into(),
            )),
        ),
        (
            "https://github.com.example.test/owner/repo.git/tree/main?keep=1#readme",
            Some(SourceLink::Other(
                "https://github.com.example.test/owner/repo.git/tree/main?keep=1#readme".into(),
            )),
        ),
        (
            "https://code.example.test/owner/repo?keep=1#readme",
            Some(SourceLink::Other(
                "https://code.example.test/owner/repo?keep=1#readme".into(),
            )),
        ),
        ("https://github.com/owner", None),
        ("https://github.com/owner/../real/repo", None),
        ("https://@github.com/owner/repo", None),
        ("https://user:pass@github.com/owner/repo", None),
        ("http://github.com/owner/repo", None),
        ("https:///owner/repo", None),
        ("", None),
    ] {
        assert_eq!(SourceLink::read(source), expected, "{source}");
    }
}

fn project(
    id: &str,
    kind: &str,
    versions: &[(&str, &str, &str)],
    extra: serde_json::Value,
) -> serde_json::Value {
    let mut p = serde_json::json!({
        "id": id, "kind": kind, "name": id.to_uppercase(), "summary": format!("{id} does things"),
        "authors": ["Someone"], "license": "MIT", "source": "https://github.com/x/y",
        "versions": versions.iter().map(|(v, game, sha)| serde_json::json!({
            "version": v, "published": format!("2026-11-{:02}", v.len() + 10), "api": "0.2", "game": game,
            "file": {"name": format!("{id}-{v}.smod"), "url": "https://example.test/x.smod", "size": 2048, "sha256": sha}
        })).collect::<Vec<_>>(),
    });
    p.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    p
}

fn index(projects: Vec<serde_json::Value>, revoked: Vec<serde_json::Value>) -> Index {
    serde_json::from_value(
        serde_json::json!({"schema": 1, "projects": projects, "revoked": revoked}),
    )
    .unwrap()
}

fn entry(id: &str, version: &str, native: bool, sha: &str) -> Entry {
    let folder = if native {
        "sunrise-native-mods"
    } else {
        "sunrise-mods"
    };
    serde_json::from_value(serde_json::json!({
        "source": "sunrise", "project": id, "slug": id, "title": id.to_uppercase(), "kind": "mod",
        "version": version, "number": version, "file": format!("{folder}/{id}.smod"),
        "checksum": format!("SHA-256:{sha}"), "page": ""
    }))
    .unwrap()
}

#[test]
fn installed_source_label_distinguishes_catalogue_receipts_and_direct_github() {
    use sunrise_launcher::github_mods::{AddedFrom, Origin, SourceOrigin, SourceTrack};
    let idx = index(vec![], vec![]);
    let none = HashSet::new();
    let github = Origin {
        id: "lantern".into(),
        version: "1.0.0".into(),
        owner: "Workshop".into(),
        repo: "Lantern".into(),
        commit: "1".repeat(40),
        sha256: A.into(),
        tag: None,
        asset: None,
        source: None,
        added_from: None,
    };
    let locals = [Local {
        id: github.id.clone(),
        file: "sunrise-native-mods/lantern.smod".into(),
        name: "Lantern".into(),
        version: github.version.clone(),
        github: Some(github),
        native: true,
        served_by: None,
        unreadable: false,
    }];
    let mut row = rows(
        &[],
        &World {
            local: &locals,
            ..world(&idx, &none, &none)
        },
    )
    .remove(0);
    assert_eq!(row.source_label(), tr("From GitHub"));
    row.collection = Some(CollectionGroup {
        id: "collection-a".into(),
        name: "Collection A".into(),
    });
    let origin = row.local.as_mut().unwrap().github.as_mut().unwrap();
    origin.source = Some(SourceOrigin {
        repo_id: 42,
        manifest: "sunrise-mod.toml".into(),
        track: SourceTrack::Branch("main".into()),
        published_at: None,
    });
    assert_eq!(row.source_label(), tr("Source"));
    for (added_from, expected) in [
        (AddedFrom::Github, tr("From GitHub")),
        (AddedFrom::Discover, tr("Sunrise mods")),
        (
            AddedFrom::Collection {
                id: "collection-b".into(),
                name: "Collection B".into(),
            },
            "Collection B",
        ),
    ] {
        row.local
            .as_mut()
            .unwrap()
            .github
            .as_mut()
            .unwrap()
            .added_from = Some(added_from);
        assert_eq!(row.source_label(), expected);
    }
    row.local.as_mut().unwrap().github = None;
    assert_eq!(row.source_label(), tr("Local file"));
    let registry = rows(
        &[entry("registry", "1.0.0", true, A)],
        &world(&idx, &none, &none),
    );
    assert_eq!(registry[0].source_label(), tr("Sunrise mods"));
}

#[test]
fn installed_sections_keep_collection_members_together_and_leave_loose_rows_untitled() {
    let idx = index(vec![], vec![]);
    let none = HashSet::new();
    let entries = [
        entry("ant", "1.0.0", true, A),
        entry("bird", "1.0.0", true, A),
        entry("cat", "1.0.0", true, A),
        entry("dog", "1.0.0", true, A),
    ];
    let mut rows = rows(&entries, &world(&idx, &none, &none));
    let collection = |id: &str| {
        Some(CollectionGroup {
            id: id.into(),
            name: "Wildlife".into(),
        })
    };
    rows[0].collection = collection("park");
    rows[2].collection = collection("park");
    rows[3].collection = collection("zoo");
    rows[2].need = Some(Need::Build);
    group_installed(&mut rows);
    let visible = visible(&rows, &RowFilter::default());
    assert_eq!(
        installed_groups(&rows, &visible).collect::<Vec<_>>(),
        [
            (Some("Wildlife"), &[0, 1][..]),
            (Some("Wildlife"), &[2][..]),
            (None, &[3][..])
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["cat", "ant", "dog", "bird"]
    );
    assert_eq!(
        installed_groups(&rows, &[3]).collect::<Vec<_>>(),
        [(None, &[3][..])]
    );
}

#[test]
fn installed_filters_hide_zero_counts_and_drop_a_hidden_selection() {
    let idx = index(vec![], vec![]);
    let on = HashSet::from(["lantern".into()]);
    let rows = rows(
        &[entry("lantern", "1.0.0", true, A)],
        &world(&idx, &on, &HashSet::new()),
    );
    let mut filter = RowFilter {
        show: Show::Off,
        makes: BTreeSet::from([Make::Content]),
        ..RowFilter::default()
    };
    let counts = counted_filters(&rows, &mut filter);
    assert_eq!(counts.shows, [(Show::All, 1), (Show::On, 1)]);
    assert_eq!(counts.kinds, [(Make::Native, 1)]);
    assert_eq!(filter, RowFilter::default());
    assert_eq!(visible(&rows, &filter), [0]);
}

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static NO_PINS: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
    std::sync::LazyLock::new(Default::default);
static NO_UNFIT_UPDATES: BTreeSet<String> = BTreeSet::new();

fn world<'a>(
    index: &'a Index,
    enabled: &'a HashSet<String>,
    unbuilt: &'a HashSet<String>,
) -> World<'a> {
    World {
        index: Some(index),
        game: GAME,
        v2: None,
        build: None,
        unfit_updates: &NO_UNFIT_UPDATES,
        enabled,
        unbuilt,
        chosen: &NO_PINS,
        local: &[],
    }
}

#[test]
fn discover_lists_mods_not_packs_and_says_which_fit() {
    let idx = index(
        vec![
            project(
                "zoom",
                "mod",
                &[("1.0.0", GAME, A)],
                serde_json::json!({"downloads": 50, "categories": ["utility"]}),
            ),
            project(
                "deep",
                "native-mod",
                &[("1.0.0", "25.1", A)],
                serde_json::json!({"downloads": 900}),
            ),
            project(
                "cozy",
                "modpack",
                &[("1.0.0", GAME, A)],
                serde_json::json!({}),
            ),
        ],
        vec![],
    );
    let cards = cards(&idx, GAME);
    assert_eq!(
        cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["zoom", "deep"],
        "no modpack"
    );
    assert!(cards[0].fit.fits() && cards[0].make == Make::Content);
    assert_eq!(cards[1].fit, Fit::OtherBuild("25.1".into()));
    assert_eq!(cards[1].make, Make::Native);

    let mut f = Filters::default();
    assert_eq!(discover(&cards, &f), [0], "Fits my game is on by default");
    f.fits_only = false;
    assert_eq!(discover(&cards, &f), [1, 0], "most downloaded first");
    f.make = Some(Make::Content);
    assert_eq!(discover(&cards, &f), [0]);
    f.make = None;
    f.category = Some("utility".into());
    assert_eq!(discover(&cards, &f), [0]);
    f.category = None;
    f.text = "DEEP".into();
    assert_eq!(discover(&cards, &f), [1]);
    f.text = "nothing like it".into();
    assert!(discover(&cards, &f).is_empty());
}

#[test]
fn search_matches_any_case_in_every_field_a_card_shows() {
    let mut p = project("ender", "mod", &[("1.0.0", GAME, A)], serde_json::json!({}));
    p["name"] = "Über Höhlen".into();
    p["summary"] = "Lights the DEEP caves".into();
    p["authors"] = serde_json::json!(["Ärne Smith"]);
    let cards = cards(&index(vec![p], vec![]), GAME);
    let mut f = Filters::default();
    for text in [
        "ENDER",
        "über höhlen",
        "ÜBER",
        "deep caves",
        "ärne",
        "smith ender",
    ] {
        f.text = text.into();
        assert_eq!(discover(&cards, &f), [0], "{text}");
    }
    f.text = "über nothing".into();
    assert!(discover(&cards, &f).is_empty(), "every word has to match");
}

#[test]
fn a_taken_down_version_is_refused_with_the_registrys_reason() {
    let idx = index(
        vec![project(
            "zoom",
            "mod",
            &[("1.0.0", GAME, A)],
            serde_json::json!({}),
        )],
        vec![
            serde_json::json!({"id": "zoom", "sha256": A, "reason": "it crashed worlds", "date": "2026-11-01"}),
        ],
    );
    let c = &cards(&idx, GAME)[0];
    assert_eq!(c.fit, Fit::Refused("Taken down: it crashed worlds".into()));
    let page = page(&idx, "zoom", GAME, vec![], false).unwrap();
    assert!(page.will_install.is_none(), "nothing to Add");
    assert!(!page.versions[0].fit.fits());
}

#[test]
fn the_page_names_the_version_add_installs() {
    let idx = index(
        vec![project(
            "zoom",
            "mod",
            &[("2.0.0", GAME, B), ("1.0.0", "25.1", A)],
            serde_json::json!({"downloads": 7}),
        )],
        vec![],
    );
    let p = page(&idx, "zoom", GAME, vec!["Made by Someone".into()], false).unwrap();
    assert_eq!(
        p.will_install.as_ref().map(|v| v.number.as_str()),
        Some("2.0.0")
    );
    assert_eq!(p.versions[1].fit, Fit::OtherBuild("25.1".into()));
    assert_eq!(p.about.unwrap().side, "Not needed on servers");
    assert!(page(&idx, "missing", GAME, vec![], false).is_none());
}

#[test]
fn an_update_is_only_ever_forward_to_what_fits() {
    let idx = index(
        vec![project(
            "zoom",
            "mod",
            &[("3.0.0", GAME, B), ("2.0.0", GAME, A), ("1.0.0", GAME, A)],
            serde_json::json!({}),
        )],
        vec![],
    );
    let p = idx.project("zoom").unwrap();
    assert_eq!(
        newer(&idx, p, "2.0.0", GAME, &NO_UNFIT_UPDATES),
        Some("3.0.0".into())
    );
    assert_eq!(newer(&idx, p, "3.0.0", GAME, &NO_UNFIT_UPDATES), None);
    assert_eq!(
        newer(&idx, p, "0.9.0", GAME, &NO_UNFIT_UPDATES),
        None,
        "a version the index doesn't list is left alone"
    );
    assert_eq!(
        newer_for_hold(&idx, p, "0.9.0", GAME, &NO_UNFIT_UPDATES),
        Some("3.0.0".into()),
        "a typed hold can offer a reviewed replacement for a delisted older archive"
    );
    assert_eq!(
        newer(&idx, p, "2.0.0", "25.1", &NO_UNFIT_UPDATES),
        None,
        "nothing for another build"
    );
}

#[test]
fn known_unfit_archives_are_not_offered_as_updates() {
    let idx = index(
        vec![project(
            "zoom",
            "native-mod",
            &[("3.0.0", GAME, B), ("2.0.0", GAME, A), ("1.0.0", GAME, A)],
            serde_json::json!({}),
        )],
        vec![],
    );
    let on = HashSet::from(["zoom".to_owned()]);
    let unbuilt = HashSet::new();
    let known = BTreeSet::from([B.to_owned()]);
    let w = World {
        unfit_updates: &known,
        ..world(&idx, &on, &unbuilt)
    };
    let latest = rows(&[entry("zoom", "2.0.0", true, A)], &w).remove(0);
    assert!(latest.on);
    assert!(latest.update.is_none() && !bulk_updatable(&latest));
    let older = rows(&[entry("zoom", "1.0.0", true, A)], &w).remove(0);
    assert_eq!(older.update.as_deref(), Some("2.0.0"));
    assert!(bulk_updatable(&older));
    assert_eq!(
        newer_for_hold(&idx, idx.project("zoom").unwrap(), "0.9.0", GAME, &known).as_deref(),
        Some("2.0.0")
    );
}

fn pack() -> (Index, Vec<Entry>) {
    let idx = index(
        vec![
            project(
                "zoom",
                "mod",
                &[("2.0.0", GAME, B), ("1.0.0", GAME, A)],
                serde_json::json!({}),
            ),
            project(
                "bad",
                "mod",
                &[("2.0.0", GAME, B), ("1.0.0", GAME, A)],
                serde_json::json!({}),
            ),
            project(
                "deep",
                "native-mod",
                &[("2.0.0", GAME, B), ("1.0.0", GAME, A)],
                serde_json::json!({}),
            ),
            project("quiet", "mod", &[("1.0.0", GAME, A)], serde_json::json!({})),
        ],
        vec![
            serde_json::json!({"id": "bad", "sha256": A, "reason": "unsafe", "date": "2026-11-01"}),
        ],
    );
    let entries = vec![
        entry("quiet", "1.0.0", false, A),
        entry("zoom", "1.0.0", false, A),
        entry("deep", "1.0.0", true, A),
        entry("bad", "1.0.0", false, A),
    ];
    (idx, entries)
}

#[test]
fn a_held_source_is_grouped_with_rows_that_need_attention() {
    let (idx, entries) = pack();
    let enabled = HashSet::new();
    let unbuilt = HashSet::new();
    let mut assembled = rows(&entries, &world(&idx, &enabled, &unbuilt));
    let quiet = assembled.iter_mut().find(|row| row.id == "quiet").unwrap();
    assert_eq!(quiet.group(), Group::Rest);
    quiet.held_for = Some(Hold {
        build: "nightly-test".into(),
        reason: sunrise_launcher::managed_play::HeldReason::NoFit {
            build: "nightly-test".into(),
            detail: None,
        },
        retry: false,
    });
    assert_eq!(quiet.group(), Group::NeedsYou);
    // A registry hold waits for its author: its switch is out of reach and dimmed.
    let waiting = cells(quiet, None, None);
    assert!(!waiting.switch && waiting.unavailable);
    // Local or source bytes may be tried again: the switch works and isn't dimmed as dropped.
    quiet.held_for.as_mut().unwrap().retry = true;
    let retryable = cells(quiet, None, None);
    assert!(retryable.switch && !retryable.unavailable);
}

#[test]
fn rows_put_what_needs_the_player_first_then_updates_then_the_rest() {
    let (idx, entries) = pack();
    let enabled: HashSet<String> = ["quiet", "zoom", "deep", "bad"].map(String::from).into();
    let unbuilt: HashSet<String> = ["deep".to_string()].into();
    let r = rows(&entries, &world(&idx, &enabled, &unbuilt));
    let order: Vec<(&str, Group)> = r.iter().map(|r| (r.id.as_str(), r.group())).collect();
    assert_eq!(
        order,
        [
            ("bad", Group::NeedsYou),
            ("deep", Group::NeedsYou),
            ("zoom", Group::Updates),
            ("quiet", Group::Rest)
        ]
    );
    assert_eq!(r[0].need, Some(Need::TakenDown("unsafe".into())));
    assert_eq!(
        r[0].update.as_deref(),
        Some("2.0.0"),
        "the way out of a takedown is shown too"
    );
    assert_eq!(r[1].need, Some(Need::Build));
    assert_eq!(r[1].make, Some(Make::Native));
    assert_eq!(r[2].update.as_deref(), Some("2.0.0"));
}

#[test]
fn registry_installed_rows_use_live_names_and_statuses_or_ids_and_not_checked() {
    let (idx, mut entries) = pack();
    let bad = entries.iter_mut().find(|e| e.project == "bad").unwrap();
    bad.title = "Unverified saved name".into();
    let none = HashSet::new();
    let live = world(&idx, &none, &none);
    let row = rows(std::slice::from_ref(bad), &live).pop().unwrap();
    assert_eq!(row.title, "BAD");
    assert_eq!(row.authors, "Someone");
    assert_eq!(row.need, Some(Need::TakenDown("unsafe".into())));
    assert_eq!(row.update.as_deref(), Some("2.0.0"));
    assert!(!row.registry_unchecked);

    let offline = World {
        index: None,
        ..live
    };
    let row = rows(std::slice::from_ref(bad), &offline).pop().unwrap();
    assert_eq!(row.title, row.id);
    assert!(row.authors.is_empty());
    assert!(row.icon.is_none());
    assert!(row.need.is_none());
    assert!(row.update.is_none());
    assert!(row.registry_unchecked);
}

#[test]
fn native_code_that_is_on_and_unbuilt_asks_to_build() {
    let (idx, entries) = pack();
    let enabled: HashSet<String> = ["deep".to_string()].into();
    let unbuilt: HashSet<String> = ["deep".to_string()].into();
    let r = rows(&entries, &world(&idx, &enabled, &unbuilt));
    let deep = r.iter().find(|r| r.id == "deep").unwrap();
    assert_eq!(deep.need, Some(Need::Build));
}

#[test]
fn only_registry_mods_are_rows_packs_are_left_alone() {
    let (idx, mut entries) = pack();
    let mut other = entry("shaderish", "1", false, A);
    other.kind = sunrise_launcher::browser::Kind::Shader;
    entries.push(other);
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &none, &none));
    assert_eq!(r.len(), 4);
    assert!(r.iter().all(|r| r.id != "shaderish"));
}

#[test]
fn filters_narrow_the_shown_rows_and_never_reorder_them() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["quiet", "zoom"].map(String::from).into();
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &on, &none));
    let ids = |f: &RowFilter| {
        visible(&r, f)
            .into_iter()
            .map(|i| r[i].id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&RowFilter::default()), ["bad", "deep", "zoom", "quiet"]);
    assert_eq!(
        ids(&RowFilter {
            show: Show::On,
            ..Default::default()
        }),
        ["zoom", "quiet"]
    );
    assert_eq!(
        ids(&RowFilter {
            show: Show::Off,
            ..Default::default()
        }),
        ["bad", "deep"]
    );
    assert_eq!(
        ids(&RowFilter {
            makes: [Make::Native].into(),
            ..Default::default()
        }),
        ["deep"]
    );
    assert_eq!(
        ids(&RowFilter {
            text: "QUI".into(),
            ..Default::default()
        }),
        ["quiet"]
    );
}

#[test]
fn update_all_includes_off_and_native_rows_but_not_a_player_pin() {
    let (idx, entries) = pack();
    let none = HashSet::new();
    let on: HashSet<String> = ["zoom".to_string()].into();
    let unpinned = world(&idx, &on, &none);
    let offered = rows(&entries, &unpinned);
    let updates = offered
        .iter()
        .filter(|r| bulk_updatable(r))
        .map(|r| r.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(updates, ["bad", "deep", "zoom"]);
    assert!(offered
        .iter()
        .any(|r| r.id == "bad" && !r.on && bulk_updatable(r)));

    let pins = std::collections::BTreeMap::from([("zoom".into(), "1.0.0".into())]);
    let pinned = rows(
        &entries,
        &World {
            chosen: &pins,
            ..unpinned
        },
    );
    assert!(pinned
        .iter()
        .any(|r| r.id == "zoom" && r.update.is_some() && !bulk_updatable(r)));
}

#[test]
fn a_row_the_plan_cannot_update_says_why() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["zoom"].map(String::from).into();
    let none = HashSet::new();
    let pins = std::collections::BTreeMap::from([("zoom".to_string(), "1.0.0".to_string())]);
    let w = World {
        chosen: &pins,
        ..world(&idx, &on, &none)
    };
    let r = rows(&entries, &w);
    let zoom = r.iter().find(|r| r.id == "zoom").unwrap();
    assert!(!bulk_updatable(zoom));
    let tip = cells(zoom, None, None).status.expect("says something").tip;
    assert!(tip.contains("You chose its version"), "{tip}");
}

#[test]
fn native_files_the_browser_didnt_install_are_listed_and_never_planned() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["mine", "zoom", "by_hand"].map(String::from).into();
    let none = HashSet::new();
    let local = [
        Local {
            id: "mine".into(),
            file: "mine".to_owned(),
            name: "mine".into(),
            version: "0.3".into(),
            github: None,
            native: true,
            served_by: None,
            unreadable: false,
        },
        Local {
            id: "by_hand".into(),
            file: "by_hand".to_owned(),
            name: "By Hand".into(),
            version: "1.0".into(),
            github: None,
            native: false,
            served_by: None,
            unreadable: false,
        },
    ];
    let w = World {
        local: &local,
        ..world(&idx, &on, &none)
    };
    let r = rows(&entries, &w);
    let mine = r.iter().find(|r| r.id == "mine").expect("listed");
    assert!(mine.local.is_some() && mine.entry.is_none() && mine.update.is_none());
    assert_eq!(mine.need, None, "it is built and needs no permission");
    // A package of `sunrise-mods/` put there by hand is listed by its name and needs neither
    // permission nor a build.
    let content = r.iter().find(|r| r.id == "by_hand").expect("listed");
    assert_eq!((content.title.as_str(), content.on), ("By Hand", true));
    assert_eq!((content.make, &content.need), (Some(Make::Content), &None));
    let mut filter = RowFilter {
        makes: BTreeSet::from([Make::Content]),
        ..RowFilter::default()
    };
    let counts = counted_filters(&r, &mut filter);
    let contents = visible(&r, &filter);
    assert!(contents.iter().any(|&i| r[i].id == "by_hand"));
    assert!(contents.iter().all(|&i| r[i].make == Some(Make::Content)));
    assert!(counts.kinds.contains(&(Make::Content, contents.len())));
    assert!(content.local.is_some() && content.entry.is_none());
    assert!(!bulk_updatable(mine));
    let shown = visible(&r, &RowFilter::default());
    assert_eq!(shown.len(), 6, "four managed mods and two hand-added mods");
    assert_eq!(visible_count(&r, &shown), "6 of 6 mods shown");
}

#[test]
fn turning_on_a_local_native_file_exposes_its_build() {
    let home = tempfile::tempdir().unwrap();
    let on_list = home.path();
    let id = "example";
    let local = [Local {
        id: id.into(),
        file: format!("{id}.smod"),
        name: "Example".into(),
        version: "1.0".into(),
        github: None,
        native: true,
        served_by: None,
        unreadable: false,
    }];
    let unbuilt = HashSet::from([id.into()]);
    let row = || {
        let enabled = if mod_list::is_enabled(on_list, id) {
            HashSet::from([id.into()])
        } else {
            HashSet::new()
        };
        rows(
            &[],
            &World {
                index: None,
                game: GAME,
                v2: None,
                build: None,
                unfit_updates: &NO_UNFIT_UPDATES,
                enabled: &enabled,
                unbuilt: &unbuilt,
                chosen: &NO_PINS,
                local: &local,
            },
        )
        .remove(0)
    };
    let off = row();
    assert!(!off.on);
    assert!(cells(&off, None, None).switch);
    mod_list::set_enabled(on_list, id, true).unwrap();
    let on = row();
    assert!(on.on);
    assert_eq!(on.need, Some(Need::Build));
}

#[test]
fn turning_on_skips_what_is_taken_down() {
    let (idx, entries) = pack();
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &none, &none));
    let all: Vec<&Row> = r.iter().collect();
    let (ok, held) = turn_on_plan(&all);
    assert_eq!(ok.len(), 3);
    assert_eq!(held.len(), 1, "taken down stays held");
}

#[test]
fn counts_read_short() {
    assert_eq!(size(2048), "2.0 KiB");
    assert_eq!(size(3 << 20), "3.0 MiB");
    assert!(downloads(12_400).starts_with("12.4k"));
    assert!(downloads(7).starts_with("7 "));
}

#[test]
fn every_reason_a_build_left_a_mod_out_is_shown_and_counted() {
    use native_mods::{Detail, LeftOut, Why};
    let (idx, entries) = pack();
    let on: HashSet<String> = ["lib", "addon", "old", "fine"].map(String::from).into();
    let none = HashSet::new();
    let local = ["lib", "addon", "old", "fine"].map(|id| Local {
        id: id.into(),
        file: format!("{id}.smod"),
        name: id.into(),
        version: "1.0".into(),
        github: None,
        native: true,
        served_by: None,
        unreadable: false,
    });
    let w = World {
        local: &local,
        ..world(&idx, &on, &none)
    };
    let mut r = rows(&entries, &w);
    let left = |id: &str, why| LeftOut {
        id: id.into(),
        version: "1.0".into(),
        why,
        detail: None,
    };
    let list = [
        left("lib", Why::DidNotBuild),
        left("addon", Why::Needs("lib".into())),
        LeftOut {
            detail: Some(Detail {
                made_for: Some("nightly-2026-11-16-abc".into()),
                changes: vec!["a hook moved".into()],
            }),
            ..left("old", Why::Unfit("made for Nov 16".into()))
        },
    ];
    mark_left_out(&mut r, &list, "Nov 17", |tag| format!("name of {tag}"));
    let row = |id: &str| r.iter().find(|r| r.id == id).unwrap();
    let failed = cells(row("lib"), None, None);
    let status = failed.status.unwrap();
    assert_eq!(status.label, "Failed");
    assert!(status.tip.contains("lib"));
    assert!(failed.unavailable);
    assert!(
        matches!(&row("addon").left_out, Some(Omission::Skipped(why)) if why.contains("needs lib"))
    );
    assert!(row("lib").unfit.is_none() && row("lib").need.is_none());
    let old = row("old").unfit.as_ref().unwrap();
    assert_eq!(
        old.made_for,
        Some((
            "nightly-2026-11-16-abc".into(),
            "name of nightly-2026-11-16-abc".into()
        ))
    );
    assert!(row("old").left_out.is_none());
    assert!(row("fine").unfit.is_none() && row("fine").left_out.is_none());
}

#[test]
fn every_row_fills_the_same_columns_whatever_it_has_to_say() {
    use native_mods::{Detail, LeftOut, Why};
    let (idx, entries) = pack();
    let on: HashSet<String> = ["lib", "addon", "old", "fine"].map(String::from).into();
    let unbuilt: HashSet<String> = ["addon".to_string()].into();
    let local = ["lib", "addon", "old", "fine"].map(|id| Local {
        id: id.into(),
        file: format!("{id}.smod"),
        name: id.into(),
        version: "1.0".into(),
        github: (id == "lib").then(|| sunrise_launcher::github_mods::Origin {
            id: id.to_owned(),
            version: "1.0".into(),
            owner: "publisher".into(),
            repo: "source".into(),
            tag: Some("v1.0".into()),
            commit: "a".repeat(40),
            asset: Some("lib.smod".into()),
            source: None,
            added_from: None,
            sha256: "b".repeat(64),
        }),
        native: true,
        served_by: None,
        unreadable: false,
    });
    let w = World {
        local: &local,
        ..world(&idx, &on, &unbuilt)
    };
    let mut r = rows(&entries, &w);
    mark_left_out(
        &mut r,
        &[LeftOut {
            id: "old".into(),
            version: "1.0".into(),
            why: Why::Unfit("made for Nov 16".into()),
            detail: Some(Detail {
                made_for: None,
                changes: vec!["src/lib.rs:3 builds it".into()],
            }),
        }],
        "Nov 17",
        |tag| tag.to_owned(),
    );
    let row = |id: &str| r.iter().find(|r| r.id == id && r.local.is_some()).unwrap();
    let github_details = row("lib").installed_details();
    assert_eq!(
        github_details.source,
        Some(SourceLink::GitHub(
            "https://github.com/publisher/source".into()
        ))
    );
    assert_eq!(github_details.fit, Fit::Unknown);
    assert!(github_details.about.is_none() && github_details.will_install.is_none());
    assert_eq!(row("addon").installed_details().source, None);

    // Every row has a kind and a version for the same two columns.
    for row in &r {
        let cells = cells(row, None, None);
        assert_eq!(cells.version, row.version);
        assert_eq!(cells.kind, row.make.map_or("", Make::label));
    }
    // A mod that needs an update keeps the switch's place, and its status opens the popover:
    // no file or type name in the label or the tooltip.
    let old = cells(row("old"), None, None);
    let status = old.status.unwrap();
    assert_eq!(status.label, "Needs an update");
    assert_eq!(status.tone, Tone::Warning);
    assert!(old.popover);
    assert!(!status.tip.contains("src/lib.rs"), "{}", status.tip);
    assert_eq!(old.action, None);
    // A mod that waits for a build says so, and says Building while its setup's build runs.
    let addon = row("addon");
    assert_eq!(addon.need, Some(Need::Build));
    assert_eq!(
        cells(addon, None, None).status.unwrap().label,
        "Added · builds when you play"
    );
    for (at, label, tone) in [
        (BuildState::Waiting, "Waiting", Tone::Muted),
        (BuildState::Building, "Building", Tone::Accent),
        (BuildState::Built, "Built", Tone::Success),
        (
            BuildState::Failed("Missing hook".into()),
            "Failed",
            Tone::Warning,
        ),
        (
            BuildState::NeedsUpdate {
                why: "No build for this game".into(),
                details: Vec::new(),
            },
            "Needs an update",
            Tone::Warning,
        ),
    ] {
        let cells = cells(addon, None, Some(&at));
        let status = cells.status.unwrap();
        assert_eq!((status.label.as_str(), status.tone), (label, tone));
        match at {
            BuildState::Failed(why) | BuildState::NeedsUpdate { why, .. } => {
                assert!(status.tip.contains(&why));
                assert!(
                    cells.unavailable && cells.switch,
                    "can still turn the mod off"
                );
            }
            _ => assert!(!cells.unavailable),
        }
    }
    // The source is in the byline; it doesn't take up the status column.
    let fine = cells(row("fine"), None, None);
    assert!(fine.status.is_none());
    assert_eq!(fine.action, None);
    assert!(fine.switch, "local files can be switched on and off");
    // A registry row with nothing to say: empty status and action cells, a live switch.
    let plain = r
        .iter()
        .find(|r| r.local.is_none() && r.need.is_none() && r.update.is_none());
    if let Some(plain) = plain {
        let cells = cells(plain, None, None);
        assert_eq!(
            (cells.status, cells.action, cells.switch),
            (None, None, true)
        );
    }
    // The most pressing thing is the status; the rest is in its tooltip. The one button is
    // Update whenever a newer version is out.
    let mut both = row("addon").clone();
    both.update = Some("1.1".into());
    let cells_both = cells(&both, Some("the download stopped"), None);
    let status = cells_both.status.unwrap();
    assert_eq!(status.label, "Update failed");
    assert!(status.tip.contains("the download stopped"));
    assert!(status.tip.contains("Not built for this computer yet"));
    assert!(status.tip.contains("Version 1.1 is out"));
    assert_eq!(cells_both.action, Some(RowAction::Update));
}

// ---------------------------------------------------------------- collections

fn needs(id: &str, range: &str) -> Option<Vec<index::Dependency>> {
    Some(vec![index::Dependency {
        id: id.into(),
        version: range.into(),
        optional: false,
        side: None,
    }])
}

/// A collection of `cow`, `pig` and `owl`, with `owl` needing `cow`, and `cow` and `pig` needing
/// `lib`, which no collection lists; and `jei`, which belongs to none.
fn zoo() -> Index {
    let mut idx = index(
        ["lib", "cow", "pig", "owl", "jei"]
            .map(|id| {
                project(
                    id,
                    "native-mod",
                    &[("1.0.0", GAME, A)],
                    match id {
                        "lib" => serde_json::json!({"downloads": 10, "categories": ["library"]}),
                        _ => serde_json::json!({"downloads": 10}),
                    },
                )
            })
            .to_vec(),
        vec![],
    );
    let mut dependencies = |id: &str, dep: Option<Vec<index::Dependency>>| {
        idx_version(&mut idx, id).dependencies = dep;
    };
    dependencies("cow", needs("lib", "^1"));
    dependencies("pig", needs("lib", "^1"));
    dependencies("owl", needs("cow", "^1"));
    idx.collections = vec![serde_json::from_value(serde_json::json!({
        "id": "farm", "name": "Farm", "summary": "Animals.", "owners": ["Someone"],
        "icon": "https://example.test/farm.png",
        "members": [{"id": "cow"}, {"id": "pig"}, {"id": "owl", "default": false}],
    }))
    .unwrap()];
    idx
}

fn idx_version<'a>(idx: &'a mut Index, id: &str) -> &'a mut index::Version {
    &mut idx
        .projects
        .iter_mut()
        .find(|p| p.id == id)
        .unwrap()
        .versions[0]
}

fn ticked(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn farm(idx: &Index, picks: &[&str]) -> collection::View {
    let c = idx.collection("farm").unwrap();
    let base = collection::base(idx, c, &[], GAME);
    collection::view(idx, c, &base, &ticked(picks), &[], GAME)
}

#[test]
fn discover_folds_members_into_the_collection_and_hides_libraries_until_searched() {
    let idx = zoo();
    let all = cards(&idx, GAME);
    let names = |f: &Filters| -> Vec<String> {
        discover(&all, f)
            .into_iter()
            .map(|i| all[i].id.clone())
            .collect()
    };
    let mut f = Filters {
        sort: Sort::Name,
        ..Filters::default()
    };
    assert_eq!(
        names(&f),
        ["farm", "jei"],
        "the card, not its three mods or the library"
    );
    let farm = all.iter().find(|c| c.id == "farm").unwrap();
    assert_eq!(farm.members, Some(3));
    assert_eq!(
        farm.downloads, 30,
        "no count of its own published: its mods' downloads added up, never 0"
    );
    assert!(all.iter().find(|c| c.id == "lib").unwrap().library);
    assert!(!all.iter().find(|c| c.id == "jei").unwrap().library);
    // A search finds a member and a library like any other mod.
    f.text = "cow".into();
    assert_eq!(names(&f), ["cow"]);
    f.text = "lib".into();
    assert_eq!(names(&f), ["lib"]);
    f.text = "farm".into();
    assert_eq!(names(&f), ["farm"]);
}

#[test]
fn a_collection_shows_its_own_downloads_once_the_registry_counts_them() {
    let mut idx = zoo();
    idx.collections[0].downloads = Some(5);
    let all = cards(&idx, GAME);
    let farm = all.iter().find(|c| c.id == "farm").unwrap();
    assert_eq!(
        farm.downloads, 5,
        "the collection's own count, not its members' sum"
    );
    assert_eq!(collection::downloads(&idx.collections[0], &idx), 5);
    // A published 0 is a real count, unlike a missing one.
    idx.collections[0].downloads = Some(0);
    assert_eq!(collection::downloads(&idx.collections[0], &idx), 0);
}

#[test]
fn a_members_sum_that_passes_u64_max_stops_there() {
    let mut idx = zoo();
    assert_eq!(idx.collections[0].downloads, None);
    for id in ["cow", "pig"] {
        idx.projects
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .downloads = u64::MAX;
    }
    assert_eq!(collection::downloads(&idx.collections[0], &idx), u64::MAX);
    let all = cards(&idx, GAME);
    let farm = all.iter().find(|c| c.id == "farm").unwrap();
    assert_eq!(farm.downloads, u64::MAX, "neither a panic nor a wrap");
}

#[test]
fn a_member_says_which_collection_it_is_part_of() {
    let idx = zoo();
    let all = cards(&idx, GAME);
    let part = |id: &str| all.iter().find(|c| c.id == id).unwrap().part_of.clone();
    assert_eq!(part("cow").as_deref(), Some("Farm"));
    assert_eq!(part("owl").as_deref(), Some("Farm"));
    assert_eq!(part("lib"), None, "a library is in no collection");
    assert_eq!(part("jei"), None);
    assert_eq!(part("farm"), None);
}

#[test]
fn the_header_box_is_empty_dashed_or_ticked_by_how_many_members_are() {
    assert_eq!(collection::ticks(0, 3), collection::Ticks::None);
    assert_eq!(collection::ticks(1, 3), collection::Ticks::Some);
    assert_eq!(collection::ticks(3, 3), collection::Ticks::All);
    let idx = zoo();
    assert_eq!(farm(&idx, &[]).header, collection::Ticks::None);
    assert_eq!(farm(&idx, &["pig"]).header, collection::Ticks::Some);
    assert_eq!(
        farm(&idx, &["cow", "pig", "owl"]).header,
        collection::Ticks::All
    );

    // Clicking it ticks all but a library, or clears them all.
    let v = farm(&idx, &["pig"]);
    let mut t = ticked(&["pig"]);
    v.flip_all(&mut t);
    assert_eq!(t, ticked(&["cow", "pig", "owl"]));
    farm(&idx, &["cow", "pig", "owl"]).flip_all(&mut t);
    assert!(t.is_empty());
}

#[test]
fn what_a_ticked_mod_needs_is_ticked_and_locked_while_it_is_needed() {
    let idx = zoo();
    let v = farm(&idx, &["cow", "pig", "owl"]);
    let line =
        |v: &collection::View, id: &str| v.lines.iter().find(|l| l.id == id).unwrap().clone();
    assert_eq!(
        v.lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        ["cow", "pig", "owl", "lib"],
        "the members in order, then the library"
    );
    let lib = line(&v, "lib");
    assert!(lib.library && lib.ticked);
    assert_eq!(
        lib.needed_by,
        ["COW", "PIG", "OWL"],
        "in the collection's order, and including what needs it through another pick"
    );
    let cow = line(&v, "cow");
    assert!(
        cow.ticked && cow.locked(),
        "a member another pick needs is locked too"
    );
    assert_eq!(cow.needed_by, ["OWL"]);
    assert!(!line(&v, "owl").locked());

    // A locked box does not flip; its dependent's does, and the lock goes with it.
    let mut t = ticked(&["cow", "pig", "owl"]);
    v.flip(&mut t, "cow");
    v.flip(&mut t, "lib");
    assert_eq!(t, ticked(&["cow", "pig", "owl"]));
    v.flip(&mut t, "owl");
    let v = farm(&idx, &["cow", "pig"]);
    assert!(!line(&v, "cow").locked());
    assert_eq!(line(&v, "lib").needed_by, ["COW", "PIG"]);
    let v = farm(&idx, &[]);
    let lib = line(&v, "lib");
    assert!(!lib.ticked && !lib.locked(), "nothing ticked needs it");
}

#[test]
fn the_plan_line_counts_the_plans_mods_and_libraries() {
    let say = |mods, libraries, updates| {
        collection::Summary {
            mods,
            libraries,
            updates,
            bytes: 0,
        }
        .line()
    };
    assert_eq!(say(8, 1, 0), "Installs 8 mods and 1 library");
    assert_eq!(say(1, 0, 0), "Installs 1 mod");
    assert_eq!(say(0, 2, 0), "Installs 2 libraries");
    assert_eq!(
        say(2, 1, 1),
        "Installs 2 mods and 1 library. Updates 1 installed mod"
    );
    assert_eq!(say(0, 0, 0), "Everything you picked is installed.");

    // From a real plan: two members, their library once, and nothing once they are held.
    let idx = zoo();
    let v = farm(&idx, &["cow", "pig"]);
    assert_eq!(v.summary.line(), "Installs 2 mods and 1 library");
    assert_eq!(v.summary.bytes, 3 * 2048);
    let held = ["cow", "pig", "lib"].map(|id| index::resolve::InstalledMod {
        id: id.into(),
        version: "1.0.0".into(),
        sha256: Some(A.into()),
    });
    let c = idx.collection("farm").unwrap();
    let base = collection::base(&idx, c, &held, GAME);
    let v = collection::view(&idx, c, &base, &ticked(&["cow", "pig"]), &held, GAME);
    assert!(v.summary.nothing(), "{:?}", v.summary);
}

#[test]
fn a_member_another_pick_needs_is_ticked_and_counted_though_it_was_left_unticked() {
    let idx = zoo();
    let v = farm(&idx, &["owl"]);
    let cow = v.lines.iter().find(|l| l.id == "cow").unwrap();
    assert!(
        cow.ticked && cow.locked(),
        "owl needs cow: shown ticked, not unticked and locked"
    );
    assert_eq!(cow.needed_by, ["OWL"]);
    assert_eq!(
        (v.ticked, v.members),
        (2, 3),
        "counted in 'N of M selected'"
    );
    assert_eq!(v.header, collection::Ticks::Some);
    assert_eq!(
        v.summary.line(),
        "Installs 2 mods and 1 library",
        "owl, cow and lib"
    );
    // Install is given the one it needs too: it is ticked.
    assert_eq!(v.to_install(), ["cow", "owl"]);
}

#[test]
fn a_library_is_marked_by_the_registry_and_needed_by_another_mod() {
    let mut idx = zoo();
    let mut add = |id: &str, extra: serde_json::Value| {
        idx.projects.push(
            serde_json::from_value(project(id, "native-mod", &[("1.0.0", GAME, A)], extra))
                .unwrap(),
        );
    };
    // `tool` has content of its own and is needed by `jei`; `solo` is marked a library but
    // nothing needs it; `lib` is marked and needed by the farm's mods.
    add("tool", serde_json::json!({"downloads": 5}));
    add(
        "solo",
        serde_json::json!({"downloads": 4, "categories": ["utility", "library"]}),
    );
    idx_version(&mut idx, "jei").dependencies = needs("tool", "^1");
    let all = cards(&idx, GAME);
    let card = |id: &str| all.iter().find(|c| c.id == id).unwrap();
    assert!(card("lib").library);
    assert!(!card("tool").library, "needed, but not marked: still a mod");
    assert!(
        !card("solo").library,
        "marked, but needed by nothing: a mod of its own"
    );
    assert!(!card("cow").library);
    let f = Filters {
        sort: Sort::Name,
        ..Filters::default()
    };
    let browse: Vec<&str> = discover(&all, &f)
        .into_iter()
        .map(|i| all[i].id.as_str())
        .collect();
    assert_eq!(
        browse,
        ["farm", "jei", "solo", "tool"],
        "only the library goes"
    );
}

#[test]
fn picks_the_planner_cannot_install_together_say_so() {
    let mut idx = zoo();
    // Two more mods need incompatible lines of one library.
    idx_version(&mut idx, "lib").version = "2.0.0".into();
    let mut older = idx
        .projects
        .iter()
        .find(|p| p.id == "lib")
        .unwrap()
        .versions[0]
        .clone();
    older.version = "1.0.0".into();
    older.file.sha256 = B.into();
    idx.projects
        .iter_mut()
        .find(|p| p.id == "lib")
        .unwrap()
        .versions
        .push(older);
    idx_version(&mut idx, "cow").dependencies = needs("lib", "^1");
    idx_version(&mut idx, "pig").dependencies = needs("lib", "^2");
    let v = farm(&idx, &["cow", "pig"]);
    assert!(v.conflict.is_some(), "{:?}", v.lines);
    let v = farm(&idx, &["cow"]);
    assert!(v.conflict.is_none());
}

#[test]
fn the_planner_runs_again_only_when_the_picks_or_what_is_installed_change() {
    let idx = std::sync::Arc::new(zoo());
    let c = idx.collection("farm").unwrap().clone();
    let mut planned = collection::Planned::default();
    let picks = ticked(&["cow", "pig"]);
    for _ in 0..3 {
        planned.view(&idx, &c, &picks, &[], GAME);
    }
    assert_eq!(planned.worked, (1, 1), "three draws, one run of each");
    planned.view(&idx, &c, &ticked(&["pig"]), &[], GAME);
    assert_eq!(
        planned.worked,
        (1, 2),
        "a tick changes only the picks' plan"
    );
    let held = [index::resolve::InstalledMod {
        id: "lib".into(),
        version: "1.0.0".into(),
        sha256: None,
    }];
    planned.view(&idx, &c, &ticked(&["pig"]), &held, GAME);
    assert_eq!(planned.worked, (2, 3), "an install changes both");
}

#[test]
fn install_is_given_only_the_ticked_mods_not_yet_installed_and_stops_at_the_first_failure() {
    let idx = zoo();
    let c = idx.collection("farm").unwrap();
    let held = [index::resolve::InstalledMod {
        id: "cow".into(),
        version: "1.0.0".into(),
        sha256: Some(A.into()),
    }];
    let base = collection::base(&idx, c, &held, GAME);
    let v = collection::view(&idx, c, &base, &ticked(&["cow", "pig"]), &held, GAME);
    assert_eq!(
        v.to_install(),
        ["pig"],
        "not cow (installed), owl (unticked) or lib (a library)"
    );
}

#[test]
fn a_collection_reads_installed_when_the_members_it_starts_with_and_can_install_are() {
    let mut idx = zoo();
    idx.collections[0].members[2].default = false; // owl
    idx_version(&mut idx, "pig").game = "25.1".into(); // this build can't take pig
    let all = cards(&idx, GAME);
    let c = idx.collection("farm").unwrap();
    let have = |ids: &[&str]| -> std::collections::HashSet<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    };
    assert!(!installed_card(c, &all, &have(&[])));
    assert!(
        installed_card(c, &all, &have(&["cow"])),
        "owl is optional and pig can't be installed"
    );
}

fn installed_card(
    c: &sunrise_launcher_smod::index::Collection,
    cards: &[Card],
    have: &std::collections::HashSet<String>,
) -> bool {
    collection::installed(c, cards, |id| have.contains(id))
}

/// The farm where `cow`, a member, is itself a library: `owl` needs it, and it needs `lib`.
fn zoo_with_a_member_library() -> Index {
    let mut idx = zoo();
    idx.projects
        .iter_mut()
        .find(|p| p.id == "cow")
        .unwrap()
        .categories = vec!["library".into()];
    idx
}

#[test]
fn the_plan_line_and_the_header_count_the_same_picks_in_the_same_words() {
    let idx = zoo_with_a_member_library();
    let say = |picks: &[&str]| {
        let v = farm(&idx, picks);
        (
            v.plan_line(),
            v.can_install(),
            format!("{} of {}", v.ticked, v.members),
        )
    };
    // One pick whose library is locked in: the library is a library, however it got in.
    assert_eq!(
        say(&["pig"]),
        ("Installs 1 mod and 1 library".into(), true, "1 of 3".into())
    );
    // A member that is a library, pulled in by the pick that needs it, counts as selected.
    assert_eq!(
        say(&["owl"]),
        (
            "Installs 1 mod and 2 libraries".into(),
            true,
            "2 of 3".into()
        )
    );
    // All picked: the member library and the pulled-in one are libraries, the rest mods.
    assert_eq!(
        say(&["cow", "pig", "owl"]),
        (
            "Installs 2 mods and 2 libraries".into(),
            true,
            "3 of 3".into()
        )
    );
    // The member library alone.
    assert_eq!(
        say(&["cow"]),
        ("Installs 2 libraries".into(), true, "1 of 3".into())
    );
    // None picked: the card says so, and Install is off.
    assert_eq!(
        say(&[]),
        ("Pick the mods you want.".into(), false, "0 of 3".into())
    );
}

#[test]
fn install_is_off_when_everything_picked_is_installed() {
    let idx = zoo();
    let c = idx.collection("farm").unwrap();
    let held = ["cow", "pig", "lib"].map(|id| index::resolve::InstalledMod {
        id: id.into(),
        version: "1.0.0".into(),
        sha256: Some(A.into()),
    });
    let base = collection::base(&idx, c, &held, GAME);
    let v = collection::view(&idx, c, &base, &ticked(&["cow", "pig"]), &held, GAME);
    assert_eq!(v.plan_line(), "Everything you picked is installed.");
    assert!(v.done() && !v.can_install());
}

#[test]
fn an_unreadable_file_is_never_on_and_cannot_switch_a_working_mod_of_its_name() {
    let on: HashSet<String> = HashSet::from(["ruby".to_owned()]);
    let none = HashSet::new();
    let (idx, _) = pack();
    let file = |unreadable| Local {
        id: "ruby".into(),
        file: "ruby".to_owned(),
        name: "ruby".into(),
        version: String::new(),
        github: None,
        native: true,
        served_by: None,
        unreadable,
    };
    let local = [file(true), file(false)];
    let w = World {
        local: &local,
        ..world(&idx, &on, &none)
    };
    let r = rows(&[], &w);
    let broken = r
        .iter()
        .find(|r| r.local.as_ref().is_some_and(|l| l.unreadable))
        .expect("listed");
    assert!(!broken.on);
    assert_eq!(broken.need, None);
    assert!(!cells(broken, None, None).switch);
    let good = r
        .iter()
        .find(|r| r.local.as_ref().is_some_and(|l| !l.unreadable))
        .expect("listed");
    assert!(good.on);
    assert_eq!(good.need, None);
    assert!(cells(good, None, None).switch);
}

#[test]
fn manual_mods_get_badges_and_never_enter_bulk_updates() {
    use sunrise_launcher::github_mods::{AddedFrom, Origin, SourceOrigin, SourceTrack};

    let (idx, _) = pack();
    let on = HashSet::from([
        "github_mod".to_owned(),
        "folder_mod".to_owned(),
        "local_smod".to_owned(),
        "discover_src".to_owned(),
    ]);
    let none = HashSet::new();
    let github_origin = Origin {
        id: "github_mod".into(),
        version: "0.4.0".into(),
        owner: "emberlane".into(),
        repo: "camp-lanterns".into(),
        commit: "1".repeat(40),
        sha256: A.into(),
        tag: None,
        asset: None,
        source: Some(SourceOrigin {
            repo_id: 99,
            manifest: "sunrise-mod.toml".into(),
            track: SourceTrack::Branch("main".into()),
            published_at: None,
        }),
        added_from: Some(AddedFrom::Github),
    };
    let folder_origin = Origin::local_folder(
        "folder_mod".into(),
        "0.2.0".into(),
        "/tmp/local-repo".into(),
        A.into(),
    );
    let discover_origin = Origin {
        id: "discover_src".into(),
        added_from: Some(AddedFrom::Discover),
        ..github_origin.clone()
    };
    let locals = [
        Local {
            id: "github_mod".into(),
            file: "sunrise-native-mods/github_mod.smod".into(),
            name: "Camp Lanterns".into(),
            version: "0.4.0".into(),
            github: Some(github_origin),
            native: true,
            served_by: None,
            unreadable: false,
        },
        Local {
            id: "folder_mod".into(),
            file: "sunrise-mods/folder_mod.smod".into(),
            name: "Folder Mod".into(),
            version: "0.2.0".into(),
            github: Some(folder_origin),
            native: false,
            served_by: None,
            unreadable: false,
        },
        Local {
            id: "local_smod".into(),
            file: "sunrise-native-mods/local_smod.smod".into(),
            name: "Lantern Tweaks".into(),
            version: "0.1.0".into(),
            github: None,
            native: true,
            served_by: None,
            unreadable: false,
        },
        Local {
            id: "discover_src".into(),
            file: "sunrise-native-mods/discover_src.smod".into(),
            name: "Discover Source".into(),
            version: "0.4.0".into(),
            github: Some(discover_origin),
            native: true,
            served_by: None,
            unreadable: false,
        },
    ];
    let w = World {
        local: &locals,
        ..world(&idx, &on, &none)
    };
    let mut all = rows(&[entry("zoom", "1.0.0", false, A)], &w);
    // Even if an update string were attached, manual mods must never be bulk-updatable.
    for row in &mut all {
        row.update = Some("9.9.9".into());
    }
    let find = |id: &str| all.iter().find(|r| r.id == id).unwrap();
    assert_eq!(
        find("github_mod").manual_badge(),
        Some(ManualBadge::FromSource)
    );
    assert!(find("github_mod").is_manual());
    assert!(!bulk_updatable(find("github_mod")));

    assert_eq!(
        find("folder_mod").manual_badge(),
        Some(ManualBadge::FromSource)
    );
    assert!(find("folder_mod").is_manual());
    assert!(!bulk_updatable(find("folder_mod")));

    assert_eq!(find("local_smod").manual_badge(), Some(ManualBadge::Local));
    assert!(find("local_smod").is_manual());
    assert!(!bulk_updatable(find("local_smod")));

    assert_eq!(find("discover_src").manual_badge(), None);
    assert!(!find("discover_src").is_manual());

    assert_eq!(find("zoom").manual_badge(), None);
    assert!(!find("zoom").is_manual());
    assert!(bulk_updatable(find("zoom")));
}

#[test]
fn every_fitting_native_mod_builds_locally_even_with_legacy_native_binaries() {
    let mut native = project(
        "zoom",
        "native-mod",
        &[("1.0.0", GAME, A)],
        serde_json::json!({}),
    );
    native["versions"][0]["native_binaries"] = serde_json::json!([{
        "build": "b".repeat(40),
        "target": "x86_64-unknown-linux-gnu",
        "file": {"name": "zoom.tar.xz", "url": "https://example.test/zoom.tar.xz", "size": 128, "sha256": A}
    }]);
    let mut idx = index(
        vec![
            native,
            project(
                "old",
                "native-mod",
                &[("1.0.0", "25.1", A)],
                serde_json::json!({}),
            ),
            project("paint", "mod", &[("1.0.0", GAME, A)], serde_json::json!({})),
            project(
                "other",
                "native-mod",
                &[("1.0.0", GAME, B)],
                serde_json::json!({}),
            ),
        ],
        vec![],
    );
    idx.collections = vec![serde_json::from_value(serde_json::json!({
        "id": "pack", "name": "Pack", "summary": "Native bundle.", "owners": ["Someone"],
        "icon": "https://example.test/pack.png",
        "members": [{"id": "zoom"}, {"id": "other"}],
    }))
    .unwrap()];
    let all = cards(&idx, GAME);
    let card = |id: &str| all.iter().find(|c| c.id == id).unwrap();
    assert!(card("zoom").builds_locally());
    assert!(!card("old").builds_locally());
    assert!(!card("paint").builds_locally());
    assert!(!card("pack").builds_locally());
}

#[test]
#[ignore = "local benchmark; run with --release --ignored --nocapture"]
fn benchmark_discover_search() {
    let projects = (0..600)
        .map(|n| {
            let mut p = project(
                &format!("mod-{n}"),
                "mod",
                &[("1.0.0", GAME, A)],
                serde_json::json!({"downloads": n}),
            );
            p["summary"] = "Adds a handful of new blocks, a crafting bench and some ambient sounds to caves and the deep dark, with options for each.".into();
            p
        })
        .collect();
    let cards = cards(&index(projects, vec![]), GAME);
    let f = Filters {
        text: "deep bench".into(),
        ..Filters::default()
    };
    for _ in 0..50 {
        std::hint::black_box(discover(&cards, &f));
    }
    let runs = 2000;
    let t = std::time::Instant::now();
    for _ in 0..runs {
        std::hint::black_box(discover(&cards, std::hint::black_box(&f)));
    }
    println!(
        "discover over {} cards: {:.1} µs a call",
        cards.len(),
        t.elapsed().as_secs_f64() * 1e6 / runs as f64
    );
}

#[test]
fn a_v2_installed_row_offers_the_newest_release_listed_for_its_build_and_shows_a_takedown() {
    let release = |version: &str, fits: &str, sha: char| {
        serde_json::json!({
            "version": version, "published": "2026-11-22", "sunrise": "26.3-0.1.0",
            "workflow": "w", "fits": [fits], "does_not_fit": [],
            "file": {"url": "u", "size": 1, "sha256": sha.to_string().repeat(64), "attestation": "a"},
            "dependencies": []
        })
    };
    let v2 = |revoked: serde_json::Value| -> index_v2::Index {
        serde_json::from_value(serde_json::json!({
            "schema": 2, "serial": 1, "generated": "2026-11-22T00:00:00Z", "builds": [], "categories": [],
            "projects": [{
                "id": "Ab3dE9xQ", "slug": "wayfinder", "mod": "wayfinder", "kind": "mod",
                "name": "Wayfinder", "summary": "", "description": "", "authors": ["Kit"],
                "owners": ["kit"], "license": "MIT", "icon": "i", "screenshots": [], "categories": [],
                "repository": {"id": 1, "name": "kit/wayfinder", "path": "."}, "workflow": "w",
                "releases": [
                    release("1.3.0", "26.3-0.2.0", 'd'),
                    release("1.2.0", "26.3-0.1.0", 'c'),
                    release("1.1.0", "26.3-0.1.0", 'b'),
                    release("1.0.0", "26.3-0.1.0", 'a'),
                ]
            }],
            "revoked": revoked
        }))
        .unwrap()
    };
    let mut installed = entry("Ab3dE9xQ", "1.0.0", false, &"a".repeat(64));
    installed.mod_id = Some("wayfinder".into());
    let on: HashSet<String> = ["wayfinder".to_owned()].into();
    let none = HashSet::new();
    let world = |index: &index_v2::Index| -> Row {
        rows(
            std::slice::from_ref(&installed),
            &World {
                index: None,
                game: GAME,
                v2: Some(index),
                build: Some("26.3-0.1.0"),
                unfit_updates: &NO_UNFIT_UPDATES,
                enabled: &on,
                unbuilt: &none,
                chosen: &NO_PINS,
                local: &[],
            },
        )
        .remove(0)
    };

    let listed = v2(serde_json::json!([]));
    let row = world(&listed);
    assert_eq!(row.title, "Wayfinder");
    assert!(row.v2_installed());
    // 1.3.0 is newer but listed for another build only.
    assert_eq!(row.update.as_deref(), Some("1.2.0"));
    assert_eq!(row.need, None);

    let taken_down = v2(serde_json::json!([{
        "project": "Ab3dE9xQ", "sha256": "a".repeat(64), "reason": "malware", "date": "2026-11-23"
    }]));
    assert_eq!(
        world(&taken_down).need,
        Some(Need::TakenDown("malware".into()))
    );
}

#[test]
fn a_served_mod_needs_its_host_on() {
    let local = |id: &str, native: bool, served_by: Option<&str>| Local {
        id: id.into(),
        file: format!("x/{id}.smod"),
        name: id.into(),
        version: "1.0.0".into(),
        github: None,
        native,
        served_by: served_by.map(str::to_owned),
        unreadable: false,
    };
    let locals = [
        Local {
            name: "Java Host".into(),
            ..local("java_host", true, None)
        },
        local("runes", false, Some("java_host")),
    ];
    let none = HashSet::new();
    let shown = |enabled: &HashSet<String>| {
        rows(
            &[],
            &World {
                index: None,
                game: GAME,
                v2: None,
                build: None,
                unfit_updates: &NO_UNFIT_UPDATES,
                enabled,
                unbuilt: &none,
                chosen: &NO_PINS,
                local: &locals,
            },
        )
    };
    let runes = |rows: &[Row]| rows.iter().find(|r| r.id == "runes").unwrap().clone();

    // On without its host: the game leaves it out, and the row says what it needs.
    let alone: HashSet<String> = ["runes".to_owned()].into();
    let row = runes(&shown(&alone));
    assert_eq!(row.need, Some(Need::Host("Java Host".into())));
    assert_eq!(row.group(), Group::NeedsYou);
    let both: HashSet<String> = ["runes".to_owned(), "java_host".to_owned()].into();
    assert_eq!(runes(&shown(&both)).need, None);
}
