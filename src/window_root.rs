//! The window's root: the launcher, drawn from its last frame (GPUI Fast retains views).
use crate::Launcher;
use gpui::{div, prelude::*, Context, Entity, IntoElement, Render, Window};

pub struct Shell {
    launcher: Entity<Launcher>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let launcher = cx.new(|cx| Launcher::new(window, cx));
        Self { launcher }
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            // Cached: the launcher is drawn again only when it is notified.
            .child(
                self.launcher
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
    }
}
