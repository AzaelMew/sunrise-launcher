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
