//! The Outline look's top bar (`docs/design-system.md`, "Outline"): the mark and "Sunrise" on the
//! left, the pages centred as text with a line under the open one, then how many play, the player's
//! face and Settings on the right.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use sunrise_theme::glass::{fill, ink, radius};

/// The brand's width: the 22 px mark, 10 px, then "Sunrise" at 17 px bold; the mark alone.
const BRAND: f32 = 130.;
const MARK: f32 = 22.;

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
        let t = self.theme;

        let tight = self.window_width < 1280.;
        let (tab_size, tab_gap) = if tight { (14., 14.) } else { (15., 22.) };
        let in_bar: Vec<_> = Page::in_top_bar().collect();
        let tabs = in_bar
            .iter()
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
            + tab_gap * in_bar.len().saturating_sub(1) as f32;
        let mut pages = div().flex().items_center().gap(px(tab_gap));
        for (page, label, _) in in_bar {
            let open = self.page == page;
            let id = SharedString::from(format!("nav-{label}"));
            // The open page's line is simply there; another page's fades in faintly under the
            // pointer.
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
            pages = pages.child(keyboard(
                track_hover(div().id(id.clone()), id)
                    .relative()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .child(underline)
                    .active(|s| s.opacity(theme::PRESSED))
                    .cursor_pointer()
                    .font_family(sunrise_theme::glass::FONT)
                    .text_size(px(tab_size))
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
                &t,
            ));
        }

        let side = crate::frame::SIDEBAR + crate::frame::MARGIN;
        let signed_in = self.accounts.selected().is_some();
        let mono_width = |text: &str| {
            line_width(
                window,
                crate::theme::MONO,
                FontWeight::NORMAL,
                12.,
                &text.to_uppercase(),
            )
        };
        let count_width = self.playing.map(|n| {
            6. + 8. + mono_width(&trf("{count} playing now", &[("count", &thousands(n))]))
        });
        let sign_in_width = |label: &str| 2. + 24. + 14. + 8. + mono_width(tr(label));
        // The right side: settings, then the face or the sign-in, then the count, 14 px apart.
        let right_of = |count: bool, short: bool| {
            let account = if signed_in {
                61.
            } else if short {
                sign_in_width("Sign in")
            } else {
                sign_in_width("Sign in with Microsoft")
            };
            let count = count_width.filter(|_| count).map_or(0., |w| w + 14.);
            36. + 14. + account + count
        };
        // Narrower, the players count goes first, then "with Microsoft": the pages keep their room.
        let room = self.window_width - side - 48. - BRAND - 48.;
        let count = count_width.is_some() && room - right_of(true, false) >= tabs;
        let short = !signed_in && room - right_of(false, false) < tabs;
        let right = right_of(count, short);
        // Narrower still, the brand gives way to the pages: its word, then its mark.
        let fits = |brand: f32| self.window_width - side - 48. - brand - 48. - right >= tabs;
        let (word, mark) = if fits(BRAND) {
            (true, true)
        } else {
            (false, fits(MARK))
        };
        let brand_width = match (word, mark) {
            (true, _) => BRAND,
            (false, true) => MARK,
            _ => -24.,
        };
        let brand = mark.then(|| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .child(self.sunrise_mark(22.))
                .when(word, |d| {
                    d.child(ui::text(17., FontWeight::BOLD, ink::FULL).child("Sunrise"))
                })
        });
        let half = self.window_width / 2.;
        let centred = half - tabs / 2. >= side + 24. + brand_width + 16.
            && half + tabs / 2. <= self.window_width - 24. - right - 16.;
        let (pages_layer, pages_flow) = if centred {
            (Some(pages.flex_none()), None)
        } else {
            (None, Some(pages.flex_none()))
        };
        // Between brand and buttons, the pages scroll in the strip with arrows at hidden ends.
        let performance = self.settings.performance();
        let flow = pages_flow.map(|p| self.nav_strip.render(p, &t, performance, window));
        let playing = self.playing.filter(|_| count).map(|count| {
            let url = telemetry::stats_url(&self.links.website);
            mono(12., ink::NOTE)
                .id("playing-now")
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(|s| s.text_color(white(ink::FULL)))
                .child(div().size(px(6.)).bg(ui::live()))
                .child(trf("{count} playing now", &[("count", &thousands(count))]).to_uppercase())
                .on_click(move |_: &ClickEvent, _, cx| {
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                })
        });

        let face = self.accounts.selected().map(|a| a.uuid.clone());
        let open = self.account_menu;
        let player = face.is_some().then(|| {
            let chip = hoverable("account", fill::HOVER)
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(36.))
                .px(px(6.))
                .bg(white(if open { fill::ACTIVE } else { 0. }))
                .cursor_pointer()
                .child(
                    div()
                        .size(px(26.))
                        .overflow_hidden()
                        .child(
                            self.profile_heads
                                .draw(face.as_deref(), 26., self.scale, &t),
                        ),
                )
                .child(ui::icon("arrow-down-01", 15., white(ink::FAINT)))
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
        });
        let sign_in = face.is_none().then(|| {
            keyboard(
                hoverable("topbar-sign-in", fill::BUTTON)
                    .child(edge("topbar-sign-in"))
                    .font_family(crate::theme::MONO)
                    .text_size(px(12.))
                    .text_color(white(ink::FULL))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .border_1()
                    .border_color(ui::line())
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
        });
        let settings = ui::icon_button(
            "nav-Settings",
            "sliders-horizontal",
            36.,
            18.,
            radius::CONTROL,
            self.page == Page::Settings,
            &t,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.go(if this.page == Page::Settings {
                Page::Play
            } else {
                Page::Settings
            });
            cx.notify();
        }));

        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(px(56.))
            .px(px(24.))
            .mt(px(8.))
            .gap(px(24.))
            // On the window's centre (the bar starts right of the sidebar) where the pages clear
            // the brand and the buttons; else between them, scrolling sideways when too long.
            .children(pages_layer.map(|pages| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-side))
                    .right_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pages)
            }))
            .children(brand)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .children(flow),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(14.))
                    .children(playing)
                    .children(player)
                    .children(sign_in)
                    .child(settings),
            )
    }
}

