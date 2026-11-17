//! The Outline look's top bar (`docs/design-system.md`, "Outline"): the mark and "Sunrise" on the
//! left, the pages centred as text with a line under the open one, then how many play, the player's
//! face and Settings on the right.
use super::*;
use crate::glass_home_tabs::COMPACT;
use crate::glass_ui::{self as ui, mono, white};
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// A folded sidebar opening over the page while the pointer is near it, and closing again.
const UNFOLD: sunrise_launcher_theme::Motion = sunrise_launcher_theme::Motion {
    name: "unfold",
    ms: 220,
    curve: [0.2, 0.7, 0.2, 1.],
};
/// How near a folded sidebar the pointer opens it.
const NEAR: f32 = 12.;
/// A link icon's side at the sidebar's foot.
const LINK: f32 = 30.;

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
        let sides = self.outline_sides();
        let right_side = sides && self.outline_wide();

        let tight = self.window_width < 1280.;
        let (tab_size, tab_gap) = if tight { (14., 18.) } else { (15., 28.) };
        let in_bar: Vec<_> = Page::in_top_bar().collect();
        let tabs = in_bar
            .iter()
            .map(|(_, label, _)| {
                line_width(
                    window,
                    sunrise_launcher_theme::glass::FONT,
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
            let tab = crate::press(track_hover(div().id(id.clone()), id.clone()), &id, |d| {
                d.opacity(theme::PRESSED)
            });
            pages = pages.child(keyboard(
                tab.relative()
                    .flex()
                    .items_center()
                    .h(px(34.))
                    .child(underline)
                    .cursor_pointer()
                    .font_family(sunrise_launcher_theme::glass::FONT)
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

        let side = self.outline_side_width();
        // Without the sidebars the bar starts with the mark and the name.
        let name_width = line_width(
            window,
            sunrise_launcher_theme::glass::FONT,
            FontWeight::BOLD,
            17.,
            "Sunrise",
        );
        let logo_width = if sides { 0. } else { 22. + 10. + name_width };
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
        // The right side: the face or the sign-in, then the count, 14 px apart. Settings is in the
        // right sidebar's top cell while the window is narrow.
        let right_of = |count: bool, short: bool| {
            let account = if signed_in {
                61.
            } else if short {
                sign_in_width("Sign in")
            } else {
                sign_in_width("Sign in with Microsoft")
            };
            let count = count_width.filter(|_| count).map_or(0., |w| w + 14.);
            account + count
        };
        // Over the hero, on the page's centre, which is the window's.
        let end = side + self.outline_page_width();
        let centre = side + self.outline_page_width() / 2.;
        let fits = |right: f32| {
            centre - tabs / 2. >= side + 24. + logo_width + 16.
                && centre + tabs / 2. <= end - 24. - right - 16.
        };
        // Narrower, the players count goes first, then "with Microsoft": the pages keep their
        // place over the hero.
        let count = count_width.is_some() && fits(right_of(true, false));
        let short = !signed_in && !fits(right_of(false, false));
        // With the right sidebar open, the account is in its top cell and only Settings stays
        // here; folded, Settings is in its cell instead; without the sidebars, both are here.
        let right = if right_side {
            36.
        } else if sides {
            right_of(count, short)
        } else {
            right_of(count, short) + 14. + 36.
        };
        let centred = fits(right);
        let (pages_layer, pages_flow) = if centred {
            (Some(pages.flex_none()), None)
        } else {
            (None, Some(pages.flex_none()))
        };
        // Beside the buttons, the pages scroll in the strip with arrows at hidden ends.
        let performance = self.settings.performance();
        let flow = pages_flow.map(|p| self.nav_strip.render(p, &t, performance, window));
        let account = if right_side {
            self.outline_settings(cx).into_any_element()
        } else {
            self.outline_account(count, short, !sides, cx)
                .into_any_element()
        };
        let logo = (!sides).then(|| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .child(mark(22.))
                .child(ui::text(17., FontWeight::BOLD, ink::FULL).child("Sunrise"))
        });

        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(px(70.))
            .pt(px(14.))
            .px(px(24.))
            .gap(px(24.))
            .border_b_1()
            .border_color(white(0.08))
            .child(ui::grain(
                ui::color(t.night.side),
                ui::TEXTURE_QUIET,
                self.outline_page_width(),
                70.,
                self.scale,
            ))
            .children(logo)
            // Over the hero where the pages clear the buttons; else beside them, scrolling
            // sideways when too long.
            .children(pages_layer.map(|pages| {
                div()
                    .absolute()
                    .top(px(14.))
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(pages)
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .children(flow),
            )
            .child(account)
    }

    /// How many play now, in mono after a live dot; a click opens the stats page.
    pub(super) fn outline_playing(&self) -> Option<Stateful<Div>> {
        self.playing.map(|count| {
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
        })
    }

    /// The player's side of the top bar: how many play (when `count`), the player's face or
    /// Sign in (shortened when `short`), and Settings. The right sidebar's top cell holds it
    /// where there is one.
    pub(super) fn outline_account(
        &mut self,
        count: bool,
        short: bool,
        settings: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let playing = count.then(|| self.outline_playing()).flatten();

        let face = self.accounts.selected().map(|a| a.uuid.clone());
        let open = self.account_menu;
        let player = face.is_some().then(|| {
            let chip = hoverable("account", fill::HOVER)
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(36.))
                .px(px(6.))
                .rounded(px(ui::rad(0.)))
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
                .child(self.islands.account_menu.clone())
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
        let settings = settings.then(|| self.outline_settings(cx));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(14.))
            .children(playing)
            .children(player)
            .children(sign_in)
            .children(settings)
    }

    /// Settings, its sliders: a press opens it, or goes back Home from it.
    pub(super) fn outline_settings(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        ui::icon_button(
            "nav-Settings",
            "sliders-horizontal",
            36.,
            18.,
            radius::CONTROL,
            self.page == Page::Settings,
            &self.theme,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.go(if this.page == Page::Settings {
                Page::Play
            } else {
                Page::Settings
            });
            cx.notify();
        }))
    }
}

/// An Outline control: a `fill` of white that eases in under the pointer over the design's
/// quick curve, and a press that dims it, as the glass look's controls do.
pub(super) fn hoverable(id: impl Into<SharedString>, fill: f32) -> Stateful<Div> {
    let id = id.into();
    let el = track_hover(
        div()
            .id(id.clone())
            .relative()
            .child(hover_layer(&id, white(fill), 0., 0.)),
        id.clone(),
    );
    crate::press(el, &id, |d| d.opacity(theme::PRESSED))
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
    /// A sidebar (the `left` one, else the right): its place in the page's row, and on a narrow
    /// window the panel to draw after the page. At full width on a wide window; narrower, folded
    /// to its icon column, opening to full width over the page while the pointer is near it and
    /// folding again as it leaves. The page never moves for it.
    ///
    /// The panel is drawn after the page (over it, under the dialogs) and always the same
    /// element: lifted into GPUI's top layer only while it opened, it changed place every time
    /// it began to open, and the hover of everything in it (the instances' ⋮) was lost.
    pub(super) fn outline_side(
        &mut self,
        left: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Option<AnyElement>) {
        // Any of the way open, the sidebar draws its full self, cut to the width it has reached.
        let bar = |this: &mut Self, open: f32, cx: &mut Context<Self>| {
            if left {
                this.outline_sidebar(open > 0., open, cx)
            } else {
                this.outline_right_sidebar(open > 0., cx)
            }
        };
        if self.outline_wide() {
            return (bar(self, 1., cx).into_any_element(), None);
        }
        let side = usize::from(!left);
        let open = self.unfolded(side, window);
        let width = COMPACT + (frame::SIDEBAR - COMPACT) * open;
        let content = bar(self, open, cx);
        // The sidebar as wide as it has opened, its content held to the window's edge.
        let shown = div()
            .relative()
            .flex_none()
            .w(px(width))
            .h_full()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .when(left, |d| d.left_0())
                    .when(!left, |d| d.right_0())
                    .child(content),
            );
        // A strip of air on its page side, so the pointer opens it a little before it arrives.
        let near = div().flex_none().w(px(NEAR)).h_full();
        let panel = div()
            .id(if left { "left-fold" } else { "right-fold" })
            .absolute()
            .top_0()
            .bottom_0()
            .when(left, |d| d.left_0())
            .when(!left, |d| d.right_0())
            .flex()
            .when(!left, |d| d.flex_row_reverse())
            .child(shown)
            .child(near)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let was = this.side_folds[side].is_some_and(|(open, _)| open);
                if was != *hovered {
                    this.side_folds[side] = Some((*hovered, Instant::now()));
                    cx.notify();
                }
            }));
        // The pointer can leave the window without leaving the sidebar first (straight into
        // another window), and hover alone never hears of it: the window's exit folds it.
        let this = cx.entity().downgrade();
        let exit = gpui::canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let this = this.clone();
                window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Bubble {
                        return;
                    }
                    let _ = this.update(cx, |this, cx| {
                        let open = this.side_folds[side].is_some_and(|(open, _)| open);
                        if open {
                            this.side_folds[side] = Some((false, Instant::now()));
                            cx.notify();
                        }
                    });
                });
            },
        )
        .absolute()
        .size_0();
        let place = div()
            .relative()
            .flex_none()
            .w(px(COMPACT))
            .h_full()
            .child(exit)
            .into_any_element();
        (place, Some(panel.into_any_element()))
    }

    /// How far (0 folded, 1 open) the folded sidebar on `side` (0 left, 1 right) has opened,
    /// asking for frames while it moves.
    fn unfolded(&self, side: usize, window: &mut Window) -> f32 {
        let Some((open, at)) = self.side_folds[side] else {
            return 0.;
        };
        let t = if crate::motion::still() {
            1.
        } else {
            (at.elapsed().as_secs_f32() / UNFOLD.duration().as_secs_f32()).min(1.)
        };
        if t < 1. {
            window.request_animation_frame();
        }
        let t = UNFOLD.ease(t);
        if open {
            t
        } else {
            1. - t
        }
    }

    /// The Outline look's sidebar (Figma: Explorations → Sidebar): a flush column on the page's
    /// colour, the mark and "Sunrise" in its top cell, Instances, the instances with their ⋮ and
    /// New instance, then the launcher's update and the links with the version at its foot. A
    /// faint dashed sketch stands behind its lower half; the top bar's line and the hero's floor
    /// run through it. On a narrow window it folds, as the right one does, to an icon column: the
    /// mark, the instances' icons and New instance's plus, the links' icons.
    pub(super) fn outline_sidebar(
        &mut self,
        full: bool,
        unfold: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let soft = self.look.accent_soft;
        let width = if full { frame::SIDEBAR } else { COMPACT };
        self.load_packs();
        let logo = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(70.))
            .flex()
            .items_center()
            .gap(px(10.))
            .pt(px(14.))
            // Where the folded column centres it, so it stays put as the sidebar opens.
            .pl(px((COMPACT - 22.) / 2.))
            .child(mark(22.))
            .when(full, |d| {
                d.child(ui::text(17., FontWeight::BOLD, ink::FULL).child("Sunrise"))
            });
        // Folded, the label keeps its room unseen, so the rows don't jump as the sidebar opens.
        let label = div()
            .p(px(8.))
            // One line in either width: wrapped in the folded column it would stand taller.
            .whitespace_nowrap()
            .overflow_hidden()
            .when(!full, |d| d.invisible())
            .child(mono(12., ink::QUIET).child(tr("Instances").to_uppercase()));
        let new = hoverable("add-instance", fill::HOVER)
            .flex()
            .flex_none()
            .items_center()
            .h(px(36.))
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_instance_dialog(None);
                cx.notify();
            }));
        let new = if full {
            new.px(px(14.)).child(
                ui::text(14., FontWeight::MEDIUM, ink::BODY)
                    .relative()
                    .child(format!("+  {}", tr("New instance"))),
            )
        } else {
            new.justify_center()
                .child(ui::icon("plus-sign", 18., white(ink::BODY)).relative())
                .tooltip(ui::tip(tr("New instance")))
        };
        let new = keyboard(new, &t);
        // Open, the rows reach 6 px further out, so each icon stands where its folded one does.
        let list = div()
            .id("side-list")
            .when(full, |d| d.mx(px(-6.)))
            .flex()
            .flex_col()
            .gap(px(4.))
            .min_h_0()
            .overflow_y_scroll()
            .children(self.glass_instances(!full, cx))
            .child(new);
        let foot = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(12.))
            .children(full.then(|| self.rail_update_card(soft, cx)).flatten())
            .child(self.outline_side_links(if full { unfold } else { 0. }));
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(4.))
            .w(px(width))
            .h_full()
            .pt(px(82.))
            .px(px(if full { 12. } else { 10. }))
            .pb(px(32.))
            .border_r_1()
            .border_color(white(0.08))
            .child(ui::grain(
                ui::color(t.night.side),
                ui::TEXTURE_QUIET,
                width,
                self.window_height,
                self.scale,
            ))
            .when(full, |d| d.child(sketch()))
            .child(
                div()
                    .absolute()
                    .top(px(70.))
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(white(0.08)),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(24.))
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(white(0.10)),
            )
            .child(logo)
            .child(label)
            .child(list)
            .child(div().flex_1())
            .child(foot)
    }

    /// The sidebar's foot: Discord, Documentation and Website as icons, the launcher's version
    /// on the right; folded (not `full`), the icons alone, one above the other.
    fn outline_side_links(&self, unfold: f32) -> Div {
        let t = self.theme;
        let icon = |id: &'static str, icon: &'static str, url: String| {
            keyboard(
                hoverable(id, fill::BUTTON)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(LINK))
                    .cursor_pointer()
                    .child(ui::icon(icon, 18., white(ink::SOFT)).relative())
                    .on_click(move |_, _, cx| cx.open_url(&url)),
                &t,
            )
        };
        let links: Vec<_> = [
            ("discord", "discord", &self.links.discord),
            ("docs", "book", &self.links.docs),
            ("website", "globe-02", &self.links.website),
        ]
        .into_iter()
        .filter(|(_, _, url)| !url.is_empty())
        .collect();
        // The first stays where it is; each next one stands above it while folded and beside it
        // open, gliding between the two as the sidebar opens.
        let step = LINK + 2.;
        let more = links.len().saturating_sub(1) as f32;
        let mut stack = div()
            .relative()
            .flex_none()
            // Centred in the folded column, and in the same place open.
            .ml(px(
                (COMPACT - LINK) / 2. - if unfold > 0. { 12. } else { 10. }
            ))
            .w(px(LINK + step * more * unfold))
            .h(px(LINK + step * more * (1. - unfold)));
        for (i, (id, name, url)) in links.into_iter().enumerate() {
            stack = stack.child(
                div()
                    .absolute()
                    .left(px(step * i as f32 * unfold))
                    .bottom(px(step * i as f32 * (1. - unfold)))
                    .child(icon(id, name, url.clone())),
            );
        }
        if unfold <= 0. {
            return stack;
        }
        div()
            .flex()
            .items_end()
            .justify_between()
            .child(stack)
            .child(
                mono(12., ink::DIM)
                    .id("launcher-version")
                    .opacity(unfold)
                    .child(backend::VERSION),
            )
    }
}

