//! The full-height profiles sidebar. Its middle scrolls independently of its controls.
use super::*;
use crate::{
    frame::{Page, Reach},
    paint::palette,
};
use gpui::rgb;
use instances::{Icon, Instance};

impl Launcher {
    pub(super) fn toggle_sidebar(&mut self) {
        self.settings.sidebar_expanded = !self.settings.sidebar_expanded;
        self.save_settings();
    }

    fn profile_locked(&self) -> bool {
        self.launch_busy()
            || self.client.is_some()
            || self.pending_consent.is_some()
            || self.ui.after_sign_in.is_some()
            || self.ui.after_install.is_some()
            || self.location_error.is_some()
    }

    pub(super) fn switch_profile(&mut self, instance: &Instance, cx: &mut Context<Self>) {
        if self.profile_locked() {
            return;
        }
        if !self
            .selected_instance()
            .is_some_and(|i| i.id == instance.id)
        {
            self.select_instance(instance);
            self.quick_play = None;
            self.joining = None;
            self.client_error = None;
            self.prepare_error = None;
            self.install_error = None;
            self.crash = None;
        }
        self.navigate(Page::Home);
        cx.notify();
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
                .cursor_pointer()
                .hover(|d| d.bg(rgb(p.chrome_raised)))
                .tooltip(crate::glass_ui::tip(label))
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
        let locked = self.profile_locked();
        let add = row("sidebar-create".into(), tr("New profile").into(), false)
            .child(blocks::icon("plus", 20., rgb(p.chrome_text).into()))
            .when(wide, |d| d.child(txt(tr("New profile").into())))
            .when(locked, |d| d.opacity(0.45).tab_stop(false))
            .when(!locked, |d| {
                d.on_click(cx.listener(|this, _, _, cx| {
                    if !this.profile_locked() {
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
            list = list.child(
                row(
                    ElementId::named_usize("sidebar-profile", n),
                    i.name.clone(),
                    on,
                )
                .child(self.profile_mark(&i, 28.))
                .when(wide, |d| d.child(txt(i.name.clone())))
                .when(!locked, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| this.switch_profile(&i, cx)))
                })
                .when(locked, |d| d.tab_stop(false).cursor_default()),
            );
        }
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(if wide { 204. } else { 54. }))
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
