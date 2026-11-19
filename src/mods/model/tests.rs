use super::*;
use sunrise_launcher::browser::{installed::Entry, mod_list};

const GAME: &str = "26.3";

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

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

static NO_PINS: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
    std::sync::LazyLock::new(Default::default);

fn world<'a>(
    index: &'a Index,
    enabled: &'a HashSet<String>,
    unbuilt: &'a HashSet<String>,
    allowed: bool,
) -> World<'a> {
    World {
        index: Some(index),
        game: GAME,
        enabled,
        unbuilt,
        native_allowed: allowed,
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
    assert_eq!(p.side, "Not needed on servers");
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
    assert_eq!(newer(&idx, p, "2.0.0", GAME), Some("3.0.0".into()));
    assert_eq!(newer(&idx, p, "3.0.0", GAME), None);
    assert_eq!(
        newer(&idx, p, "0.9.0", GAME),
        None,
        "a version the index doesn't list is left alone"
    );
    assert_eq!(
        newer(&idx, p, "2.0.0", "25.1"),
        None,
        "nothing for another build"
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
fn rows_put_what_needs_the_player_first_then_updates_then_the_rest() {
    let (idx, entries) = pack();
    let enabled: HashSet<String> = ["quiet", "zoom", "deep", "bad"].map(String::from).into();
    let unbuilt: HashSet<String> = ["deep".to_string()].into();
    let r = rows(&entries, &world(&idx, &enabled, &unbuilt, true));
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
    let live = world(&idx, &none, &none, true);
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
fn native_code_that_is_on_without_permission_says_so_instead_of_asking_to_build() {
    let (idx, entries) = pack();
    let enabled: HashSet<String> = ["deep".to_string()].into();
    let unbuilt: HashSet<String> = ["deep".to_string()].into();
    let r = rows(&entries, &world(&idx, &enabled, &unbuilt, false));
    let deep = r.iter().find(|r| r.id == "deep").unwrap();
    assert_eq!(deep.need, Some(Need::Permission));
}

#[test]
fn only_registry_mods_are_rows_packs_are_left_alone() {
    let (idx, mut entries) = pack();
    let mut other = entry("shaderish", "1", false, A);
    other.kind = sunrise_launcher::browser::Kind::Shader;
    entries.push(other);
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &none, &none, true));
    assert_eq!(r.len(), 4);
    assert!(r.iter().all(|r| r.id != "shaderish"));
}

#[test]
fn filters_narrow_the_shown_rows_and_never_reorder_them() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["quiet", "zoom"].map(String::from).into();
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &on, &none, true));
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
fn update_all_covers_the_whole_pack_and_names_what_it_leaves_out() {
    let (idx, entries) = pack();
    let none = HashSet::new();
    let on: HashSet<String> = ["zoom", "bad", "deep"].map(String::from).into();
    let r = rows(&entries, &world(&idx, &on, &none, false));
    let plan = plan_updates(&r, false);
    let ids: Vec<&str> = plan.included.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        ["bad", "zoom"],
        "a taken-down mod is updated to a file that wasn't"
    );
    assert_eq!(plan.skipped.len(), 1, "{:?}", plan.skipped);
    assert!(plan
        .skipped
        .iter()
        .any(|(t, why)| t == "DEEP" && why.contains("native")));
    let with = plan_updates(&r, true);
    assert!(with.included.iter().any(|i| i.id == "deep" && i.native));
    assert!(with.skipped.is_empty());

    // A version the player chose is never moved by a batch.
    let pins = std::collections::BTreeMap::from([("zoom".to_string(), "1.0.0".to_string())]);
    let w = World {
        chosen: &pins,
        ..world(&idx, &on, &none, true)
    };
    let pinned = plan_updates(&rows(&entries, &w), true);
    assert!(pinned.included.iter().all(|i| i.id != "zoom"));
    assert!(pinned.skipped.iter().any(|(t, _)| t == "ZOOM"));
}

#[test]
fn an_update_never_switches_a_turned_off_mod_on() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["zoom".to_string()].into();
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &on, &none, true));
    let plan = plan_updates(&r, true);
    assert_eq!(
        plan.included
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        ["zoom"]
    );
    let off: Vec<&str> = plan
        .skipped
        .iter()
        .filter(|(_, why)| why.contains("turned off"))
        .map(|(t, _)| t.as_str())
        .collect();
    assert_eq!(off, ["BAD", "DEEP"]);
}

#[test]
fn native_files_the_browser_didnt_install_are_listed_and_never_planned() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["mine", "zoom", "by_hand"].map(String::from).into();
    let none = HashSet::new();
    let local = [
        Local {
            id: "mine".into(),
            name: "mine".into(),
            version: "0.3".into(),
            github: false,
            native: true,
        },
        Local {
            id: "by_hand".into(),
            name: "By Hand".into(),
            version: "1.0".into(),
            github: false,
            native: false,
        },
    ];
    let w = World {
        local: &local,
        ..world(&idx, &on, &none, false)
    };
    let r = rows(&entries, &w);
    let mine = r.iter().find(|r| r.id == "mine").expect("listed");
    assert!(mine.local.is_some() && mine.entry.is_none() && mine.update.is_none());
    assert_eq!(
        mine.need,
        Some(Need::Permission),
        "it can't run without permission, like any native mod"
    );
    // A package of `sunrise-mods/` put there by hand is listed by its name and needs neither
    // permission nor a build.
    let wasm = r.iter().find(|r| r.id == "by_hand").expect("listed");
    assert_eq!((wasm.title.as_str(), wasm.on), ("By Hand", true));
    assert_eq!((wasm.make, &wasm.need), (None, &None));
    assert!(wasm.local.is_some() && wasm.entry.is_none());
    assert!(plan_updates(&r, true)
        .included
        .iter()
        .all(|i| i.id != "mine"));
}

