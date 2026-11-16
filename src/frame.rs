//! The glass look's frame (the mockup's `launcher-app.tsx`): the sidebar plate with the
//! instances, the launcher's update, Discord and the links; the top bar with the
//! pages in a glass tab strip, the player, Microsoft sign-in and Settings; and the menus they open.
//! Dark appearance only for now (`docs/design-system.md`, "Glass").
use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use crate::instances_page::java_line;
use gpui::{anchored, canvas, deferred, point};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use sunrise_launcher::instances::{Icon, Instance, Kind as InstanceKind};
use sunrise_theme::glass::{fill, ink, radius};

/// The sidebar's distance from the window's edges (`m-3.5`).
pub const MARGIN: f32 = 14.;

/// The Outline look's sidebar, 14 from the window's edges.
pub const SIDEBAR: f32 = 248.;
/// What drifting particles (Taiga's snow, Hypr Neon's streaks, leaves, petals) keep clear of in a
/// window of `w` by `h`: the title and top bars, the rail, and the text. Home's hero text sits in
/// the left column and its tabs below it, so the sky drifts through the top right between them;
/// every other page is text from the top bar down.
pub fn keep_clear(w: f32, h: f32, home: bool) -> rain::Keep {
    const TOP: f32 = 32. + MARGIN + theme::TOPBAR;
    const HERO: f32 = 400.;
    const HERO_WIDTH: f32 = 640.;
    let left = MARGIN + SIDEBAR;
    let body = if home { HERO } else { TOP };
    rain::Keep([
        [0., 0., w, TOP],
        [0., 0., left, h],
        [left, TOP, HERO_WIDTH.min(w - left), (HERO - TOP).max(0.)],
        [left, body, w - left, (h - body).max(0.)],
        // The hero's right column: its version and What's new buttons, the invite-code field.
        [(w - 360.).max(left), TOP, 360., (HERO - TOP).max(0.)],
    ])
}

/// The ⋮ menu of a sidebar row while open: whose, and whether Delete… was pressed once.
#[derive(Clone, Default)]
pub struct RowMenu {
    pub open: Option<(String, bool)>,
    /// Where each row was drawn last, by instance id: the menu opens beside its row.
    pub rows: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
}

