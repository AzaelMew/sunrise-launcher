//! Prebuilt build eligibility, independent of UI preferences.
use super::sdk_cache::Refusal;

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// Made for the build the instance plays.
    Prebuilt,
    /// Made for another build, and fits this one: the line saying so
    /// ("Made for nightly-2026-11-10. Fits this game (nightly-2026-11-13).").
    Fits {
        line: String,
    },
    /// The build can't use it (it doesn't fit, or works on its own build only): what the player
    /// reads, the headline, why, and what to do.
    Unfit {
        headline: String,
        line: String,
        actions: String,
    },
    Invalid {
        reason: String,
    },
    /// Made for another build, and its package carries no list of what it uses, so it can't be
    /// shown to fit this one: only its author can change that. Why, in the checker's words.
    Unproven {
        line: String,
    },
    /// Not checked: the build's SDK isn't available (offline, or Sunrise isn't installed); why, as the player reads it.
    Unchecked {
        reason: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routes {
    pub mods: Vec<(String, Route)>,
    pub unavailable: Option<Refusal>,
}
