//! The full-height profiles sidebar. Its middle scrolls independently of its controls.
use super::*;
use crate::{
    frame::{Page, Reach},
    paint::palette,
};
use gpui::{rgb, MouseButton, MouseDownEvent};
use instances::{Icon, Instance, Kind};
use std::{cell::Cell, rc::Rc};

/// How wide the sidebar is closed, and open.
const CLOSED: f32 = 54.;
const OPEN: f32 = 204.;
/// The room a row has inside the closed sidebar: its padding and both borders taken off.
const CLOSED_ROW: f32 = CLOSED - 2. * 5. - 2. - 2.;

/// How wide the sidebar is, `open` of the way from closed (0) to open (1).
pub(crate) fn rail_width(open: f32) -> f32 {
    CLOSED + (OPEN - CLOSED) * open
}

/// A label's opacity as the sidebar opens: it waits for room before showing.
fn label_opacity(open: f32) -> f32 {
    ((open - 0.3) / 0.7).clamp(0., 1.)
}

/// The column a sidebar icon is centred in, open or closed: a closed row's inside (the closed
/// rail less its 1 px edges, its 5 px padding and the row's own 1 px edges). Labels follow it,
/// so no icon moves when the sidebar opens or closes.
fn icon_slot(icon: impl IntoElement) -> Div {
    div()
        .flex_none()
        .w(px(CLOSED_ROW))
        .flex()
        .items_center()
        .justify_center()
        .child(icon)
}

/// Whether the selected profile's actions button shows beside its name, `open` of the way:
/// once it fades in, when it is already clear of the mark.
fn side_actions_shown(open: f32) -> bool {
    label_opacity(open) > 0.
}

/// The room under the mark the closed actions button took (28 px), `open` of the way.
fn actions_room(open: f32) -> f32 {
    28. * (1. - open).clamp(0., 1.)
}

fn profile_lock_reason(
    work: &crate::launch_state::Facts,
    located: bool,
    pending: bool,
) -> Option<&'static str> {
    if !located {
        Some(tr(
            "Fix the data folder in Settings before changing profiles.",
        ))
    } else if work.busy() {
        Some(tr(
            "Wait for the current operation to finish before changing profiles.",
        ))
    } else if pending {
        Some(tr(
            "Finish or cancel the pending launch before changing profiles.",
        ))
    } else {
        None
    }
}

impl Launcher {
    pub(super) fn toggle_sidebar(&mut self) {
        let open = !self.settings.sidebar_expanded;
        self.settings.sidebar_expanded = open;
        // From wherever the last toggle had got to: a second press turns it around there.
        let mut fade = crate::motion::Fade::toward(self.ui.rail, open, std::time::Instant::now());
        fade.motion = crate::motion::RAIL;
        self.ui.rail = Some(fade);
        self.save_settings();
    }

    /// The window's width less what the sidebar takes beyond its closed width, as it is drawn
    /// now: what the top bar's narrow layouts go by, so they change when the room does, part
    /// way through an ease, not on the press that starts it.
    pub(crate) fn bar_width(&self) -> f32 {
        self.window_width - (rail_width(self.rail_open()) - CLOSED)
    }

    /// How open the sidebar is drawn: 0 closed, 1 open, between while it eases. The frame's
    /// one sample (`frame::State::rail_drawn`): a part built later in the same frame never sees
    /// a width the rail wasn't drawn at.
    pub(crate) fn rail_open(&self) -> f32 {
        self.ui
            .rail_drawn
            .unwrap_or_else(|| self.rail_open_at(std::time::Instant::now()))
    }

    /// How open the sidebar is at `now`.
    pub(crate) fn rail_open_at(&self, now: std::time::Instant) -> f32 {
        let at_rest = if self.settings.sidebar_expanded {
            1.
        } else {
            0.
        };
        self.ui
            .rail
            .map_or(at_rest, |fade| fade.value(now).clamp(0., 1.))
    }

