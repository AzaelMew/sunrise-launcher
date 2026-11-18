use super::*;
use sunrise_launcher_friends::invite::Invite;

fn project(id: &str, kind: Kind, blocked: Option<&str>) -> Project {
    Project {
        source: SourceId::Sunrise,
        id: id.into(),
        slug: id.into(),
        title: id.into(),
        summary: String::new(),
        author: String::new(),
        downloads: 0,
        page: String::new(),
        icon: None,
        license: String::new(),
        kind,
        game_versions: vec!["26.3".into()],
        loaders: vec!["sunrise".into()],
        blocked: blocked.map(String::from),
    }
}

#[test]
fn the_box_tells_an_address_from_an_invite_code() {
    let code = Invite {
        secret: [7; sunrise_launcher_friends::invite::SECRET],
        port: 25565,
        endpoints: vec!["203.0.113.5:25565".parse().unwrap()],
        relay: None,
    }
    .encode();
    assert_eq!(entry(&code), Entry::Invite);
    assert_eq!(
        entry("  play.example.com:25566 "),
        Entry::Address("play.example.com:25566".into())
    );
    assert_eq!(entry("   "), Entry::Empty);
    assert_eq!(entry("a b"), Entry::Unusable);
}

#[test]
fn a_world_always_takes_the_same_block() {
    assert_eq!(block_for("Harbor"), block_for("Harbor"));
    assert!(BLOCKS.contains(&block_for("")));
    let seen: std::collections::HashSet<_> = ["a", "b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .map(|f| block_for(f))
        .collect();
    assert!(seen.len() > 1);
}

#[test]
fn a_tile_shows_the_first_letter_in_capitals() {
    assert_eq!(initial("  éclair"), "É");
    assert_eq!(initial(""), "?");
    assert_eq!(sentence("yesterday"), "Yesterday");
}

#[test]
fn popular_mods_leave_out_what_cannot_run_or_is_installed() {
    let found = vec![
        project("a", Kind::Mod, None),
        project("b", Kind::Mod, Some("taken down")),
        project("c", Kind::Modpack, None),
        project("d", Kind::Mod, None),
        project("e", Kind::Mod, None),
    ];
    let ids = |rows: Vec<Project>| rows.into_iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(
        ids(popular_rows(&found, "26.3", |p| p.id == "d", 6)),
        ["a", "e"]
    );
    assert_eq!(ids(popular_rows(&found, "26.3", |_| false, 2)), ["a", "d"]);
}

#[test]
fn there_are_no_featured_servers_until_there_is_a_source() {
    assert!(featured().is_empty());
}
