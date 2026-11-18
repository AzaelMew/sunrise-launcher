//! A stand-in for the glass frame's navigation, so the account chip runs on the R4 frame until
//! the frame brings its own `navigate`. Not part of the account chip's patch.
use super::*;
use crate::frame::Page;

impl Launcher {
    pub(crate) fn navigate(&mut self, page: Page) {
        match page {
            Page::Discover => self.open_mods(),
            Page::Packs => self.open_packs(),
            _ => self.ui.page = page,
        }
        self.ui.picker = false;
    }
}
