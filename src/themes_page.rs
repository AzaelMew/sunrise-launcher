//! Settings' accent colour and Home background rows (`sunrise_launcher::themes`).
use super::*;








impl Launcher {
    /// The theme Settings ask for, under the window's own light or dark: the player's look,
    /// under the launcher theme's sky if they picked one.
    pub(super) fn settings_theme(&self, window: &Window) -> Theme {
        crate::theme::from_settings(&self.settings, window.appearance())
    }

}

