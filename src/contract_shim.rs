//! SCAFFOLD, not part of the hand-over: stand-ins for the three methods the frame's owner
//! (launcher-redesign-impl) provides, with the agreed signatures, so this branch runs end to
//! end. Dropped when the real ones land.
use super::*;
use crate::paint::palette;
use gpui::rgb;

impl Launcher {
    pub(crate) fn navigate(&mut self, page: crate::frame::Page) {
        self.ui.page = page;
        self.ui.picker = false;
    }

    pub(crate) fn switch_profile(
        &mut self,
        instance: &instances::Instance,
        cx: &mut Context<Self>,
    ) {
        if !self.launch_busy() && self.client.is_none() {
            self.select_instance(instance);
            self.client_error = None;
        }
        cx.notify();
    }

    pub(crate) fn profile_mark(&mut self, instance: &instances::Instance, size: f32) -> Div {
        let picture = instance.home.join(instances::PICTURE);
        let key = format!("picture:{}", picture.display());
        let image = self.icon_image(key, size, false, || std::fs::read(&picture).ok());
        let tile = div()
            .flex_none()
            .size(px(size))
            .rounded(px(tokens::R_CARD))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center();
        match image {
            Some(image) => tile.child(gpui::img(image).size(px(size))),
            None if instance.kind == instances::Kind::Default => tile.child(crate::mark(size)),
            None => tile
                .bg(rgb(palette().selected))
                .text_size(px(size * 0.45))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(palette().text))
                .child(crate::join::initial(&instance.name)),
        }
    }
}
