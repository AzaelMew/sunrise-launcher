//! The Modpacks page's headline and row explanations, in the launcher's language (English here).
use std::path::PathBuf;
use sunrise_launcher::{
    i18n::{self, Language},
    modpacks::{row_plain, Imported, Row, Summary},
};

fn pack_with(summary: Summary) -> Imported {
    Imported {
        slug: "pack".into(),
        work: PathBuf::new(),
        pack: Default::default(),
        rows: Vec::new(),
        summary,
        fetched: Default::default(),
        mods: Vec::new(),
        left_off: Vec::new(),
        portable: 0,
        resourcepacks: 0,
        indexed_resources: 0,
        datapacks: 0,
    }
}

#[test]
fn the_headline_says_what_comes_over_and_what_waits() {
    i18n::set(Language::English);
    assert!(pack_with(Summary::default()).headline().is_empty());
    let partial = Summary {
        needing: 2,
        carried: 2,
        partial: 2,
        ..Default::default()
    };
    assert_eq!(
        pack_with(partial).headline(),
        [
            "2 of 2 mods that need porting come over as content (blocks, items and structures); \
             their own code does not run.",
            "Partial means part of a mod came over, but something is left out, differs from the \
             original or has not been checked yet.",
        ]
    );
    let waiting = Summary {
        needing: 4,
        carried: 1,
        ready: 1,
        blocked: 1,
        later: 1,
        ..Default::default()
    };
    let lines = pack_with(waiting).headline();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("1 of 4 mods that need porting come over"));
    assert!(
        lines[1].starts_with("Not yet: 1 are content mods Port can carry over, 1 need mod support")
    );
}

#[test]
fn row_explanations_keep_unknown_sentences_and_add_the_dependency_count() {
    i18n::set(Language::English);
    let row = |plain: &str, waiting: usize| -> Row {
        serde_json::from_value(serde_json::json!({
            "key": "a", "status": "not-yet", "plain": plain, "waiting": waiting
        }))
        .unwrap()
    };
    assert_eq!(
        row_plain(&row("It does not come over yet.", 0)),
        "It does not come over yet."
    );
    assert_eq!(
        row_plain(&row("It does not come over yet.", 1)),
        "It does not come over yet. It also waits for 1 mod it depends on that is not ported yet."
    );
    assert!(row_plain(&row("It does not come over yet.", 2))
        .ends_with("2 mods it depends on that are not ported yet."));
    assert_eq!(
        row_plain(&row("An explanation from another build.", 0)),
        "An explanation from another build."
    );
}
