//! The right sidebar, the left one's mirror: the player's side of the top bar in its top cell,
//! then the selected instance's sections (installed mods, worlds, saved servers, packs, shaders,
//! screenshots, the log) as rows with their counts, and how many play at its foot. A row slides
//! its section in at Home's right edge; the same row or the panel's close puts it away. Narrow
//! windows keep the sections as an icon column and the player's side in the top bar.
use super::*;
use crate::glass_ui::{ white};
use home_tabs::HomeTab;

use sunrise_launcher_theme::glass::{ink, radius};

/// From this window width the right sidebar is as wide as the left; narrower, it is an icon
/// column [`COMPACT`] wide.
const FULL_FROM: f32 = 1180.;
pub(crate) const COMPACT: f32 = 64.;
/// The open section's panel is this wide where the page has the room, and never narrower
/// than [`PANEL_MIN`].
const PANEL: f32 = 520.;
const PANEL_MIN: f32 = 320.;
/// How far the panel slides in from.
const SLIDE: f32 = 16.;

impl Launcher {
    /// Whether the window is wide enough for both sidebars at full width (the right one then
    /// holds the player's side of the top bar); narrower, both fold to icon columns.
    pub(super) fn outline_wide(&self) -> bool {
        self.window_width >= FULL_FROM
    }

    /// Whether the sidebars show: on Home, and on Settings opened over it. The other pages have
    /// the window to themselves.
    pub(super) fn outline_sides(&self) -> bool {
        self.page == Page::Play || (self.page == Page::Settings && self.behind == Page::Play)
    }

    /// How wide each sidebar is (none where they don't show).
    pub(super) fn outline_side_width(&self) -> f32 {
        if !self.outline_sides() {
            0.
        } else if self.outline_wide() {
            frame::SIDEBAR
        } else {
            COMPACT
        }
    }

}

/// A section's name and its icon.
fn section(tab: HomeTab) -> (&'static str, &'static str) {
    match tab {
        HomeTab::Overview => (tr("Overview"), "dashboard-square-01"),
        HomeTab::Mods => (tr("Installed mods"), "puzzle"),
        HomeTab::Packs => (tr("Resource packs"), "image-01"),
        HomeTab::Shaders => (tr("Shaders"), "flash"),
        HomeTab::Worlds => (tr("Worlds"), "earth"),
        HomeTab::Multiplayer => (tr("Saved servers"), "user-multiple-02"),
        HomeTab::Screenshots => (tr("Screenshots"), "view"),
        HomeTab::Console => (tr("Log"), "computer-terminal-01"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
