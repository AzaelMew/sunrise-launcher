//! One confirmation for profile removal, whether opened in Settings or in the sidebar.
use super::*;
use crate::{
    frame::{button, Reach as _},
    mods::refused::{details_box, details_label},
    paint::palette,
    settings::danger,
    sheets::{modal_frame, title},
};
use gpui::{rgb, Global, MouseButton};
use instances::{DeleteError, Instance};

/// A native close or Quit must not abandon a folder in the middle of the move.
#[derive(Default)]
struct MoveInFlight {
    quit_requested: bool,
}

impl Global for MoveInFlight {}

pub(super) fn moving(cx: &App) -> bool {
    cx.has_global::<MoveInFlight>()
}

pub(super) fn defer_shutdown(cx: &mut App) -> bool {
    if !moving(cx) {
        return false;
    }
    cx.global_mut::<MoveInFlight>().quit_requested = true;
    true
}

pub(super) enum Action {
    Menu {
        target: Instance,
        top: f32,
    },
    Confirm(Instance),
    Deleting(String),
    Problem {
        heading: String,
        message: Option<String>,
        error: String,
        details: bool,
    },
}

impl Launcher {
    pub(super) fn open_profile_menu(&mut self, target: &Instance, top: f32) {
        self.ui.sheets.profile_action = Some(Action::Menu {
            target: target.clone(),
            top,
        });
    }

    pub(super) fn confirm_profile_delete(&mut self, target: &Instance) {
        self.ui.sheets.profile_action = Some(Action::Confirm(target.clone()));
    }

    pub(super) fn close_profile_action(&mut self) {
        if !matches!(self.ui.sheets.profile_action, Some(Action::Deleting(_))) {
            self.ui.sheets.profile_action = None;
        }
    }

