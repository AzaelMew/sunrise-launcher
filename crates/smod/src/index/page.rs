//! Project and collection page limits shared by the index reader.

pub const MAX_SCREENSHOTS: usize = 8;
pub const MAX_DESCRIPTION: usize = 10_000;

/// The title and one-line summary rules shared by project and collection pages.
pub(super) fn check_title(name: &str, summary: &str) -> Vec<String> {
    const MAX_SUMMARY: usize = 160;
    let mut errors = Vec::new();
    if name.trim().is_empty() {
        errors.push("has no name".into());
    }
    if summary.trim().is_empty() {
        errors.push("has no summary".into());
    } else if summary.chars().count() > MAX_SUMMARY {
        errors.push(format!("the summary is over {MAX_SUMMARY} characters"));
    }
    errors
}

pub(super) fn check_description_limit(description: &str) -> Option<String> {
    let length = description.trim().chars().count();
    (length > MAX_DESCRIPTION)
        .then(|| format!("the description is {length} characters: at most {MAX_DESCRIPTION}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_and_description_limits_reject_empty_or_oversize_values() {
        assert!(check_title("Lanterns", "Warm lights for dark caves.").is_empty());
        assert_eq!(check_title("   ", "ok"), ["has no name"]);
        assert_eq!(check_title("Lanterns", "   "), ["has no summary"]);
        assert_eq!(
            check_title("Lanterns", &"a".repeat(161)),
            ["the summary is over 160 characters"]
        );
        assert!(check_description_limit(&"a".repeat(MAX_DESCRIPTION)).is_none());
        assert!(
            check_description_limit(&"a".repeat(MAX_DESCRIPTION + 1))
                .unwrap()
                .contains("at most 10000")
        );
    }
}
