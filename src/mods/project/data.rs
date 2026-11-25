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
