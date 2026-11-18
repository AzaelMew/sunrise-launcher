//! Home's head in the glass look (the mockup's `HomeView`, `launcher-views.tsx`): the instance's
//! edition, its icon and name, how long it was played, then Play with its menu, the version, the
//! instance's mods and its folders, and joining a friend on the right. Dark appearance only.

use crate::glass_ui::{ white};
use gpui::deferred;
use sunrise_launcher_theme::glass::{fill, ink, radius};


/// The Version menu's state (Home's picker).
#[derive(Default)]
pub(super) struct PickerState {
    /// The menu is open (the old look's; the glass look keeps versions in instance settings).
    pub(super) open: bool,
}

/// Which of Home's menus is open.
#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum HomeMenu {
    #[default]
    None,
    Play,
    Folder,
}


#[cfg(test)]
mod tests {
    use super::*;

}
