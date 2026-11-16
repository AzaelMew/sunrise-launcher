//! The Outline look's top bar (`docs/design-system.md`, "Outline"): the mark over the rail, the
//! pages centred on the window as text with a line under the open one, then search and the player
//! (or Sign in with Microsoft) on the right.
use super::*;
use crate::glass_ui::{self as ui, white};
use sunrise_theme::glass::{fill, ink};

/// How wide `text` is drawn, so the bar fits each part in the language shown.
fn line_width(window: &Window, family: &str, weight: FontWeight, size: f32, text: &str) -> f32 {
    let mut font = gpui::font(SharedString::from(family.to_owned()));
    font.weight = weight;
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        px(size),
        &[run],
        None,
    );
    f32::from(line.width).ceil()
}

impl Launcher {
    pub(super) fn outline_topbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tight = self.window_width < 1280.;
        let (tab_size, tab_gap) = if tight { (14., 14.) } else { (15., 22.) };
        let tabs = Page::in_top_bar()
            .map(|(_, label, _)| {
                line_width(
                    window,
                    sunrise_theme::glass::FONT,
                    FontWeight::SEMIBOLD,
                    tab_size,
                    tr(label),
                )
            })
            .sum::<f32>()
            + tab_gap * Page::in_top_bar().count().saturating_sub(1) as f32;
        let mut pages = div().flex().flex_none().items_center().gap(px(tab_gap));
        for (page, label, _) in Page::in_top_bar() {
            pages = pages.child(self.outline_page_tab(page, label, tab_size, cx));
        }