    fn profile_lock(&self) -> Option<&'static str> {
        profile_lock_reason(
            &self.launch_activity(),
            !self.location_unavailable,
            self.pending_consent.is_some()
                || self.ui.after_sign_in.is_some()
                || self.ui.after_install.is_some(),
        )
    }

    pub(super) fn switch_profile(&mut self, instance: &Instance, cx: &mut Context<Self>) {
        if self.profile_lock().is_some() {
            return;
        }
        if !self
            .selected_instance()
            .is_some_and(|i| i.id == instance.id)
        {
            self.select_instance(instance);
            self.clear_profile_notices();
        }
        self.navigate(Page::Home);
        cx.notify();
    }

    pub(super) fn clear_profile_notices(&mut self) {
        self.client_error = None;
        self.prepare_error = None;
        self.install_error = None;
        self.crash = None;
    }

    pub(super) fn profile_mark(&mut self, i: &Instance, size: f32) -> Div {
        let p = palette();
        let picture = self.icon_image(format!("picture:{}", i.id), size, || {
            Some(i.home.join(instances::PICTURE))
        });
        let texture = match &i.icon {
            Some(icon @ Icon::Texture(name)) if picture.is_none() => {
                self.version_of(i).cloned().and_then(|tag| {
                    self.icon_image(format!("texture:{tag}:{name}"), size, || {
                        icon.texture(&backend::resources(&tag))
                    })
                })
            }
            _ => None,
        };
        let tile = div()
            .size(px(size))
            .flex_none()
            .rounded(px(size * 0.22))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center();
        if let Some(image) = picture.or(texture) {
            return tile.child(
                gpui::img(image)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain),
            );
        }
        match &i.icon {
            Some(Icon::Colour(hex)) => {
                let colour =
                    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(p.selected);
                let ink = if sunrise_launcher_theme::contrast(colour, p.text)
                    > sunrise_launcher_theme::contrast(colour, p.background)
                {
                    p.text
                } else {
                    p.background
                };
                tile.bg(rgb(colour))
                    .text_color(rgb(ink))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(size * 0.54))
                    .child(
                        i.name
                            .chars()
                            .next()
                            .unwrap_or('S')
                            .to_uppercase()
                            .to_string(),
                    )
            }
            _ => crate::mark(size),
        }
    }

    pub(super) fn sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let wide = self.settings.sidebar_expanded;
        let open = self.rail_open();
        // Labels are drawn once there is any room for them; they fade in as it opens.
        let labelled = open > 0.;
        let p = palette();
        let lock = self.profile_lock();
        let row = |id: ElementId, label: String, on: bool| {
            div()
                .id(id)
                .reach_edge()
                .h(px(40.))
                .flex_none()
                .w_full()
                .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                .border_1()
                .border_color(if on {
                    rgb(p.accent_ink).into()
                } else {
                    gpui::transparent_black()
                })
                // The chrome's own lift, in the theme's hue, under the accent's edge: the
                // accent washed into the fill stood out in themes whose hue differs from it.
                .when(on, |d| d.bg(rgb(p.chrome_raised)))
                .flex()
                .items_center()
                // Icons stay in their column (`icon_slot`); the room after the label eases in.
                .gap(px(4.))
                .pr(px(8. * open))
                .when(lock.is_none(), |d| {
                    d.cursor_pointer().hover(|d| d.bg(rgb(p.chrome_raised)))
                })
                .when(lock.is_some(), |d| d.opacity(0.45).tab_stop(false))
                .tooltip(crate::glass_ui::tip(lock.map_or(label, str::to_owned)))
        };
        let txt = |s: String| {
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                // An ellipsis would flicker along a label still being uncovered.
                .when(open >= 1., |d| d.text_ellipsis())
                .opacity(label_opacity(open))
                .text_size(px(tokens::LABEL))
                .child(s)
        };
        // A small toolbar button, not a row: it names itself in its tooltip only. Ctrl+B does
        // the same, and the search lists that key.
        let toggle = div()
            .flex()
            .flex_none()
            .h(px(32.))
            .items_center()
            // In line with the rows' icons, whose rows have a 1 px edge.
            .px(px(1.))
            .child(icon_slot(
                div()
                    .id("sidebar-toggle")
                    .reach()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(crate::tokens::r(crate::tokens::R_SMALL))
                    .cursor_pointer()
                    .hover(|d| d.bg(rgb(p.chrome_raised)))
                    .tooltip(crate::glass_ui::tip(if wide {
                        tr("Hide sidebar")
                    } else {
                        tr("Show sidebar")
                    }))
                    .child(
                        div()
                            .w(px(16.))
                            .h(px(13.))
                            .flex()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(rgb(p.chrome_muted))
                            .child(div().ml(px(4.)).w(px(1.)).h_full().bg(rgb(p.chrome_muted))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_sidebar();
                        cx.notify();
                    })),
            ));
        let add = row("sidebar-create".into(), tr("New profile").into(), false)
            .child(icon_slot(blocks::icon(
                "plus",
                20.,
                rgb(p.chrome_text).into(),
            )))
            .when(labelled, |d| d.child(txt(tr("New profile").into())))
            .when(lock.is_none(), |d| {
                d.on_click(cx.listener(|this, _, _, cx| {
                    if this.profile_lock().is_none() {
                        this.new_profile(cx);
                    }
                }))
            });
        let selected = self.selected_instance().map(|i| i.id.clone());
        let mut list = div()
            .id("sidebar-profiles")
            .flex()
            .flex_col()
            .min_h_0()
            .flex_1()
            .gap(px(4.))
            .overflow_y_scroll()
            .track_scroll(&crate::frame::scrolls("sidebar"));
        for (n, i) in self.instances.list.clone().into_iter().enumerate() {
            let on = selected.as_ref() == Some(&i.id);
            let deletable = i.kind == Kind::Own && !self.location_unavailable;
            let more = on && deletable;
            let profile = row(
                ElementId::named_usize("sidebar-profile", n),
                i.name.clone(),
                on,
            )
            .child(icon_slot(self.profile_mark(&i, 28.)))
            .when(labelled, |d| d.child(txt(i.name.clone())))
            .when(labelled && more, |d| d.pr(px(36.)))
            .when(deletable, |d| {
                let i = i.clone();
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.open_profile_menu(&i, f32::from(ev.position.y));
                        cx.notify();
                    }),
                )
            });
            // Opening, the button leaves the row for its place beside the name, which it reaches
            // only once there is room: until it shows, it sits over the mark, so it isn't built
            // at all, or a press on the mark could open the profile's menu.
            let side = labelled && side_actions_shown(open);
            let actions = (more && (!labelled || side)).then(|| {
                let top = Rc::new(Cell::new(0.));
                let read_top = top.clone();
                let i = i.clone();
                div()
                    .id(SharedString::from(format!("profile-actions-{}", i.id)))
                    .reach()
                    // Beside the name once the sidebar opens, fading in with it; under the
                    // mark when closed.
                    .when(labelled, |d| {
                        d.absolute()
                            .top(px(6.))
                            .right(px(5.))
                            .w(px(28.))
                            .opacity(label_opacity(open))
                    })
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|d| d.bg(rgb(p.chrome_raised)))
                    .tooltip(crate::glass_ui::tip(tr("Profile actions")))
                    .child(div().flex().gap(px(2.)).children(
                        (0..3).map(|_| div().size(px(3.)).rounded_full().bg(rgb(p.chrome_text))),
                    ))
                    .child(
                        gpui::canvas(
                            move |bounds, _, _| top.set(f32::from(bounds.top())),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_profile_menu(&i, read_top.get());
                        cx.notify();
                    }))
            });
            list = list.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .child(profile.when(lock.is_none(), |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| this.switch_profile(&i, cx)))
                    }))
                    .children(actions)
                    // The room the closed button took under the mark eases away with the
                    // width, so the profiles below don't jump as it leaves the row.
                    .when(more && labelled, |d| {
                        d.child(div().flex_none().h(px(actions_room(open))))
                    }),
            );
        }
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(rail_width(open)))
            .min_h_0()
            .flex_none()
            .gap(px(4.))
            .p(px(5.))
            .rounded(crate::tokens::r(crate::tokens::R_PANEL))
            .bg(p.chrome_plate())
            .border_1()
            .border_color(rgb(p.divider))
            .text_color(rgb(p.chrome_text))
            .child(toggle)
            .child(add)
            .child(list);
        for (id, label, icon, url) in [
            (
                "sidebar-discord",
                tr("Discord"),
                "discord",
                self.links.discord.clone(),
            ),
            (
                "sidebar-website",
                tr("Website"),
                "earth",
                self.links.website.clone(),
            ),
            (
                "sidebar-settings",
                tr("Settings"),
                "sliders-horizontal",
                String::new(),
            ),
        ] {
            let settings = id == "sidebar-settings";
            rail = rail.child(
                row(
                    id.into(),
                    label.into(),
                    settings && self.ui.page == Page::Settings,
                )
                .child(icon_slot(crate::glass_ui::icon(
                    icon,
                    18.,
                    rgb(p.chrome_muted).into(),
                )))
                .when(labelled, |d| d.child(txt(label.into())))
                .when(settings || !url.is_empty(), |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        if settings {
                            this.navigate(Page::Settings);
                        } else {
                            Self::open_url(&url, cx);
                        }
                        cx.notify();
                    }))
                }),
            );
        }
        rail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch_state::Facts;

    #[test]
    fn the_actions_button_takes_input_only_where_it_shows_clear_of_the_mark() {
        for step in 0..=1000 {
            let open = step as f32 / 1000.;
            // Inside the rail's padding (5) and border (1): the button sits 5 px from the
            // right and is 28 wide; the mark sits in its column after the row's border.
            let button_left = rail_width(open) - 2. * 5. - 2. - 5. - 28.;
            // The mark is centred in its column, which doesn't move as the sidebar opens.
            let mark_right = 1. + (CLOSED_ROW - 28.) / 2. + 28.;
            if open > 0. && side_actions_shown(open) {
                assert!(
                    button_left >= mark_right,
                    "at {open} the button ({button_left}) is over the mark ({mark_right})"
                );
            }
        }
        // Closed, the button is in the row under the mark; the room it took eases away.
        assert_eq!(actions_room(0.), 28.);
        assert_eq!(actions_room(1.), 0.);
        assert!(
            (actions_room(0.001) - 28.).abs() < 0.1,
            "no jump as opening begins"
        );
    }

    #[test]
    fn icons_keep_the_closed_rows_column_and_labels_wait_for_room() {
        // The icon column is a closed row's inside: the rail less its edges, padding and the
        // row's own edges, as `icon_slot` was sized from before the sidebar eased.
        assert_eq!(CLOSED_ROW, rail_width(0.) - 2. * (1. + 5. + 1.));
        assert_eq!(rail_width(0.), 54.);
        assert_eq!(rail_width(1.), 204.);
        assert_eq!(label_opacity(0.2), 0.);
        assert_eq!(label_opacity(1.), 1.);
    }

    #[test]
    fn profiles_unlock_when_starting_finishes_even_while_the_game_runs() {
        let mut work = Facts {
            starting: true,
            ..Facts::default()
        };
        assert!(profile_lock_reason(&work, true, false).is_some());
        work.running = true;
        work.starting = false;
        assert_eq!(profile_lock_reason(&work, true, false), None);
        assert!(profile_lock_reason(&work, false, false).is_some());
        assert!(profile_lock_reason(&work, true, true).is_some());
    }
}
