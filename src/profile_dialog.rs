//! Xmilon's preview and creation form, scoped to Sunrise profiles. Saving uses the same
//! instance creation/copy transaction as the launcher; the dialog doesn't write game files.
use super::*;
use crate::{
    frame::{button, primary},
    instances_page::InstanceDialog,
    paint::palette,
    servers_page::Field,
};
use gpui::rgb;
use instances::{Channel, Icon};

impl Launcher {
    pub(super) fn new_profile(&mut self, cx: &mut Context<Self>) {
        self.servers.text(Field::InstanceName).set(String::new());
        self.servers.text(Field::InstanceGroup).set(String::new());
        self.servers.focus_soon = Some(Field::InstanceName);
        self.instances.dialog = Some(InstanceDialog {
            editing: None,
            copy: false,
            icon: None,
            picture: None,
            error: None,
            busy: false,
            pin: None,
            channel: Channel::Stable,
            discarding: false,
        });
        cx.notify();
    }

    pub(super) fn profile_dialog(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let d = self.instances.dialog.as_ref()?;
        let (busy, copy, icon, channel, pin, error) = (
            d.busy,
            d.copy,
            d.icon.clone(),
            d.channel,
            d.pin.clone(),
            d.error.clone(),
        );
        let p = palette();
        let name = self.servers.value(Field::InstanceName).trim().to_owned();
        let ready = !name.is_empty() && !busy;
        let current = pin
            .as_deref()
            .map(|tag| self.version_label(tag))
            .unwrap_or_else(|| {
                match channel {
                    Channel::Stable => tr("Latest stable"),
                    Channel::Nightly => tr("Latest nightly"),
                }
                .into()
            });
        let preview = div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .w(px(156.))
            .flex_none()
            .p(px(14.))
            .gap(px(12.))
            .rounded(px(tokens::R_CARD))
            .bg(p.card())
            .child(match icon.as_ref() {
                Some(Icon::Colour(c)) => div()
                    .size(px(64.))
                    .rounded(px(14.))
                    .bg(rgb(
                        u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(p.accent)
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(blocks::icon("package", 34., rgb(p.accent_on).into())),
                _ => crate::mark(64.),
            })
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(tokens::HEADING))
                    .font_weight(FontWeight::BOLD)
                    .child(if name.is_empty() {
                        tr("New profile").into()
                    } else {
                        name
                    }),
            )
            .child(
                div()
                    .text_size(px(tokens::SMALL))
                    .text_color(rgb(p.muted))
                    .child(current),
            );
        let row = || div().flex().items_center().gap(px(8.));
        let mut form = div()
            .flex()
            .flex_col()
            .min_w_0()
            .flex_1()
            .gap(px(10.))
            .child(tr("Name and group"))
            .child(
                row()
                    .child(
                        self.field(Field::InstanceName, tr("Profile name"), 190., cx)
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        self.field(Field::InstanceGroup, tr("No group"), 148., cx)
                            .flex_1()
                            .min_w_0(),
                    ),
            );
        let mut channels = row();
        for (id, c, label) in [
            ("new-stable", Channel::Stable, tr("Stable")),
            ("new-nightly", Channel::Nightly, tr("Nightly")),
        ] {
            channels = channels.child(
                button(id, label)
                    .when(c == channel, |d| {
                        d.bg(rgb(p.selected)).border_color(rgb(p.accent_ink))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = this.instances.dialog.as_mut().filter(|d| !d.busy) {
                            d.channel = c;
                            d.pin = None;
                        }
                        cx.notify();
                    })),
            );
        }
        let tags = match channel {
            Channel::Stable => self.stable_builds().into_iter().map(|b| b.tag).collect(),
            Channel::Nightly => self.sunrise_choices(),
        };
        let mut versions = div()
            .id("new-versions")
            .max_h(px(94.))
            .flex()
            .flex_col()
            .gap(px(3.))
            .overflow_y_scroll()
            .track_scroll(&crate::frame::scrolls("new-versions"));
        for (idx, tag) in std::iter::once(None)
            .chain(tags.into_iter().map(Some))
            .enumerate()
        {
            let label = tag
                .as_deref()
                .map(|t| self.version_label(t))
                .unwrap_or_else(|| tr("Follow latest").into());
            versions = versions.child(
                button(ElementId::named_usize("new-version", idx), label)
                    .justify_start()
                    .when(pin == tag, |d| {
                        d.bg(rgb(p.selected)).border_color(rgb(p.accent_ink))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = this.instances.dialog.as_mut().filter(|d| !d.busy) {
                            d.pin = tag.clone();
                        }
                        cx.notify();
                    })),
            );
        }
        form = form.child(channels).child(versions);
        let mut start = row();
        for (id, duplicate, label) in [
            ("new-empty", false, tr("Start empty")),
            ("new-copy", true, tr("Copy selected profile")),
        ] {
            start = start.child(
                button(id, label)
                    .when(copy == duplicate, |d| {
                        d.bg(rgb(p.selected)).border_color(rgb(p.accent_ink))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = this.instances.dialog.as_mut().filter(|d| !d.busy) {
                            d.copy = duplicate;
                        }
                        cx.notify();
                    })),
            );
        }
        let mut icons = row().child(
            button("new-mark", tr("Sunrise"))
                .when(icon.is_none(), |d| d.border_color(rgb(p.accent_ink)))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(d) = this.instances.dialog.as_mut().filter(|d| !d.busy) {
                        d.icon = None;
                    }
                    cx.notify();
                })),
        );
        for (idx, a) in sunrise_launcher::themes::Accent::ALL.iter().enumerate() {
            let colour = a.rgb();
            let choice = Icon::Colour(format!("#{colour:06x}"));
            icons = icons.child(
                button(ElementId::named_usize("new-colour", idx), "")
                    .px_0()
                    .w(px(22.))
                    .h(px(22.))
                    .bg(rgb(colour))
                    .when(icon.as_ref() == Some(&choice), |d| {
                        d.border_2().border_color(rgb(p.text))
                    })
                    .tooltip(crate::glass_ui::tip(a.name()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = this.instances.dialog.as_mut().filter(|d| !d.busy) {
                            d.icon = Some(choice.clone());
                        }
                        cx.notify();
                    })),
            );
        }
        let body = div()
            .flex()
            .flex_col()
            .child(crate::sheets::title(tr("New profile").into()))
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child(tr("Name it, pick what it plays, then an icon.")),
            )
            .child(
                row()
                    .items_stretch()
                    .child(preview)
                    .child(form.child(start).child(icons)),
            )
            .children(error.map(|e| div().text_color(rgb(p.danger)).child(e)))
            .child(
                row()
                    .justify_end()
                    .child(button("new-cancel", tr("Cancel")).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.close_instance_dialog();
                            cx.notify();
                        },
                    )))
                    .child(
                        primary(
                            "new-save",
                            if busy {
                                tr("Creating…")
                            } else {
                                tr("Create")
                            },
                        )
                        .when(!ready, |d| d.opacity(0.45).tab_stop(false))
                        .when(ready, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| {
                                this.save_instance(cx);
                                cx.notify();
                            }))
                        }),
                    ),
            );
        Some(crate::sheets::modal_frame(
            "new-profile",
            680.,
            body,
            cx,
            Launcher::close_instance_dialog,
        ))
    }
}
