//! Opt-in example lists for design critique (`SUNRISE_PREVIEW_EXAMPLES=1`). They are drawn in
//! place of the registry and the player's files, say so on screen, and nothing on them can be
//! installed, turned on or removed: every action on an example row is disabled. No example is
//! ever passed to the install, update, toggle or remove code.
use super::model::{Card, Fit, Make, Need, Page, Row, VersionLine};

pub(super) fn on() -> bool {
    std::env::var("SUNRISE_PREVIEW_EXAMPLES").is_ok_and(|v| !v.is_empty() && v != "0")
}

const CARDS: [(&str, &str, &str, u64, Make, &str); 12] = [
    (
        "jei",
        "JEI",
        "Look up how any item is made and what it is used for.",
        12_400,
        Make::Native,
        "Quality of life",
    ),
    (
        "inventory_lens",
        "Inventory Lens",
        "Search nearby storage without moving a single item.",
        8_200,
        Make::Content,
        "Storage",
    ),
    (
        "field_botanist",
        "Field Botanist",
        "Identify flowers and record where you found them.",
        6_400,
        Make::Content,
        "Exploration",
    ),
    (
        "recipe_bookmarks",
        "Recipe Bookmarks",
        "Keep favourite recipes one keypress away.",
        5_800,
        Make::Content,
        "Quality of life",
    ),
    (
        "village_lanterns",
        "Village Lanterns",
        "Hanging lights for paths and trading halls.",
        4_600,
        Make::Content,
        "Building",
    ),
    (
        "ore_survey",
        "Ore Survey",
        "Map ore samples without recording your seed.",
        4_100,
        Make::Native,
        "Exploration",
    ),
    (
        "shelf_labels",
        "Shelf Labels",
        "Name a shelf once and read it from across the room.",
        3_900,
        Make::Content,
        "Storage",
    ),
    (
        "pocket_field_guide",
        "Pocket Field Guide",
        "A notebook that fills in as you discover things.",
        3_400,
        Make::Content,
        "Exploration",
    ),
    (
        "northern_reach",
        "Caves of the Northern Reach",
        "Deeper caverns, ice halls and underground rivers.",
        3_100,
        Make::Native,
        "World generation",
    ),
    (
        "storage_drawers",
        "Compact Storage Drawers",
        "Drawers that stack, sort and show what they hold.",
        2_800,
        Make::Content,
        "Storage",
    ),
    (
        "saltmarsh",
        "Saltmarsh Gardens",
        "Reeds, samphire and tide pools for the coast.",
        2_500,
        Make::Content,
        "World generation",
    ),
    (
        "lantern_pathways",
        "Lantern Pathways",
        "Paths that light up as you walk them at night.",
        2_200,
        Make::Content,
        "Building",
    ),
];

pub(super) fn cards() -> Vec<Card> {
    CARDS
        .iter()
        .enumerate()
        .map(|(i, (id, name, summary, downloads, make, category))| Card {
            id: (*id).into(),
            name: (*name).into(),
            authors: ["mezz", "Mallow", "Linden", "Wren"][i % 4].into(),
            summary: (*summary).into(),
            downloads: *downloads,
            make: *make,
            categories: vec![(*category).into()],
            icon: None,
            updated: format!("2026-11-{:02}", 17 - i),
            first: format!("2026-0{}-01", 1 + i % 9),
            fit: if i == 8 {
                Fit::OtherBuild("an older build".into())
            } else {
                Fit::Fits
            },
            builds_locally: *make == Make::Native,
            example: true,
        })
        .collect()
}

pub(super) fn page(id: &str) -> Option<Page> {
    let card = cards().into_iter().find(|c| c.id == id)?;
    let line = |number: &str, fit: Fit| VersionLine {
        number: number.into(),
        published: "2026-11-12".into(),
        game: "example".into(),
        fit,
        size: 2_200_000,
    };
    Some(Page {
        id: card.id.clone(),
        name: card.name.clone(),
        authors: vec![card.authors.clone()],
        description: format!(
            "{}\n\nThis page is an example for design review; it is not read from the registry.",
            card.summary
        ),
        license: "MPL-2.0".into(),
        source: String::new(),
        downloads: card.downloads,
        updated: card.updated.clone(),
        categories: card.categories.clone(),
        screenshots: Vec::new(),
        icon: None,
        make: card.make,
        side: "Not needed on servers",
        versions: vec![line("2.4.1", card.fit.clone()), line("2.4.0", Fit::Fits)],
        fit: card.fit.clone(),
        will_install: Some(line("2.4.1", Fit::Fits)),
        trust: vec!["Example data: nothing here is checked or signed.".into()],
        requires: Vec::new(),
        example: true,
    })
}

/// A pack of `n` rows: two that need the player, three updates, the rest quiet.
pub(super) fn rows(n: usize) -> Vec<Row> {
    let names = [
        "Alpine Orchards",
        "Antique Signposts",
        "Better Container Labels",
        "Biome Soundscapes",
        "Botanical Cabinets",
        "Caves of the Northern Reach",
        "Copper Lanterns",
        "Cartographer's Desk",
    ];
    let mut out = vec![
        row(
            "machinists_workbench",
            "Machinist's Workbench",
            Some(Make::Native),
            "3.1.2",
            false,
            Some(Need::Build),
            None,
        ),
        row(
            "lantern_pathways",
            "Lantern Pathways",
            Some(Make::Content),
            "1.2.0",
            true,
            Some(Need::TakenDown("example reason".into())),
            None,
        ),
        row(
            "cartographer",
            "Cartographer's Desk",
            Some(Make::Content),
            "0.9.3",
            true,
            None,
            Some("0.9.4"),
        ),
        row(
            "copper_lanterns",
            "Copper Lanterns",
            Some(Make::Content),
            "1.4.0",
            true,
            None,
            Some("1.4.2"),
        ),
        row(
            "pocket_field_guide",
            "Pocket Field Guide",
            Some(Make::Content),
            "2.0.8",
            true,
            None,
            Some("2.1.0"),
        ),
    ];
    for i in 0..n.saturating_sub(out.len()) {
        let make = Make::ALL[i % 2];
        out.push(row(
            &format!("example_{i}"),
            &format!("{} {}", names[i % names.len()], i / names.len() + 1),
            Some(make),
            "1.0.0",
            i % 9 != 4,
            None,
            None,
        ));
    }
    out
}

fn row(
    id: &str,
    title: &str,
    make: Option<Make>,
    version: &str,
    on: bool,
    need: Option<Need>,
    update: Option<&str>,
) -> Row {
    Row {
        id: id.into(),
        title: title.into(),
        authors: "Example".into(),
        make,
        version: version.into(),
        on,
        need,
        update: update.map(str::to_owned),
        dependency: false,
        pinned: false,
        icon: None,
        entry: None,
        example: true,
    }
}