impl Launcher {
    /// The sidebar's foot in the Outline look: Discord, Documentation and Website as icons, the
    /// launcher's version on the right.
    pub(super) fn outline_side_links(&self) -> Div {
        let t = self.theme;
        let icon = |id: &'static str, icon: &'static str, url: String| {
            keyboard(
                hoverable(id, fill::BUTTON)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(30.))
                    .cursor_pointer()
                    .child(ui::icon(icon, 16., white(ink::SOFT)))
                    .on_click(move |_, _, cx| cx.open_url(&url)),
                &t,
            )
        };
        let mut icons = div().flex().items_center().gap(px(2.));
        for (id, name, url) in [
            ("discord", "discord", &self.links.discord),
            ("docs", "book", &self.links.docs),
            ("website", "globe-02", &self.links.website),
        ] {
            if !url.is_empty() {
                icons = icons.child(icon(id, name, url.clone()));
            }
        }
        div()
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .mt(px(12.))
            .px(px(4.))
            .py(px(4.))
            .child(crate::glass::backdrop(crate::glass::Kind::Glass, 0.))
            .child(icons.relative())
            .child(
                mono(12., ink::DIM)
                    .id("launcher-version")
                    .relative()
                    .pr(px(8.))
                    .child(backend::VERSION),
            )
    }
}

/// An Outline control: a `fill` of white that eases in under the pointer over the design's
/// quick curve, and a press that dims it, as the glass look's controls do.
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

impl Launcher {
    /// The Outline look's sidebar, 248 wide: Instances with its +, the instances, the player's
    /// servers, then the launcher's update and the links with the launcher's version.
    pub(super) fn outline_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let soft = self.look.accent_soft;
        self.load_packs();
        let add = ui::icon_button(
            "add-instance",
            "plus-sign",
            36.,
            16.,
            radius::CONTROL,
            false,
            &t,
        )
        .tooltip(ui::tip(tr("Add instance")))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.open_instance_dialog(None);
            cx.notify();
        }));
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(8.))
            .child(mono(12., ink::NOTE).child(tr("Instances").to_uppercase()))
            .child(add);
        let mut list = div()
            .id("side-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(10.))
            .children(self.glass_instances(cx));

        let servers: Vec<_> = self
            .servers
            .sidebar_list()
            .map(<[_]>::to_vec)
            .unwrap_or_else(|| self.side_servers.clone());
        list = list.child(
            mono(12., ink::NOTE)
                .px(px(6.))
                .pt(px(28.))
                .pb(px(6.))
                .child(tr("Servers").to_uppercase()),
        );
        if servers.is_empty() {
            list = list.child(
                ui::text(13., FontWeight::NORMAL, ink::DIM)
                    .px(px(6.))
                    .child(tr("None yet.")),
            );
        }
        let mut server_rows = div().flex().flex_col().gap(px(4.));
        for e in &servers {
            let up = self.servers.is_up(&e.dir);
            let open = self.page == Page::Servers && self.servers.open.as_ref() == Some(&e.dir);
            let dir = e.dir.clone();
            server_rows = server_rows.child(
                ui::row(format!("side-server-{}", e.dir.display()), open, 8., &t)
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(38.))
                            .bg(white(fill::BUTTON))
                            .child(ui::icon("computer-terminal-01", 18., white(ink::STRONG))),
                    )
                    .child(ui::row_text(
                        e.name.clone(),
                        if up {
                            (ui::Tone::Live, tr("Running").into())
                        } else {
                            (ui::Tone::Off, tr("Stopped").into())
                        },
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_server(dir.clone());
                        this.go(Page::Servers);
                        cx.notify();
                    })),
            );
        }
        let list = list.child(server_rows);

        let foot = div()
            .flex()
            .flex_col()
            .flex_none()
            .px(px(10.))
            .pt(px(16.))
            .pb(px(12.))
            .children(self.rail_update_card(soft, cx))
            .child(self.outline_side_links());

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(frame::SIDEBAR))
            .my(px(frame::MARGIN))
            .ml(px(frame::MARGIN))
            .overflow_hidden()
            .child(header.relative())
            .child(list.relative())
            .child(foot.relative())
    }
}
