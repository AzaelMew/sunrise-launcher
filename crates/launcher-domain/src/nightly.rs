//! Nightly version names carry their UTC build day (`nightly-2026-11-20-0c02067`); the rest of the
//! launcher reads that day for ordering and display, so the rule lives beside the ordering that
//! uses it.
use sunrise_launcher_release_verify::NIGHTLY;

/// The date of a nightly's version (`nightly-2026-10-28-0c02067`), or None for any other version.
pub fn date_of(version: &str) -> Option<&str> {
    let date = version
        .strip_prefix(NIGHTLY)?
        .strip_prefix('-')?
        .get(..10)?;
    is_date(date).then_some(date)
}

/// A `YYYY-MM-DD` day.
pub fn is_date(text: &str) -> bool {
    text.len() == 10
        && text.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_comes_from_a_dated_nightly_version_only() {
        assert_eq!(date_of("nightly-2026-10-28-0c02067"), Some("2026-10-28"));
        assert_eq!(date_of("nightly-2026-10-28"), Some("2026-10-28"));
        for other in [
            "nightly",
            "nightly-2026-10-2",
            "nightly-26-10-28-0c02067",
            "dev-2026-10-28-0c02067",
            "26.3-0.1.0",
            "",
        ] {
            assert_eq!(date_of(other), None, "{other}");
        }
    }
}
