//! Stand-ins for the glass frame's side of the contract, so the account chip and the search run
//! on the R4 frame until the frame brings its own. Not part of their patches.
use super::*;
use crate::frame::Page;
use crate::paint::palette;
use gpui::rgb;
use sunrise_launcher::instances::Instance;

impl Launcher {
    pub(crate) fn navigate(&mut self, page: Page) {
        match page {
            Page::Discover => self.open_mods(),
            Page::Packs => {
                self.open_packs();
                self.ui.page = Page::Packs;
            }
            // The R4 frame has no page for these: Home has Join, the sheet has the versions.
            Page::Join => self.ui.page = Page::Home,
            Page::Versions => {
                self.ui.page = Page::Home;
                self.open_sheet(crate::sheets::Sheet::Versions);
            }
            _ => self.ui.page = page,
        }
        self.ui.picker = false;
    }

    pub(crate) fn switch_profile(&mut self, profile: &Instance, _cx: &mut Context<Self>) {
        if !self.launch_busy() && self.client.is_none() {
            self.select_instance(profile);
            self.client_error = None;
        }
    }

    pub(crate) fn profile_mark(&mut self, profile: &Instance, size: f32) -> Div {
        div()
            .flex_none()
            .size(px(size))
            .rounded(px(tokens::R_SMALL))
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(palette().selected))
            .text_size(px(size * 0.5))
            .font_weight(FontWeight::BOLD)
            .child(crate::home::initial(&profile.name))
    }
}
