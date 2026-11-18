//! The status strip: what Play is doing or what went wrong, with its one action, on every page
//! where Play itself is not in view. Home says the same beside Play, so the strip is drawn
//! there only once Home is scrolled past Play (`HomeState::play_in_view`): never both at once.
//! It is not drawn at all while there is nothing to tell (`launch_state::View::strip`).
use super::*;
use crate::frame::{button, Page};
use crate::launch_state::{Act, Main, StripAct};
use crate::motion::{self, Frame, Tween};
use crate::paint::palette;
use crate::sheets::Sheet;
use gpui::rgb;

#[derive(Default)]
pub(super) struct Presentation {
    active: bool,
    tint: Option<Tween>,
}

impl Launcher {
    /// The strip, for the frame to place above the page: one row, as wide as it is given.
    pub(super) fn status_strip(
        &mut self,
        time: &Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let home = self.ui.page == Page::Home;
        if !home {
            self.refresh_home_inventory(cx);
        }
        let view = self.launch_facts().view();
        let visible = !home || !self.ui.home.play_in_view();
        let operation = matches!(
            view.main,
            Main::Installing(_)
                | Main::Preparing
                | Main::Building
                | Main::Starting
                | Main::ChangingMods
                | Main::Stop
        );
        let installed_bar = self.ui.page == Page::Installed && self.ui.mods.build_button_drawn;
        let handoff = view.main == Main::Building
            && (installed_bar || self.ui.sheet == Some(Sheet::ModBuild));
        let status = &mut self.ui.status;
        if !visible || !operation || handoff || view.failed {
            status.tint = None;
        } else if !status.active {
            status.tint = Some(Tween::new(time.now, 0., 1., motion::FAST));
        }
        status.active = operation;
        if !visible {
            return None;
        }
        let tint = time
            .sample(&mut status.tint, window)
            .map_or(1., motion::arrival_opacity);
        let Some((says, act)) = view.strip() else {
            // Nothing runs or failed. A build that just ended in the background is said here
            // for a moment, with nothing to press.
            return self.build_said().map(|said| {
                strip(1.)
                    .child(
                        div()
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .bg(rgb(palette().success)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(said))
            });
        };
        let id = match act {
            StripAct::Act(Act::StopBuild) => "alt-strip-stop-build",
            StripAct::StopGame => "alt-strip-stop",
            StripAct::Act(_) => "alt-strip-fix",
            StripAct::Home => "alt-strip-home",
        };
        // A build draws its own line: the stage it is at over the three stages' bar.
        // Mods › Installed shows the bar in its Build button: the line is drawn without one,
        // unless a search or filter hides that button.
        let bar_here = !installed_bar;
        let line = match self
            .build_line(bar_here, cx)
            .filter(|_| view.main == Main::Building)
        {
            Some(build) => build.into_any_element(),
            None => div().truncate().child(says).into_any_element(),
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
            strip(tint)
                .child(div().flex_none().size(px(8.)).rounded_full().bg(rgb(mark)))
                .child(div().flex_1().min_w_0().child(line))
                .children(bar)
                .child(
                    button(id, act.label()).on_click(cx.listener(move |this, _, _, cx| {
                        if home && act == StripAct::Home {
                            // Already there: back up to Play.
                            crate::frame::scrolls("home").set_offset(gpui::Point::default());
                        }
                        this.strip_act(&act);
                        cx.notify();
                    })),
                ),
        )
    }
}

/// The strip's own plate: one row, as wide as it is given.
fn strip(tint: f32) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(tokens::ACTION))
        .px(px(14.))
        .py(px(6.))
        .rounded(px(tokens::R_CARD))
        .bg(palette().card().opacity(tint))
        .border_1()
        .border_color(rgb(palette().divider))
        .text_size(px(tokens::LABEL))
        .text_color(rgb(palette().text))
}
