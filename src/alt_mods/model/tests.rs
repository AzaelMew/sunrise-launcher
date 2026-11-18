use super::*;
use sunrise_launcher::browser::installed::Entry;

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
    assert_eq!(discover(&cards, &f), [0], "Fits my build is on by default");
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
    let page = page(&idx, "zoom", GAME, vec![]).unwrap();
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
    let p = page(&idx, "zoom", GAME, vec!["Made by Someone".into()]).unwrap();
    assert_eq!(
        p.will_install.as_ref().map(|v| v.number.as_str()),
        Some("2.0.0")
    );
    assert_eq!(p.versions[1].fit, Fit::OtherBuild("25.1".into()));
    assert_eq!(p.side, "Not needed on servers");
    assert!(page(&idx, "missing", GAME, vec![]).is_none());
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
    let on: HashSet<String> = ["mine", "zoom"].map(String::from).into();
    let none = HashSet::new();
    let local = [Local {
        id: "mine".into(),
        version: "0.3".into(),
        github: false,
    }];
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
    assert!(plan_updates(&r, true)
        .included
        .iter()
        .all(|i| i.id != "mine"));
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
fn example_rows_are_marked_and_never_planned() {
    let rows = examples::rows(120);
    assert_eq!(rows.len(), 120);
    assert!(
        rows.iter().all(|r| r.example && r.entry.is_none()),
        "no example carries an installed entry"
    );
    let plan = plan_updates(&rows, true);
    assert!(
        plan.included.is_empty() && plan.skipped.is_empty(),
        "no example reaches an update"
    );
    assert!(examples::cards().iter().all(|c| c.example));
    assert!(examples::page("jei").is_some_and(|p| p.example));
}

#[test]
fn counts_read_short() {
    assert_eq!(size(2048), "2.0 KiB");
    assert_eq!(size(3 << 20), "3.0 MiB");
    assert!(downloads(12_400).starts_with("12.4k"));
    assert!(downloads(7).starts_with("7 "));
}
