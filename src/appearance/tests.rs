use super::*;
use crate::theme_files::{Local, Reason, Roles};

fn tidepool(accent: u32, picture: Option<PathBuf>) -> Entry {
    Entry::Good(Local {
        slug: "tidepool".into(),
        name: "Tidepool".into(),
        roles: Roles {
            accent,
            ..Roles::DAWN
        },
        picture,
    })
}

fn saved(settings: &Settings) -> serde_json::Value {
    serde_json::to_value(settings).unwrap()
}

#[test]
fn no_saved_theme_and_dawn_are_the_same_dawn() {
    let none = Settings::parse("{}");
    let dawn = Settings::parse(r#"{"theme": "dawn"}"#);
    for settings in [&none, &dawn] {
        assert_eq!(effective_id(settings), "dawn");
        assert_eq!(accent(settings).rgb(), 0xb4552f);
        assert!(!missing(settings));
    }
    // They share one remembered accent, and neither choice is rewritten when it is read.
    assert_eq!(themes::accent_key(None), "dawn");
    assert_eq!(themes::accent_key(Some("dawn")), "dawn");
    assert_eq!(saved(&none).get("theme"), None);
    assert_eq!(saved(&dawn)["theme"], "dawn");
    let kept = Settings::parse(r##"{"theme_accents": {"dawn": "#c2366f"}}"##);
    assert_eq!(accent(&kept).rgb(), 0xc2366f);
}

#[test]
fn every_built_in_theme_can_be_chosen_and_brings_its_own_accent() {
    let mut settings = Settings::parse("{}");
    for theme in themes::THEMES.iter() {
        select_theme(&mut settings, theme.id).unwrap();
        assert_eq!(effective_id(&settings), theme.id);
        assert_eq!(settings.accent, theme.accent.accent(), "{}", theme.id);
    }
    assert_eq!(themes::THEMES.len(), 10);
    // Sunrise's own stays the violet one, and an id from another launcher falls to it.
    select_theme(&mut settings, "sunrise").unwrap();
    assert_eq!(
        (effective_id(&settings).as_str(), settings.accent),
        ("sunrise", Accent::VIOLET)
    );
    let old = Settings::parse(r#"{"theme": "moonbase"}"#);
    assert_eq!(effective_id(&old), "sunrise");
    assert_eq!(saved(&old)["theme"], "moonbase");
    select_theme(&mut settings, "default").unwrap();
    assert_eq!(
        (settings.theme.as_deref(), effective_id(&settings).as_str()),
        (None, "dawn")
    );
}

#[test]
fn a_choice_that_is_not_there_changes_nothing_and_lists_what_is() {
    theme_files::set_loaded(vec![
        tidepool(0x8edbcb, None),
        Entry::Bad {
            folder: "broken".into(),
            reason: Reason::Version,
        },
    ]);
    let mut settings = Settings::parse(r#"{"theme": "nord"}"#);
    let before = saved(&settings);
    for wrong in [
        "moonbase",
        "local:broken",
        "local:gone",
        "local:",
        "Dawn",
        "",
    ] {
        let said = select_theme(&mut settings, wrong).unwrap_err();
        assert_eq!(saved(&settings), before, "{wrong}");
        for id in ["sunrise", "dawn", "nord", "taiga", "local:tidepool"] {
            assert!(said.contains(id), "{wrong}: {said}");
        }
        assert!(
            said.contains("broken: theme.toml must say version = 1."),
            "{said}"
        );
    }
    select_theme(&mut settings, " local:tidepool ").unwrap();
    assert_eq!(settings.theme.as_deref(), Some("local:tidepool"));
}

#[test]
fn an_accent_is_kept_per_theme_and_reset_goes_back_to_the_themes_own() {
    let mut settings = Settings::parse("{}");
    type_accent(&mut settings, "teal").unwrap();
    assert_eq!(settings.accent, Accent::TEAL);
    assert_eq!(settings.theme_accents["dawn"], Accent::TEAL);
    select_theme(&mut settings, "nord").unwrap();
    assert_eq!(settings.accent.rgb(), 0x88c0d0);
    type_accent(&mut settings, "#C2366F").unwrap();
    select_theme(&mut settings, "dawn").unwrap();
    assert_eq!(settings.accent, Accent::TEAL, "Dawn's own pick is back");
    select_theme(&mut settings, "nord").unwrap();
    assert_eq!(settings.accent.rgb(), 0xc2366f);
    // Reset forgets only this theme's.
    type_accent(&mut settings, "Theme").unwrap();
    assert_eq!(settings.accent.rgb(), 0x88c0d0);
    assert!(!settings.theme_accents.contains_key("nord"));
    assert_eq!(settings.theme_accents["dawn"], Accent::TEAL);
    // What isn't a colour changes nothing.
    let before = saved(&settings);
    assert!(type_accent(&mut settings, "mauve-ish")
        .unwrap_err()
        .contains("theme, violet"));
    assert_eq!(saved(&settings), before);
}

#[test]
fn a_colour_from_before_accents_were_kept_per_theme_survives_a_switch() {
    // An older command line set the accent and no map.
    let mut settings = Settings::parse(r##"{"theme": "nord", "accent": "#c2366f"}"##);
    assert_eq!(
        settings.accent.rgb(),
        0xc2366f,
        "a saved accent is honoured as it is read"
    );
    assert!(settings.theme_accents.is_empty());
    select_theme(&mut settings, "taiga").unwrap();
    assert_eq!(settings.accent, themes::TAIGA.accent.accent());
    select_theme(&mut settings, "nord").unwrap();
    assert_eq!(settings.accent.rgb(), 0xc2366f);
    // A theme left at its own colour leaves no entry behind.
    assert!(!settings.theme_accents.contains_key("taiga"));
    // A saved colour that happens to be the old Dawn accent is still the player's.
    let old = Settings::parse(r##"{"accent": "#f2a65e"}"##);
    assert_eq!(accent(&old).rgb(), 0xf2a65e);
    // An old light appearance still opens on Taiga.
    assert_eq!(
        effective_id(&Settings::parse(r#"{"appearance": "light"}"#)),
        "taiga"
    );
}

#[test]
fn a_players_theme_follows_its_file_until_an_accent_is_picked_under_it() {
    theme_files::set_loaded(vec![tidepool(0x8edbcb, None)]);
    let mut settings = Settings::parse("{}");
    select_theme(&mut settings, "local:tidepool").unwrap();
    assert_eq!(accent(&settings).rgb(), 0x8edbcb);
    assert_eq!(
        themes::accent_key(settings.theme.as_deref()),
        "local:tidepool"
    );
    // The file is edited and read again: the accent follows, though an older one was saved.
    theme_files::set_loaded(vec![tidepool(0x30a0ff, None)]);
    assert_eq!(accent(&settings).rgb(), 0x30a0ff);
    // Leaving it does not turn the file's old colour into a pick.
    select_theme(&mut settings, "nord").unwrap();
    assert!(!settings.theme_accents.contains_key("local:tidepool"));
    select_theme(&mut settings, "local:tidepool").unwrap();
    type_accent(&mut settings, "red").unwrap();
    theme_files::set_loaded(vec![tidepool(0x112233, None)]);
    assert_eq!(accent(&settings), Accent::RED, "a pick is the player's");
    reset_accent(&mut settings).unwrap();
    assert_eq!(accent(&settings).rgb(), 0x112233);
    assert!(!settings.theme_accents.contains_key("sunrise"));
}

#[test]
fn a_missing_theme_is_dawn_for_now_and_the_choice_stays() {
    theme_files::set_loaded(vec![tidepool(0x8edbcb, None)]);
    let mut settings = Settings::parse("{}");
    select_theme(&mut settings, "local:tidepool").unwrap();
    let chosen = saved(&settings);
    // Its file breaks.
    theme_files::set_loaded(vec![Entry::Bad {
        folder: "tidepool".into(),
        reason: Reason::Version,
    }]);
    assert!(missing(&settings));
    assert_eq!(effective_id(&settings), "dawn");
    assert_eq!(accent(&settings).rgb(), 0xb4552f);
    // Nothing of the fallback is saved, and its accent can't be edited meanwhile.
    assert!(type_accent(&mut settings, "teal").is_err());
    assert!(reset_accent(&mut settings).is_err());
    assert_eq!(saved(&settings), chosen);
    assert_eq!(saved(&Settings::parse(&chosen.to_string())), chosen);
    // Repaired and read again, it is back.
    theme_files::set_loaded(vec![tidepool(0x8edbcb, None)]);
    assert!(!missing(&settings));
    assert_eq!(effective_id(&settings), "local:tidepool");
    assert_eq!(accent(&settings).rgb(), 0x8edbcb);
}

#[test]
fn home_shows_the_players_picture_then_the_themes_then_the_scene() {
    let data = tempfile::tempdir().unwrap();
    let theirs = data.path().join("themes/tidepool/shore.png");
    theme_files::set_loaded(vec![tidepool(0x8edbcb, Some(theirs.clone()))]);
    let mut settings = Settings::parse("{}");
    assert_eq!(behind_home(&settings, data.path()), Behind::Scene);
    select_theme(&mut settings, "local:tidepool").unwrap();
    assert_eq!(
        behind_home(&settings, data.path()),
        Behind::Theme(theirs.clone())
    );
    // The player's own picture, kept by the launcher, comes first.
    let mine = data.path().join("mine.png");
    let mut bytes = Vec::new();
    image::RgbaImage::new(80, 80)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    std::fs::write(&mine, &bytes).unwrap();
    set_background(&mut settings, data.path(), &mine).unwrap();
    let kept = data.path().join(settings.background.as_deref().unwrap());
    assert_eq!(
        behind_home(&settings, data.path()),
        Behind::Personal(kept.clone())
    );
    assert!(mine.is_file(), "the original is the player's");
    // Scenery off shows neither, and forgets neither.
    settings.scenery_on = Some(false);
    assert_eq!(behind_home(&settings, data.path()), Behind::Plain);
    assert!(settings.background.is_some());
    settings.scenery_on = Some(true);
    // The kept copy is lost: the theme's shows, the name stays, and Appearance can say so.
    std::fs::remove_file(&kept).unwrap();
    assert_eq!(behind_home(&settings, data.path()), Behind::Theme(theirs));
    assert!(background_lost(&settings, data.path()));
    assert!(settings.background.is_some());
    // A built-in theme has only its scene.
    select_theme(&mut settings, "nord").unwrap();
    assert_eq!(behind_home(&settings, data.path()), Behind::Scene);
}

#[test]
fn a_new_picture_leaves_the_old_one_until_the_choice_is_saved() {
    let data = tempfile::tempdir().unwrap();
    let picture = |name: &str, shade: u8| {
        let path = data.path().join(name);
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(80, 80, image::Rgba([shade, 0, 0, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let mut settings = Settings::parse("{}");
    set_background(&mut settings, data.path(), &picture("one.png", 10)).unwrap();
    let first = settings.background.clone().unwrap();
    set_background(&mut settings, data.path(), &picture("two.png", 200)).unwrap();
    let second = settings.background.clone().unwrap();
    assert_ne!(first, second);
    assert!(data.path().join(&first).is_file(), "kept until the save");
    // A file that isn't a picture is refused and the choice stays.
    std::fs::write(data.path().join("notes.png"), "hello").unwrap();
    assert!(set_background(&mut settings, data.path(), &data.path().join("notes.png")).is_err());
    assert_eq!(settings.background.as_deref(), Some(second.as_str()));
    // After the save, only the launcher's own older copy goes.
    themes::retire_backgrounds(data.path(), settings.background.as_deref());
    assert!(!data.path().join(&first).exists());
    assert!(data.path().join(&second).is_file());
    assert!(data.path().join("one.png").is_file() && data.path().join("two.png").is_file());
}
