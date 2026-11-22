//! One confirmation for profile removal, whether opened in Settings or in the sidebar.
use super::*;
use crate::{
    frame::{button, Reach as _},
    mods::refused::{details_box, details_label},
    paint::palette,
    settings::danger,
    sheets::{frame, title, Sheet},
};
use gpui::{rgb, MouseButton};
use instances::{DeleteTarget, Instance};

pub(super) enum Action {
    Menu {
        target: Instance,
        top: f32,
    },
    Confirm(Instance),
    Deleting(String),
    Problem {
        heading: String,
        message: Option<&'static str>,
        error: String,
        details: bool,
    },
}

impl Launcher {
    pub(super) fn open_profile_menu(&mut self, target: &Instance, top: f32) {
        self.open_sheet(Sheet::ProfileActions);
        self.ui.sheets.profile_action = Some(Action::Menu {
            target: target.clone(),
            top,
        });
    }

    pub(super) fn confirm_profile_delete(&mut self, target: &Instance) {
        self.open_sheet(Sheet::ProfileActions);
        self.ui.sheets.profile_action = Some(Action::Confirm(target.clone()));
    }

    fn delete_profile(&mut self, cx: &mut Context<Self>) {
        let Some(Action::Confirm(target)) = &self.ui.sheets.profile_action else {
            return;
        };
        let target = target.clone();
        self.ui.sheets.profile_action = Some(Action::Deleting(target.name.clone()));
        let task = cx.background_executor().spawn(async move {
            let result = instances::delete_with_selection(DeleteTarget::Confirmed(&target));
            (target, result)
        });
        cx.spawn(async move |this, cx| {
            let (target, result) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.refresh_instances_list();
                // Even a Trash failure can detach the profile into the recovery directory.
                // Reconcile only this selection, without writing settings a second time.
                if this.settings.instance.as_ref() == Some(&target.id)
                    && (result.is_ok() || !this.instances.list.iter().any(|i| i.id == target.id))
                {
                    this.settings.instance = None;
                    this.clear_profile_notices();
                }
                let trashed = |name: &str| trf("{name} is in the Trash.", &[("name", name)]);
                this.ui.sheets.profile_action = match result {
                    Ok((gone, Ok(()))) => {
                        crate::glass_toast::say(trashed(&gone.name));
                        None
                    }
                    Ok((gone, Err(error))) => Some(Action::Problem {
                        heading: trashed(&gone.name),
                        message: Some(tr("The new profile selection could not be saved.")),
                        error,
                        details: false,
                    }),
                    Err(error) => Some(Action::Problem {
                        heading: trf(
                            "Couldn't move {name} to the Trash",
                            &[("name", &target.name)],
                        ),
                        message: None,
                        error,
                        details: false,
                    }),
                };
                if this.ui.sheets.profile_action.is_some() {
                    this.open_sheet(Sheet::ProfileActions);
                } else if this.ui.sheet == Some(Sheet::ProfileActions) {
                    this.close_sheet();
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn profile_action_sheet(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let action = self.ui.sheets.profile_action.as_ref()?;
        let body = div().flex().flex_col();
        let footer = || div().flex().justify_end().gap(px(8.));
        let close = |id, label, cx: &mut Context<Self>| {
            button(id, label).on_click(cx.listener(|this, _, _, cx| {
                this.close_sheet();
                cx.notify();
            }))
        };
        let body = match action {
            Action::Menu { target, top } => return Some(self.profile_menu(target, *top, cx)),
            Action::Confirm(target) => body
                .child(title(trf("Delete {name}?", &[("name", &target.name)])))
                .child(tr(
                    "This profile and its worlds and mods will move to the Trash.",
                ))
                .child(
                    footer()
                        .child(close("profile-delete-cancel", tr("Cancel"), cx))
                        .child(
                            danger(button("profile-delete-confirm", tr("Delete profile")))
                                .on_click(cx.listener(|this, _, _, cx| this.delete_profile(cx))),
                        ),
                ),
            Action::Deleting(name) => {
                body.child(title(trf("Moving {name} to the Trash…", &[("name", name)])))
            }
            Action::Problem {
                heading,
                message,
                error,
                details,
            } => body
                .child(title(heading.clone()))
                .children(*message)
                .children(details.then(|| details_box("profile-delete-error", error.clone())))
                .child(
                    footer()
                        .child(
                            button("profile-delete-details", details_label(*details)).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(Action::Problem { details, .. }) =
                                        &mut this.ui.sheets.profile_action
                                    {
                                        *details = !*details;
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(close("profile-delete-close", tr("Close"), cx)),
                ),
        };
        Some(frame("profile-delete-sheet", 450., body, cx))
    }

    fn profile_menu(&self, target: &Instance, top: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let p = palette();
        let at = tokens::FRAME_INSET + crate::rail::rail_width(self.settings.sidebar_expanded) + 8.;
        let name = target.name.clone();
        let target = target.clone();
        let item = div()
            .id("profile-menu-delete")
            .reach()
            .flex()
            .items_center()
            .h(px(tokens::FIELD))
            .px(px(8.))
            .cursor_pointer()
            .text_color(rgb(p.danger))
            .hover(|s| s.bg(rgb(p.hover)))
            .child(tr("Delete profile…"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.confirm_profile_delete(&target);
                cx.notify();
            }));
        let action_height = tokens::FIELD + 44.; // 26 for the title, 16 padding and 2 border.
        div()
            .id("profile-menu")
            .absolute()
            .inset_0()
            .occlude()
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_sheet();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(at))
                    .top(px(top.min(self.window_height - action_height - 8.)))
                    .w(px(236.))
                    .p(px(8.))
                    .rounded(tokens::r(tokens::R_CARD))
                    .border_1()
                    .border_color(rgb(p.line))
                    .bg(rgb(p.raised))
                    .text_color(rgb(p.text))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .h(px(26.))
                            .px(px(8.))
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(tokens::SMALL))
                            .text_color(rgb(p.muted))
                            .child(name),
                    )
                    .child(item),
            )
    }
}