    fn delete_profile(&mut self, cx: &mut Context<Self>) {
        let Some(Action::Confirm(target)) = &self.ui.sheets.profile_action else {
            return;
        };
        let target = target.clone();
        let (id, name) = (target.id.clone(), target.name.clone());
        self.ui.sheets.profile_action = Some(Action::Deleting(name.clone()));
        cx.set_global(MoveInFlight::default());
        let task = cx
            .background_executor()
            .spawn(async move { instances::trash_confirmed(target) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_profile_delete(&id, &name, result);
                let movement = cx.remove_global::<MoveInFlight>();
                // A failed move or save leaves its result on screen, even after a close request.
                if movement.quit_requested && this.ui.sheets.profile_action.is_none() {
                    App::defer(cx, crate::shut_down);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_profile_delete(
        &mut self,
        id: &str,
        name: &str,
        result: Result<Instance, DeleteError>,
    ) {
        self.refresh_instances_list();
        let removed = result.is_ok() || !self.instances.list.iter().any(|i| i.id == id);
        // Downloads also save these settings. Clear the profile in the window's current
        // settings and commit them here, not from a second copy in the Trash worker.
        let failed_save = if removed && instances::clear_deleted_selection(&mut self.settings, id) {
            self.clear_profile_notices();
            match self.settings.save() {
                Ok(()) => {
                    self.settings_save_error = None;
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            None
        };
        self.ui.sheets.profile_action = match result {
            Ok(gone) => {
                storage::eviction::request_cleanup();
                let heading = trf("{name} is in the Trash.", &[("name", &gone.name)]);
                if let Some(error) = failed_save {
                    Some(Action::Problem {
                        heading,
                        message: Some(
                            tr("The new profile selection could not be saved.").to_owned(),
                        ),
                        error,
                        details: false,
                    })
                } else {
                    crate::glass_toast::say(heading);
                    None
                }
            }
            Err(error) => Some(Action::Problem {
                heading: trf("Couldn't move {name} to the Trash", &[("name", name)]),
                message: error.retained.map(|path| {
                    trf(
                        "The profile was kept in {folder}",
                        &[("folder", &path.to_string_lossy())],
                    )
                }),
                error: error.detail,
                details: false,
            }),
        };
    }

    pub(super) fn profile_action_sheet(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let action = self.ui.sheets.profile_action.as_ref()?;
        let body = div().flex().flex_col();
        let footer = || div().flex().justify_end().gap(px(8.));
        let close = |id, label, cx: &mut Context<Self>| {
            button(id, label).on_click(cx.listener(|this, _, _, cx| {
                this.close_profile_action();
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
                .child(div().text_color(rgb(palette().muted)).child(crate::selectable::selectable(trf(
                    "If you restore it from the Trash, move the restored folder back to {folder} in your game folder.",
                    &[("folder", &target.work.strip_prefix(backend::game_home()).unwrap_or(&target.work).to_string_lossy())],
                ))))
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
                .children(message.clone().map(crate::selectable::selectable))
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
        Some(modal_frame(
            "profile-delete-sheet",
            450.,
            body,
            cx,
            Self::close_profile_action,
        ))
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
                    this.close_profile_action();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_download_save_and_install_prompt_survive_a_profile_delete() {
        const CHILD: &str = "SUNRISE_PROFILE_DELETE_TEST";
        let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
            let scratch = tempfile::tempdir().unwrap();
            let root = scratch.path();
            let nowhere = "http://127.0.0.1:9";
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "profile_delete::tests::a_download_save_and_install_prompt_survive_a_profile_delete"])
                .env(CHILD, root)
                .env("SUNRISE_HOME", root.join("game"))
                .env("SUNRISE_LAUNCHER_HOME", root.join("launcher"))
                .env("SUNRISE_LAUNCHER_LOCATION_FILE", root.join("location.json"))
                .env("SUNRISE_LAUNCHER_TRASH", root.join("trash"))
                .env("SUNRISE_NO_KEYCHAIN", "1")
                .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
                .env("SUNRISE_TELEMETRY_URL", "off")
                .env("XDG_DATA_HOME", root.join("data"))
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("HTTP_PROXY", nowhere)
                .env("HTTPS_PROXY", nowhere)
                .env("ALL_PROXY", nowhere)
                .env_remove("NO_PROXY")
                .env_remove("SUNRISE_GITHUB_MIRROR")
                .env_remove("SUNRISE_LAUNCHER_BENCH")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        };
        let profile = instances::create("To Delete", None).unwrap();
        let mut settings = Settings::load();
        settings.instance = Some(profile.id.clone());
        settings.selected = Some("26.3-0.1.0".into());
        settings.prune_old_versions = Some(false);
        settings.save().unwrap();
        let mut app = gpui::TestAppContext::single();
        let (shell, cx) = app.add_window_view(crate::window_root::Shell::new);
        let launcher = shell.read_with(cx, |shell, _| shell.launcher());
        let window = cx.update(|window, _| window.window_handle());
        let ask = |launcher: &mut Launcher| {
            let (send, recv) = std::sync::mpsc::channel();
            launcher.registry_consent(
                native_mods::Consent {
                    added_crates: 1,
                    build_scripts: vec!["fixture".into()],
                    proc_macros: Vec::new(),
                },
                send,
            );
            recv
        };
        launcher.update(cx, |launcher, cx| {
            // Feed the real message handler synchronously rather than race the wake relay.
            (launcher.tx, launcher.rx) = std::sync::mpsc::channel();
            // Freeze the worker's completion until the download handler has saved.
            launcher.ui.sheets.profile_action = Some(Action::Deleting(profile.name.clone()));
            let canceled = ask(launcher);
            assert!(launcher.sheet_escape());
            assert_eq!(canceled.try_recv(), Ok(false));
            assert!(matches!(
                launcher.ui.sheets.profile_action,
                Some(Action::Deleting(_))
            ));

            launcher
                .tx
                .send(Msg::Installed(
                    "26.3-0.1.1".into(),
                    Ok(backend::InstallOutcome::Installed(None)),
                ))
                .unwrap();
            launcher.drain(window, cx);
            let live = Settings::load();
            assert_eq!(live.selected.as_deref(), Some("26.3-0.1.1"));
            assert_eq!(live.instance.as_ref(), Some(&profile.id));

            let pending = ask(launcher);
            let trashed = instances::trash_confirmed(profile.clone());
            assert!(trashed.is_ok(), "{trashed:?}");
            launcher.finish_profile_delete(&profile.id, &profile.name, trashed);
            assert!(launcher.ui.sheets.profile_action.is_none());
            assert_eq!(
                pending.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            );
            assert!(launcher.sheet_escape());
            assert_eq!(pending.try_recv(), Ok(false));
        });
        let saved = Settings::load();
        assert!(saved.instance.is_none());
        assert_eq!(saved.selected.as_deref(), Some("26.3-0.1.1"));
        assert!(root.join("trash").join(&profile.id).is_dir());
    }
}
