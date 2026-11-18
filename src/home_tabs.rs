//! The selected instance's tabs under Home's hero (docs/design-system.md, "Night"; plan R0, screen
//! 5): Overview (what Play needs), then its worlds, servers, packs, screenshots and, once the game
//! has said something, its console. One at a time, instead of one long page under the hero.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HomeTab {
    #[default]
    Overview,
    Mods,
    Worlds,
    Multiplayer,
    /// Resource packs.
    Packs,
    Shaders,
    Screenshots,
    Console,
}

impl HomeTab {
    /// `SUNRISE_LAUNCHER_PAGE=worlds` (or `instance-mods`, `multiplayer`, `packs`, `screenshots`,
    /// `console`) opens Home on that tab, for screenshots.
    pub(super) fn from_env() -> Self {
        match std::env::var("SUNRISE_LAUNCHER_PAGE").as_deref() {
            Ok("instance-mods") => HomeTab::Mods,
            Ok(page) if page.starts_with("mod-") => HomeTab::Mods,
            Ok("worlds") => HomeTab::Worlds,
            Ok("multiplayer") => HomeTab::Multiplayer,
            Ok("packs") => HomeTab::Packs,
            Ok("shader-packs") => HomeTab::Shaders,
            Ok("screenshots") => HomeTab::Screenshots,
            Ok("console") => HomeTab::Console,
            _ => HomeTab::Overview,
        }
    }
}


/// A tab's buttons and content, as the strip and the page take them.
fn split(tab: (Div, impl IntoElement)) -> (Option<Div>, AnyElement) {
    (Some(tab.0), tab.1.into_any_element())
}