/// The sidebar's faint dashed sketch: a stair of blocks and a turned one on a short floor,
/// standing on the line 76 px above the sidebar's foot.
fn sketch() -> Div {
    const S: f32 = 48.;
    let line = white(0.09);
    let square = |left: f32, up: usize| {
        div()
            .absolute()
            .left(px(left))
            .bottom(px(76. + up as f32 * S))
            .size(px(S))
            .border_1()
            .border_dashed()
            .border_color(line)
    };
    let mut d = div().absolute().top_0().bottom_0().left_0().right_0();
    for up in 0..4 {
        d = d.child(square(190., up));
    }
    for up in 0..2 {
        d = d.child(square(142., up));
    }
    // The turned block's outline, dashed as the others, by its four sides.
    d.child(
        gpui::canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let o = bounds.origin;
                let (cx, cy, r) = (
                    o.x + px(118.),
                    o.y + bounds.size.height - px(76. + 24.),
                    px(24.),
                );
                let corners = [
                    gpui::point(cx, cy - r),
                    gpui::point(cx + r, cy),
                    gpui::point(cx, cy + r),
                    gpui::point(cx - r, cy),
                ];
                // Dashes of 4 with gaps of 4 along each side.
                for i in 0..4 {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    let steps = 8;
                    for k in (0..steps).step_by(2) {
                        let p0 = a + (b - a) * (k as f32 / steps as f32);
                        let p1 = a + (b - a) * ((k + 1) as f32 / steps as f32);
                        let mut path = gpui::PathBuilder::stroke(px(1.));
                        path.move_to(p0);
                        path.line_to(p1);
                        if let Ok(path) = path.build() {
                            window.paint_path(path, line);
                        }
                    }
                }
            },
        )
        .absolute()
        .size_full(),
    )
    .child(
        div()
            .absolute()
            .left(px(24.))
            .bottom(px(76.))
            .w(px(214.))
            .h(px(1.))
            .bg(line),
    )
}
