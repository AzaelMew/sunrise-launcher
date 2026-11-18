use super::*;

#[test]
fn a_fact_starts_with_a_capital() {
    assert_eq!(sentence("yesterday"), "Yesterday");
    assert_eq!(sentence(""), "");
}

#[test]
fn a_profile_says_its_play_time_only_once_it_was_played() {
    assert_eq!(played_line(Duration::from_secs(600), None), None);
    let now = std::time::SystemTime::now();
    assert_eq!(
        played_line(Duration::from_secs(3 * 3600 + 300), Some(now)),
        Some(("3 h 05 min played".into(), "Last played today".into()))
    );
}

#[test]
fn a_long_list_builds_the_rows_near_the_view() {
    // 100 rows 56 apart, the page showing the list's first 400 points.
    assert_eq!(near(100, 1, 56., (0., 400.)), 0..11);
    // Scrolled 2000 points down: those in view, three before and a few after.
    assert_eq!(near(100, 1, 56., (2000., 2400.)), 32..46);
    // The list is far under the view, or far over it: nothing of it is built.
    assert_eq!(near(100, 1, 56., (-900., -500.)), 0..0);
    assert_eq!(near(100, 1, 56., (9000., 9400.)), 100..100);
    // Just under the view: its first rows are there for Tab to land on.
    assert_eq!(near(100, 1, 56., (-500., -100.)), 0..2);
    // A short list is whole.
    assert_eq!(near(6, 1, 56., (0., 400.)), 0..6);
    // Tiles go by whole bands of four.
    assert_eq!(near(30, 4, 196., (400., 800.)), 0..30);
    assert_eq!(near(300, 4, 196., (1960., 2360.)), 28..64);
}

#[test]
fn the_strip_takes_over_once_play_is_scrolled_out_of_view() {
    let at = |top: f32, tall: f32| Bounds {
        origin: gpui::point(px(110.), px(top)),
        size: gpui::size(px(300.), px(tall)),
    };
    let view = at(78., 500.);
    // Not drawn yet, at rest, and cut by less than half: Play says it.
    assert!(play_shows(None, view));
    assert!(play_shows(Some(at(238., 50.)), view));
    assert!(play_shows(Some(at(60., 50.)), view));
    // Half gone or more: the strip says it, so progress and Stop never leave the window.
    assert!(!play_shows(Some(at(50., 50.)), view));
    assert!(!play_shows(Some(at(-400., 50.)), view));
}
