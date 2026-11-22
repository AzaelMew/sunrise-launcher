//! The full-height profiles sidebar. Its middle scrolls independently of its controls.
use super::*;
use crate::{
    frame::{Page, Reach},
    paint::palette,
};
use gpui::{rgb, MouseButton, MouseDownEvent};
use instances::{Icon, Instance, Kind};
use std::{cell::Cell, rc::Rc};

/// How wide the sidebar is, open or closed.
pub(crate) fn rail_width(open: bool) -> f32 {
    if open {
        204.
    } else {
        54.
    }
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
        self.settings.sidebar_expanded = !self.settings.sidebar_expanded;
        self.save_settings();
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
                .when(on, |d| d.bg(rgb(p.selected)))
                .flex()
                .items_center()
                .when(wide, |d| d.px(px(8.)).gap(px(10.)))
                .when(!wide, |d| d.justify_center())
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
                .truncate()
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
            .when(wide, |d| d.px(px(4.)))
            .when(!wide, |d| d.justify_center())
            .child(
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
            );
        let add = row("sidebar-create".into(), tr("New profile").into(), false)
            .child(blocks::icon("plus", 20., rgb(p.chrome_text).into()))
            .when(wide, |d| d.child(txt(tr("New profile").into())))
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
            .child(self.profile_mark(&i, 28.))
            .when(wide, |d| d.child(txt(i.name.clone())))
            .when(wide && more, |d| d.pr(px(36.)))
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
            let actions = more.then(|| {
                let top = Rc::new(Cell::new(0.));
                let read_top = top.clone();
                let i = i.clone();
                div()
                    .id(SharedString::from(format!("profile-actions-{}", i.id)))
                    .reach()
                    .when(wide, |d| d.absolute().top(px(6.)).right(px(5.)).w(px(28.)))
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
                    .children(actions),
            );
        }
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(rail_width(wide)))
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
                .child(crate::glass_ui::icon(icon, 18., rgb(p.chrome_muted).into()))
                .when(wide, |d| d.child(txt(label.into())))
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
