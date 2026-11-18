//! Settings' accent colour and Home background rows (`sunrise_launcher::themes`).
use super::*;



use gpui::{
    canvas, fill, point, size};
use std::path::PathBuf;
use sunrise_launcher::themes::{self};


impl Launcher {
    /// The theme Settings ask for, under the window's own light or dark: the player's look,
    /// under the launcher theme's sky if they picked one.
    pub(super) fn settings_theme(&self, window: &Window) -> Theme {
        crate::theme::from_settings(&self.settings, window.appearance())
    }

    /// The picture behind Home, if the player keeps one.
    /// Home's scenery, when it shows: the player's picture, else the theme's scene (Sunrise's
    /// coast for its own theme and Dawn's, the painted sky for the others).
    pub(super) fn home_cover(&self) -> Option<crate::window_root::Cover> {
        use crate::window_root::Cover;
        if !self.theme.glass || !self.outline_sides() || !self.settings.scenery() {
            return None;
        }
        if let Some(picture) = self.home_background() {
            return Some(Cover::Picture(picture));
        }
        Some(match self.settings.theme.as_deref() {
            None => Cover::Coast(sunrise_launcher_scenery::coast::SUNRISE),
            Some("dawn") => Cover::Coast(sunrise_launcher_scenery::coast::DAWN),
            Some(id) => Cover::Sky {
                rains: id == "hypr_neon",
            },
        })
    }

    pub(super) fn home_background(&self) -> Option<PathBuf> {
        themes::background(&backend::data_dir(), self.settings.background.as_deref())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

}