        let mono_width = |text: &str| {
            line_width(
                window,
                crate::theme::MONO,
                FontWeight::NORMAL,
                12.,
                &text.to_uppercase(),
            )
        };
        let signed_in = self.accounts.selected().is_some();
        let sign_in_width = |label: &str| 2. + 24. + 14. + 8. + mono_width(tr(label));
        let right_of = |short: bool| {
            let account = match (signed_in, short) {
                (true, _) => 61.,
                (false, true) => sign_in_width("Sign in"),
                (false, false) => sign_in_width("Sign in with Microsoft"),
            };
            38. + 12. + account
        };
        let side = frame::RAIL;
        // Narrower, "with Microsoft" goes first: the pages keep their room.
        let room = self.window_width - side - 2. * frame::MARGIN - 32.;
        let short = !signed_in && room - right_of(false) < tabs;
        let right = right_of(short);
        let half = self.window_width / 2.;
        let centred = half - tabs / 2. >= side + 24.
            && half + tabs / 2. <= self.window_width - frame::MARGIN - right - 16.;

        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .h(px(64.))
            .pr(px(frame::MARGIN + 8.))
            .border_b_1()
            .border_color(ui::line())
            // Over the rail, its top cell: the mark.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(side))
                    .h_full()
                    .border_r_1()
                    .border_color(ui::line())
                    .child(self.sunrise_mark(22.)),
            )
            // On the window's centre where the pages clear both sides; else between them,
            // scrolling sideways when too long.
            .map(|d| {
                let pages = pages.id("nav-pages");
                if centred {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(pages),
                    )
                    .child(div().flex_1())
                } else {
                    d.child(
                        div().flex_1().min_w_0().overflow_hidden().child(
                            pages
                                .overflow_x_scroll()
                                .on_scroll(|_, _, cx| cx.stop_propagation()),
                        ),
                    )
                }
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(12.))
                    .child(self.outline_search(cx))
                    .child(self.outline_account(short, cx)),
            )
    }

    /// A page in the top bar: its name, a line under it while open, a faint one under the
    /// pointer.
    fn outline_page_tab(
        &self,
        page: Page,
        label: &'static str,
        size: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = self.page == page;
        let id = SharedString::from(format!("nav-{label}"));
        let underline = if open {
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(px(2.))
                .bg(white(ink::FULL))
                .into_any_element()
        } else {
            hover_fade(
                &id,
                || div().absolute().bottom_0().left_0().right_0().h(px(2.)),
                |d, a| d.bg(white(0.35 * a)),
            )
        };
        keyboard(
            track_hover(div().id(id.clone()), id)
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .h(px(34.))
                .child(underline)
                .active(|s| s.opacity(theme::PRESSED))
                .cursor_pointer()
                .font_family(sunrise_theme::glass::FONT)
                .text_size(px(size))
                .font_weight(if open {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(white(if open { ink::FULL } else { ink::NOTE }))
                .hover(|s| s.text_color(white(ink::FULL)))
                .child(tr(label))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.go(page);
                    cx.notify();
                })),
            &self.theme,
        )
    }

    /// Search: the palette (⌘K or Ctrl+K anywhere), as a square outlined button.
    fn outline_search(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let key = if cfg!(target_os = "macos") {
            "⌘K"
        } else {
            tr("Ctrl K")
        };
        keyboard(
            hoverable("topbar-search", fill::BUTTON)
                .child(edge("topbar-search"))
                .flex()
                .items_center()
                .justify_center()
                .size(px(38.))
                .cursor_pointer()
                .child(ui::icon("search-01", 17., white(ink::SOFT)).relative())
                .tooltip(ui::tip(format!("{} ({key})", tr("Search…"))))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.palette_toggle(window, cx);
                    cx.notify();
                })),
            &self.theme,
        )
    }

    /// The player's face and its menu, or "Sign in with Microsoft" while nobody is signed in.
    fn outline_account(&self, short: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let Some(face) = self.accounts.selected().map(|a| a.uuid.clone()) else {
            return keyboard(
                hoverable("topbar-sign-in", fill::BUTTON)
                    .child(edge("topbar-sign-in"))
                    .font_family(crate::theme::MONO)
                    .text_size(px(12.))
                    .text_color(white(ink::FULL))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(12.))
                    .cursor_pointer()
                    .child(microsoft::logo(14.))
                    .child(
                        tr(if short {
                            "Sign in"
                        } else {
                            "Sign in with Microsoft"
                        })
                        .to_uppercase(),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.sign_in_to_play();
                        cx.notify();
                    })),
                &t,
            )
            .into_any_element();
        };
        let (name, _) = self.who_plays();
        let open = self.account_menu;
        let chip = hoverable("account", fill::HOVER)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(38.))
            .px(px(6.))
            .bg(white(if open { fill::ACTIVE } else { 0. }))
            .cursor_pointer()
            .tooltip(ui::tip(name))
            .child(
                div()
                    .relative()
                    .size(px(26.))
                    .overflow_hidden()
                    .child(self.profile_heads.draw(Some(&face), 26., self.scale, &t)),
            )
            .child(ui::icon("arrow-down-01", 15., white(ink::FAINT)).relative())
            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                if this.closed_by_this_press(event) {
                    return;
                }
                this.account_menu = !this.account_menu;
                if this.account_menu {
                    this.accounts = Accounts::load();
                    this.load_profile_heads();
                }
                cx.notify();
            }));
        div()
            .relative()
            .child(keyboard(chip, &t))
            .children(self.glass_account_menu(cx))
            .into_any_element()
    }
}

/// An Outline control: a `fill` of white that eases in under the pointer, and a press that dims
/// it.
pub(super) fn hoverable(id: impl Into<SharedString>, fill: f32) -> Stateful<Div> {
    let id = id.into();
    track_hover(
        div()
            .id(id.clone())
            .relative()
            .child(hover_layer(&id, white(fill), 0., 0.))
            .active(|s| s.opacity(theme::PRESSED)),
        id,
    )
}

/// The Outline look's line, brightening while `id` is hovered.
pub(super) fn edge(id: impl Into<SharedString>) -> AnyElement {
    let id = id.into();
    hover_fade(
        &id,
        || div().absolute().top_0().left_0().size_full().border_1(),
        |d, a| d.border_color(white(0.12 + 0.28 * a)),
    )
}
