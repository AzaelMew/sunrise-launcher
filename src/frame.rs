//! The glass look's frame (the mockup's `launcher-app.tsx`): the sidebar plate with the
//! instances, the launcher's update, Discord and the links; the top bar with the
//! pages in a glass tab strip, the player, Microsoft sign-in and Settings; and the menus they open.
//! Dark appearance only for now (`docs/design-system.md`, "Glass").
use super::*;
use crate::glass_ui::{self as ui, white};

use gpui::{anchored, canvas, deferred, point};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use sunrise_launcher::instances::{ Instance, Kind as InstanceKind};
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// The Outline look's sidebar, 14 from the window's edges.
pub const SIDEBAR: f32 = 248.;
/// The ⋮ menu of a sidebar row while open: whose, and whether Delete… was pressed once.
#[derive(Clone, Default)]
pub struct RowMenu {
    pub open: Option<(String, bool)>,
    /// Where each row was drawn last, by instance id: the menu opens beside its row.
    pub rows: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
}

impl Launcher {

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
