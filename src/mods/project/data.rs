//! Data and labels for a v2 registry mod's page.
use super::*;

/// The profile and exact build a mod's versions are shown for, as one label.
pub(super) fn target_label(profile_name: &str, build_label: Option<&str>) -> String {
    match build_label {
        Some(build) => trf(
            "{profile} ({build})",
            &[("profile", profile_name), ("build", build)],
        ),
        None => profile_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_label_names_the_profile_and_only_an_exact_build_when_known() {
        // The key is English and its placeholders are language-independent; keep the test English.
        crate::i18n::set(crate::i18n::Language::English);
        assert_eq!(target_label("Sunrise 1.3", None), "Sunrise 1.3");
        assert_eq!(
            target_label("Sunrise 1.3", Some("b42")),
            "Sunrise 1.3 (b42)"
        );
        // Current behaviour, recorded: an empty build is passed through, so the label shows "()".
        // If a caller starts skipping empty builds, change this expectation with it.
        assert_eq!(target_label("Sunrise 1.3", Some("")), "Sunrise 1.3 ()");
    }
}