/// Records where the element it sits in is drawn, under `id`.
fn measure(rows: &Rc<RefCell<HashMap<String, Bounds<Pixels>>>>, id: String) -> impl IntoElement {
    let rows = rows.clone();
    canvas(
        move |bounds, _, _| {
            rows.borrow_mut().insert(id, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl Launcher {
    /// An instance's icon (`InstanceIcon`), `size` square with no frame: Sunrise's mark tile for
    /// Sunrise's own instance; else its picture or the block chosen for it, round at 26 %; else
    /// its loader's or Minecraft's mark; else its initial on its colour (the accent when none was
    /// chosen), as `ColorTile` draws it.
    pub(super) fn glass_instance_icon(&mut self, i: &Instance, size: f32) -> AnyElement {
        if i.kind == InstanceKind::Default && i.icon.is_none() {
            return self.sunrise_mark(size);
        }
        if let Some(picture) = self.instance_picture(i, size) {
            return instance_icons::centred(picture, size, self.scale).into_any_element();
        }
        let r = px((size * 0.26).round());
        let mut colour = self.look.accent;
        match &i.icon {
            Some(Icon::Colour(hex)) => {
                colour = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(colour);
            }
            Some(Icon::Texture(name)) => {
                if let Some(image) = self.texture_image(name, size) {
                    return gpui::img(image)
                        .flex_none()
                        .size(px(size))
                        .rounded(r)
                        .into_any_element();
                }
            }
            // A Sunrise instance with no icon of its own: its initial on a colour of the
            // picker's, the same each time for the same instance, so several read apart on
            // the rail. Java and modded ones keep their game's or loader's mark.
            None if i.java.is_none() && i.kind != InstanceKind::Port => {
                let hash =
                    i.id.bytes()
                        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
                let colours = crate::glass_instance::COLOURS;
                colour = colours[hash as usize % colours.len()];
            }
            None => {
                if let Some(mark) = self.instance_mark(i, size) {
                    return instance_icons::centred(mark, size, self.scale).into_any_element();
                }
            }
        }
        // Bricolage at the optical size a browser would pick for the letter.
        let letter = (size * 0.46).round();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(size))
            .rounded(r)
            .bg(ui::color(colour))
            .font_family(if letter >= 40. {
                sunrise_theme::glass::DISPLAY_FONT
            } else {
                sunrise_theme::glass::HEADING_FONT
            })
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(letter))
            .line_height(px(letter))
            .text_color(gpui::white())
            .child(instances_page::initial(&i.name))
            .into_any_element()
    }

    /// A sidebar row's state line and its dot (the mockup's `statusOf`), from what the launcher
    /// knows of the instance.
    pub(super) fn row_state(&self, i: &Instance) -> (Tone, String) {
        let running =
            self.client.is_some() && self.selected_instance().is_some_and(|s| s.id == i.id);
        if running {
            return (Tone::Live, tr("Running").into());
        }
        let starting = self.starting_client.is_some()
            && self.selected_instance().is_some_and(|s| s.id == i.id);
        if starting {
            return (Tone::Busy, tr("Starting").into());
        }
        match i.kind {
            InstanceKind::Default | InstanceKind::Own => {
                let tag = self.version_of(i).cloned();
                let prepared = tag.as_deref().is_some_and(backend::is_prepared);
                match (&i.java, &tag, &i.version) {
                    (Some(java), _, _) => (Tone::Live, java_line(java)),
                    (None, None, Some(own)) => (
                        Tone::Warn,
                        trf("{version} is not installed", &[("version", own)]),
                    ),
                    (None, None, None) => (Tone::Off, tr("No version installed").into()),
                    // Following the launcher's version, which has a newer one waiting.
                    (None, Some(_), None) if self.updates.sunrise.is_some() => (
                        Tone::Accent(self.look.accent_soft),
                        tr("Update available").into(),
                    ),
                    (None, Some(_), _) if !prepared => (Tone::Warn, tr("Needs preparing").into()),
                    (None, Some(tag), _) if self.native_build_of(tag).is_some() => {
                        let build = self.native_build_of(tag).unwrap_or_default();
                        (
                            Tone::Live,
                            trf(
                                "Native build · {version}",
                                &[("version", &build.sunrise_short())],
                            ),
                        )
                    }
                    (None, Some(tag), Some(_)) => (Tone::Live, format!("{} · {tag}", tr("Ready"))),
                    (None, Some(_), None) => (Tone::Live, tr("Ready").into()),
                }
            }
            InstanceKind::Port => {
                let slug = &i.id["port:".len()..];
                let summary = self.packs.iter().find(|p| p.slug == slug);
                (
                    Tone::Live,
                    summary.map_or_else(|| tr("Modpack").into(), |p| p.summary()),
                )
            }
            InstanceKind::Pack => (Tone::Off, tr("From Sunrise's registry").into()),
        }
    }

    /// The instances by group (`sections` in the mockup): those without a group first.
    /// The sidebar's instances, by group. `compact` (the folded sidebar): each its icon alone, the
    /// name in a tooltip, no group names and no ⋮ (a right click still opens the menu).
    pub(super) fn glass_instances(
        &mut self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = self.theme;
        let selected = self.selected_instance().map(|i| i.id.clone());
        let on_packs = self.page == Page::Mods && self.mods.on_modpacks();
        let mut out = Vec::new();
        let mut group = None;
        let mut list = div().flex().flex_col().gap(px(4.));
        let flush = |list: Div, out: &mut Vec<AnyElement>| {
            out.push(list.into_any_element());
            div().flex().flex_col().gap(px(4.))
        };
        for (order, i) in self.instances.list.clone().into_iter().enumerate() {
            if i.group != group {
                list = flush(list, &mut out);
                group = i.group.clone();
                if let Some(g) = group.as_ref().filter(|_| !compact) {
                    out.push(
                        ui::text(12., FontWeight::MEDIUM, ink::DIM)
                            .px(px(8.))
                            .pt(px(16.))
                            .pb(px(6.))
                            .whitespace_nowrap()
                            .child(g.clone())
                            .into_any_element(),
                    );
                }
            }
            // As in the mockup, the chosen instance stays marked on every page, unless a ported
            // pack's page is open: that row is the one shown then.
            let room_key = SharedString::from(format!("rail-name-{}", i.id));
            let cut = ui::cut(
                cx,
                &self.rooms,
                &room_key,
                &i.name,
                sunrise_theme::glass::FONT,
                14.,
                FontWeight::SEMIBOLD,
            );
            let current = match i.kind {
                InstanceKind::Default | InstanceKind::Own => {
                    !(on_packs && self.pack_open.is_some()) && selected.as_ref() == Some(&i.id)
                }
                InstanceKind::Port => {
                    on_packs && self.pack_open.as_deref() == Some(&i.id["port:".len()..])
                }
                InstanceKind::Pack => false,
            };
            let state = self.row_state(&i);
            let icon = self.glass_instance_icon(&i, 36.);
            let clicked = i.clone();
            let menu_open = self
                .row_menu
                .open
                .as_ref()
                .is_some_and(|(id, _)| *id == i.id);
            let menu_id = i.id.clone();
            // A right click on the row opens the same menu the ⋮ does.
            let right_id = i.id.clone();
            let more = div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(6.))
                .flex()
                .items_center()
                .child(
                    ui::icon_button(
                        format!("side-more-{}", i.id),
                        "more-vertical",
                        28.,
                        16.,
                        radius::SMALL,
                        menu_open,
                        &t,
                    )
                    .on_click(cx.listener(
                        move |this, event: &ClickEvent, _, cx| {
                            // The row under the ⋮ would otherwise take the click and go to Play.
                            cx.stop_propagation();
                            if this.closed_by_this_press(event) {
                                return;
                            }
                            this.row_menu.open =
                                row_menu_after_press(this.row_menu.open.take(), &menu_id);
                            cx.notify();
                        },
                    )),
                );
            // The ⋮ stays out of the way until its row is under the pointer: it fades and slides
            // in then. Shown outright while the pointer is on it (the row goes dark under it),
            // its menu is open, or the keyboard is moving through the rows.
            let row_id = SharedString::from(format!("side-instance-{}", i.id));
            let more_id = SharedString::from(format!("side-more-{}", i.id));
            let shown = menu_open
                || crate::hover_is(&more_id)
                || KEYBOARD.load(std::sync::atomic::Ordering::Relaxed);
            let more = if shown {
                more.into_any_element()
            } else {
                crate::hover_ease(
                    &row_id,
                    move || more,
                    |d, t| d.opacity(t).right(px(2. + 4. * t)),
                    sunrise_theme::QUICK,
                )
            };
            list = list.child(crate::motion::enter(
                ElementId::named_usize(format!("side-enter-{}", i.id), 0),
                order,
                div()
                    .relative()
                    .child(measure(&self.row_menu.rows, i.id.clone()))
                    .child(
                        ui::row(format!("side-instance-{}", i.id), current, 40., &t)
                            .when(compact, |d| d.justify_center().px_0())
                            .child(div().relative().flex_none().child(icon))
                            .when(!compact, |d| {
                                d.child(
                                    ui::row_text(i.name.clone(), state)
                                        .overflow_hidden()
                                        .child(ui::room(&self.rooms, room_key.clone())),
                                )
                            })
                            // A name cut short, or not shown at all: the full name on hover.
                            .when(cut || compact, |d| d.tooltip(ui::tip(i.name.clone())))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                match clicked.kind {
                                    InstanceKind::Default | InstanceKind::Own => {
                                        this.select_instance(&clicked);
                                        this.go(Page::Play);
                                    }
                                    InstanceKind::Port => {
                                        this.show_modpack(Some(clicked.id["port:".len()..].into()))
                                    }
                                    InstanceKind::Pack => this.show_modpack(None),
                                }
                                cx.notify();
                            }))
                            // The ⋮ menu, from a right click anywhere on the row.
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    // The open menu's mouse-down-out already closed it on this
                                    // press: reopening here would make the toggle a no-op.
                                    if this.menu_closed_at.take() == Some(event.position) {
                                        cx.notify();
                                        return;
                                    }
                                    this.row_menu.open =
                                        row_menu_after_press(this.row_menu.open.take(), &right_id);
                                    cx.notify();
                                }),
                            ),
                    )
                    .when(!compact, |d| d.child(more)),
            ));
        }
        out.push(list.into_any_element());
        out
    }

    /// The launcher's own update, as a card at the open rail's foot (while one downloads, waits,
    /// or could not be applied).
    pub(super) fn rail_update_card(&mut self, soft: u32, cx: &mut Context<Self>) -> Option<Div> {
        let t = self.theme;
        let card = |title: String, right: Option<String>| {
            div()
                .flex()
                .flex_col()
                .rounded(px(crate::glass_ui::rad(radius::ROW)))
                .bg(white(fill::FIELD))
                .border_1()
                .border_color(white(ui::edge::REST))
                .p(px(12.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .child(
                            ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL)
                                .flex_1()
                                .min_w_0()
                                .child(title),
                        )
                        .children(
                            right.map(|r| ui::text(12.5, FontWeight::NORMAL, ink::NOTE).child(r)),
                        ),
                )
        };
        match &self.self_update {
            SelfUpdate::Downloading(version, f) => Some(
                card(
                    trf("Launcher {version}", &[("version", version)]),
                    Some(format!("{:.0}%", f * 100.)),
                )
                .child(div().mt(px(10.)).child(ui::bar(*f, soft))),
            ),
            SelfUpdate::Ready(version) => Some(
                card(tr("A new version is available").to_string(), None)
                    .child(
                        ui::text(12.5, FontWeight::NORMAL, ink::NOTE)
                            .mt(px(4.))
                            .child(trf("Launcher {version}", &[("version", version)])),
                    )
                    .child(
                        ui::solid("update-launcher", tr("Update"), 32., 13., &t)
                            .mt(px(10.))
                            .w_full()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.apply_launcher_update(cx);
                                cx.notify();
                            })),
                    ),
            ),
            SelfUpdate::Stuck(error) if !self.self_update_hidden => {
                let why = self.launcher_update_help(error.as_deref());
                let url = backend::download_page(&self.links.website);
                Some(
                    card(tr("A new version is available").to_string(), None)
                        .child(
                            ui::text(12.5, FontWeight::NORMAL, ink::NOTE)
                                .mt(px(6.))
                                .child(why),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .mt(px(10.))
                                .when(self.launcher_download_available(), |actions| {
                                    actions.child(
                                        ui::solid(
                                            "download-launcher",
                                            tr("Download again"),
                                            32.,
                                            13.,
                                            &t,
                                        )
                                        .w_full()
                                        .on_click(move |_, _, cx| cx.open_url(&url)),
                                    )
                                })
                                .child(
                                    ui::solid(
                                        "dismiss-launcher-update",
                                        tr("Dismiss"),
                                        32.,
                                        13.,
                                        &t,
                                    )
                                    .w_full()
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.self_update_hidden = true;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        ),
                )
            }
            _ => None,
        }
    }

    /// The menu under the player (`Switch player`): everyone who can play, then Manage accounts.
    pub(super) fn glass_account_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.account_menu {
            return None;
        }
        let t = self.theme;
        let soft = self.look.accent_soft;
        let now = self.accounts.selected().map(|a| a.uuid.clone());
        let mut menu = ui::popover(250.)
            .id("account-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.account_menu = false;
                this.menu_closed_at = Some(event.position);
                cx.notify();
            }));
        for (i, (who, name, how)) in self.players().into_iter().enumerate() {
            let chosen = Some(&who) == now.as_ref();
            let id = SharedString::from(format!("account-pick-{i}"));
            menu = menu.child(keyboard(
                track_hover(
                    div()
                        .id(id.clone())
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .h(px(48.))
                        .px(px(10.))
                        .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
                        .cursor_pointer()
                        .child(hover_layer(&id, white(0.07), radius::CONTROL, 0.))
                        .child(
                            div()
                                .relative()
                                .size(px(30.))
                                .rounded(px(crate::glass_ui::rad(8.)))
                                .overflow_hidden()
                                .child(self.profile_heads.draw(
                                    Some(who.as_str()),
                                    30.,
                                    self.scale,
                                    &t,
                                )),
                        )
                        .child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    ui::text(14., FontWeight::MEDIUM, ink::FULL)
                                        .truncate()
                                        .child(name),
                                )
                                .child(ui::text(12., FontWeight::NORMAL, ink::FAINT).child(how)),
                        )
                        .when(chosen, |d| {
                            d.child(ui::icon("tick-02", 16., ui::color(soft)).relative())
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.play_as(&who);
                            cx.notify();
                        })),
                    id,
                ),
                &t,
            ));
        }
        menu = menu.child(div().mt(px(4.)).child(
            ui::menu_row("account-manage", "user", tr("Manage accounts"), false, &t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.account_menu = false;
                    this.go(Page::Settings);
                    cx.notify();
                }),
            ),
        ));
        Some(deferred(
            div().absolute().top(px(52.)).right_0().child(menu),
        ))
    }

    /// The ⋮ menu of a sidebar row, beside it: Edit…, Duplicate…, Open folder, Export…,
    /// launch settings, the desktop shortcut, then Delete… (asked twice). Each acts on that instance.
    pub(super) fn row_menu(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, confirm) = self.row_menu.open.clone()?;
        let Some(instance) = self.instances.list.iter().find(|i| i.id == id).cloned() else {
            self.row_menu.open = None;
            return None;
        };
        let row = self.row_menu.rows.borrow().get(&id).copied()?;
        let t = self.theme;
        let viewport = window.viewport_size();
        let height = f32::from(viewport.height);
        // The mockup's `rect.right + 12`, `min(rect.top - 8, innerHeight - 330)`: its ⋮ button
        // ends 6 px inside the row and sits in the row's middle.
        let x = f32::from(row.origin.x + row.size.width) - 6. + 12.;
        let button_top = f32::from(row.origin.y) + (f32::from(row.size.height) - 32.) / 2.;
        let y = (button_top - 8.).min(height - 330.).max(8.);
        let item = |key: &'static str, icon: &'static str, label: String| {
            ui::menu_row(format!("row-menu-{key}"), icon, label, false, &t)
        };
        let pick = |this: &mut Launcher, i: &Instance| {
            this.row_menu.open = None;
            this.select_instance(i);
            this.go(Page::Play);
        };
        let (edit, copy, export, launch, shortcut) = (
            instance.clone(),
            instance.clone(),
            instance.clone(),
            instance.clone(),
            instance.clone(),
        );
        let home = instance.home.clone();
        let labels = [
            tr("Edit…"),
            tr("Duplicate…"),
            tr("Open folder"),
            tr("Export…"),
            tr("Launch…"),
            if self.has_shortcut(&instance.id) {
                tr("Remove the desktop shortcut")
            } else {
                tr("Create a desktop shortcut")
            },
            if confirm {
                tr("Click again to delete")
            } else {
                tr("Delete…")
            },
        ];
        let [edit_label, copy_label, folder_label, export_label, launch_label, shortcut_label, delete_label] =
            labels;
        let viewport_width = f32::from(viewport.width);
        let width = ui::menu_content_width(window, &labels)
            .max(250.)
            .min((viewport_width - 16.).max(0.));
        let x = x.min(viewport_width - width - 8.).max(8.);
        let mut menu = ui::popover(width)
            .id("row-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.row_menu.open = None;
                this.menu_closed_at = Some(event.position);
                cx.notify();
            }))
            .child(
                item("edit", "pencil-edit-02", edit_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.row_menu.open = None;
                        this.open_instance_sheet(edit.clone(), glass_sheet::SheetTab::General);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("duplicate", "copy-01", copy_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        pick(this, &copy);
                        this.start_copy();
                        cx.notify();
                    },
                )),
            )
            .child(
                item("folder", "folder-01", folder_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.row_menu.open = None;
                        let _ = std::fs::create_dir_all(&home);
                        crate::glass_toast::open(cx, &home);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("export", "upload-04", export_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        pick(this, &export);
                        this.open_instance_sheet(export.clone(), glass_sheet::SheetTab::Storage);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("launch", "sliders-horizontal", launch_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        pick(this, &launch);
                        this.open_instance_sheet(launch.clone(), glass_sheet::SheetTab::Launch);
                        cx.notify();
                    },
                )),
            )
            .child(
                item("shortcut", "link-01", shortcut_label.into()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        this.row_menu.open = None;
                        this.toggle_shortcut(shortcut.id.clone(), shortcut.name.clone());
                        cx.notify();
                    },
                )),
            );
        // Sunrise's own instance can't be deleted, nor one while a game runs.
        let can_delete = instance.kind != InstanceKind::Default
            && self.client.is_none()
            && self.starting_client.is_none();
        let gone = instance.id.clone();
        let delete = ui::menu_row("row-menu-delete", "delete-02", delete_label, true, &t);
        let delete = if can_delete {
            delete.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some((_, confirm)) = this.row_menu.open.as_mut().filter(|(_, c)| !*c) {
                    *confirm = true;
                } else {
                    this.row_menu.open = None;
                    this.delete_instance(&gone);
                }
                cx.notify();
            }))
        } else {
            delete.opacity(0.4).cursor_default()
        };
        menu = menu.child(div().mt(px(4.)).child(delete));
        Some(
            deferred(anchored().position(point(px(x), px(y))).child(menu))
                .with_priority(1)
                .into_any_element(),
        )
    }
}
