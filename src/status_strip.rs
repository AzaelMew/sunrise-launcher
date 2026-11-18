//! The status strip: on every page but Home, what Play is doing or what went wrong, with its
//! one action. Home says the same beside Play itself, so the strip is not drawn there, and it
//! is not drawn at all while there is nothing to tell (`launch_state::View::strip`).
use super::*;
use crate::frame::button;
use crate::launch_state::{Act, Main, StripAct};
use crate::paint::palette;
use gpui::rgb;

impl Launcher {
    /// The strip, for the frame to place above the page: one row, as wide as it is given.
    pub(crate) fn status_strip(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if self.ui.page == crate::frame::Page::Home {
            return None;
        }
        let view = self.launch_facts().view();
        let (says, act) = view.strip()?;
        let id = match act {
            StripAct::Act(Act::StopBuild) => "alt-strip-stop-build",
            StripAct::StopGame => "alt-strip-stop",
            StripAct::Act(_) => "alt-strip-fix",
            StripAct::Home => "alt-strip-home",
        };
        // A build draws its own line: the stage it is at over the three stages' bar.
        let line = match self.build_line().filter(|_| view.main == Main::Building) {
            Some(build) => build,
            None => div().truncate().child(says),
        };
        let mark = if view.failed {
            palette().warning
        } else {
            palette().accent
        };
        // A download says how far it is in the label; its bar shows it too.
        let bar = match view.main {
            Main::Installing(progress) => Some(
                div()
                    .flex_none()
                    .w(px(120.))
                    .h(px(4.))
                    .rounded(px(tokens::R_BAR))
                    .bg(rgb(palette().line))
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(4.))
                            .rounded(px(tokens::R_BAR))
                            .bg(rgb(palette().accent))
                            .w(relative(progress.clamp(0.03, 1.))),
                    ),
            ),
            _ => None,
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(tokens::ACTION))
                .px(px(14.))
                .py(px(6.))
                .rounded(px(tokens::R_CARD))
                .bg(palette().card())
                .border_1()
                .border_color(rgb(palette().divider))
                .text_size(px(tokens::LABEL))
                .text_color(rgb(palette().text))
                .child(div().flex_none().size(px(8.)).rounded_full().bg(rgb(mark)))
                .child(div().flex_1().min_w_0().child(line))
                .children(bar)
                .child(
                    button(id, act.label()).on_click(cx.listener(move |this, _, _, cx| {
                        this.strip_act(&act);
                        cx.notify();
                    })),
                ),
        )
    }
}