#[test]
fn turning_on_a_local_native_file_exposes_permission_then_build() {
    let home = tempfile::tempdir().unwrap();
    let on_list = home.path();
    let id = "example";
    let local = [Local {
        id: id.into(),
        name: "Example".into(),
        version: "1.0".into(),
        github: false,
        native: true,
    }];
    let unbuilt = HashSet::from([id.into()]);
    let row = |allowed| {
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
                enabled: &enabled,
                unbuilt: &unbuilt,
                native_allowed: allowed,
                chosen: &NO_PINS,
                local: &local,
            },
        )
        .remove(0)
    };
    let off = row(false);
    assert!(!off.on);
    assert!(cells(&off, None, None).switch);
    mod_list::set_enabled(on_list, id, true).unwrap();
    let needs_permission = row(false);
    assert!(needs_permission.on);
    assert_eq!(needs_permission.need, Some(Need::Permission));
    assert_eq!(row(true).need, Some(Need::Build));
}

#[test]
fn turning_on_skips_what_is_taken_down_or_needs_permission() {
    let (idx, entries) = pack();
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &none, &none, false));
    let all: Vec<&Row> = r.iter().collect();
    let (ok, held) = turn_on_plan(&all, false);
    assert_eq!(
        ok.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["zoom", "quiet"]
    );
    assert_eq!(held.len(), 2);
    let (ok, held) = turn_on_plan(&all, true);
    assert_eq!(ok.len(), 3);
    assert_eq!(held.len(), 1, "taken down stays held even with permission");
}

#[test]
fn removing_a_mod_names_what_still_needs_it() {
    let mut idx = index(
        vec![
            project("lib", "mod", &[("1.0.0", GAME, A)], serde_json::json!({})),
            project(
                "uses_lib",
                "mod",
                &[("1.0.0", GAME, B)],
                serde_json::json!({}),
            ),
        ],
        vec![],
    );
    idx.projects[1].versions[0].requires = vec![index::Requirement {
        id: "lib".into(),
        version: "1.0.0".into(),
        sha256: A.into(),
    }];
    let entries = vec![
        entry("lib", "1.0.0", false, A),
        entry("uses_lib", "1.0.0", false, B),
    ];
    let none = HashSet::new();
    let r = rows(&entries, &world(&idx, &none, &none, true));
    let removing: HashSet<String> = ["lib".to_string()].into();
    assert_eq!(
        dependents(&r, &removing, Some(&idx)),
        [("USES_LIB".to_string(), vec!["LIB".to_string()])]
    );
    let both: HashSet<String> = ["lib", "uses_lib"].map(String::from).into();
    assert!(dependents(&r, &both, Some(&idx)).is_empty());
    assert!(dependents(&r, &removing, None).is_empty());
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
        name: id.into(),
        version: "1.0".into(),
        github: false,
        native: true,
    });
    let w = World {
        local: &local,
        native_allowed: true,
        ..world(&idx, &on, &none, false)
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
        name: id.into(),
        version: "1.0".into(),
        github: id == "lib",
        native: true,
    });
    let w = World {
        local: &local,
        native_allowed: true,
        ..world(&idx, &on, &unbuilt, false)
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
        "Needs a build"
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
    // A row with nothing pressing says where it is from, quietly; its cells are still there.
    let fine = cells(row("fine"), None, None);
    assert_eq!(fine.status.as_ref().unwrap().tone, Tone::Muted);
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
    // Permission comes before a newer version: its button is Allow.
    both.need = Some(Need::Permission);
    assert_eq!(cells(&both, None, None).action, Some(RowAction::Allow));
}

#[test]
fn the_build_button_is_only_counted_as_drawn_when_its_group_is_shown() {
    let (idx, entries) = pack();
    let on: HashSet<String> = ["lib", "addon", "fine"].map(String::from).into();
    let unbuilt: HashSet<String> = ["addon".to_string()].into();
    let local = ["lib", "addon", "fine"].map(|id| Local {
        id: id.into(),
        name: id.into(),
        version: "1.0".into(),
        github: false,
        native: true,
    });
    let w = World {
        local: &local,
        native_allowed: true,
        ..world(&idx, &on, &unbuilt, false)
    };
    let r = rows(&entries, &w);
    let addon = r.iter().find(|r| r.id == "addon").unwrap();
    assert_eq!((addon.need.clone(), addon.on), (Some(Need::Build), true));
    // Unfiltered: Build stands in the NEEDS YOU head, for the one mod that waits.
    let all = visible(&r, &RowFilter::default());
    assert_eq!(build_in_head(&r, &all), Some(1));
    // A filter that still shows a row of the group keeps its head, and the button with it.
    let off = visible(
        &r,
        &RowFilter {
            show: Show::Off,
            ..RowFilter::default()
        },
    );
    assert!(off.iter().any(|&i| r[i].id == "bad"));
    assert_eq!(build_in_head(&r, &off), Some(1));
    // A search that leaves the whole group out: no button is drawn, so the status strip keeps
    // the running build's bar.
    let shown = visible(
        &r,
        &RowFilter {
            text: "fine".into(),
            ..RowFilter::default()
        },
    );
    assert!(!shown.is_empty());
    assert!(shown.iter().all(|&i| r[i].group() != Group::NeedsYou));
    assert_eq!(build_in_head(&r, &shown), None);
    // Nothing waits for a build: no button either way.
    let none: HashSet<String> = HashSet::new();
    let built = rows(
        &entries,
        &World {
            local: &local,
            native_allowed: true,
            ..world(&idx, &on, &none, false)
        },
    );
    assert_eq!(
        build_in_head(&built, &visible(&built, &RowFilter::default())),
        None
    );
}
