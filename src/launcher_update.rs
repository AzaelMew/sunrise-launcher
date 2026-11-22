//! Launcher update actions, shared by the top bar, its release-notes sheet and Settings.
use super::*;
use crate::frame::{button, primary};
use crate::paint::palette;
use crate::sheets::{self, Sheet};
use gpui::rgb;
use sunrise_launcher::update_handoff;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Status<'a> {
    Available,
    Downloading(f32),
    Ready,
    Disabled,
    Stuck(Option<&'a str>),
    Failed(&'a str),
}

#[derive(Clone, Copy)]
pub(super) enum Wait {
    Game,
    Work,
}

pub(super) struct Offer<'a> {
    release: &'a Release,
    pub(super) version: &'a str,
    pub(super) status: Status<'a>,
    dot: bool,
}

impl<'a> Offer<'a> {
    fn new(
        state: &'a SelfUpdate,
        found: Option<&'a Release>,
        dismissed: Option<&str>,
        allowed: bool,
        managed: bool,
    ) -> Option<Self> {
        let (release, status) = match state {
            SelfUpdate::Idle => (found?, Status::Available),
            SelfUpdate::Downloading(release, progress) => (release, Status::Downloading(*progress)),
            SelfUpdate::Ready(release) => (release, Status::Ready),
            SelfUpdate::Stuck(error) => (found?, Status::Stuck(error.as_deref())),
            SelfUpdate::Failed(release, error) => (release, Status::Failed(error)),
        };
        let version = backend::launcher_version(&release.tag)?;
        let status = if !allowed {
            Status::Disabled
        } else if managed {
            Status::Stuck(None)
        } else {
            status
        };
        Some(Self {
            release,
            version,
            status,
            dot: dismissed == Some(version),
        })
    }

    pub(super) fn action(&self, wait: Option<Wait>, armed: bool) -> Option<String> {
        if armed
            || !matches!(
                self.status,
                Status::Available | Status::Ready | Status::Failed(_)
            )
        {
            return None;
        }
        Some(match wait {
            Some(Wait::Game) => tr("Update when you close the game").into(),
            Some(Wait::Work) => tr("Update when the current task finishes").into(),
            None => match self.status {
                Status::Ready => tr("Restart to update").into(),
                Status::Failed(_) => tr("Try update again").into(),
                _ => trf("Update to {version}", &[("version", self.version)]),
            },
        })
    }

    pub(super) fn about(&self, launcher: &Launcher) -> String {
        match self.status {
            Status::Downloading(progress) => trf(
                "Downloading {version} · {percent}%",
                &[
                    ("version", self.version),
                    ("percent", &format!("{:.0}", progress * 100.)),
                ],
            ),
            _ if launcher.restart_for_launcher_update => match launcher.launcher_update_wait() {
                Some(Wait::Game) => tr("The launcher will update when you close the game.").into(),
                Some(Wait::Work) => {
                    tr("The launcher will update when the current task finishes.").into()
                }
                None => tr("Ready to restart").into(),
            },
            Status::Ready => tr("Ready to restart").into(),
            Status::Stuck(error) => launcher.launcher_update_help(error),
            Status::Failed(error) => trf("the update failed: {error}", &[("error", error)]),
            Status::Disabled => tr("Automatic launcher updates are disabled.").into(),
            Status::Available => trf("{version} available", &[("version", self.version)]),
        }
    }
}

impl SelfUpdate {
    fn ready_to_restart(
        &self,
        armed: bool,
        allowed: bool,
        idle: impl FnOnce() -> bool,
    ) -> Option<&Release> {
        match self {
            Self::Ready(release) if armed && allowed && idle() => Some(release),
            _ => None,
        }
    }
}

impl Launcher {
    pub(super) fn note_launcher_offer(&mut self, found: Option<&Release>) {
        match &self.self_update {
            SelfUpdate::Failed(attempt, _)
                if found.is_none_or(|release| release.tag != attempt.tag) =>
            {
                self.self_update = SelfUpdate::Idle;
            }
            SelfUpdate::Ready(staged)
                if !self.restart_for_launcher_update
                    && found.is_some_and(|release| {
                        backend::compare_versions(&release.tag, &staged.tag).is_gt()
                    }) =>
            {
                // A paused restart can't keep a newer release hidden behind the old version's dot.
                update_handoff::forget();
                self.self_update = SelfUpdate::Idle;
            }
            _ => {}
        }
        if found.is_some()
            && matches!(self.self_update, SelfUpdate::Idle)
            && backend::self_update_allowed()
            && !backend::package_managed_launcher()
            && backend::cannot_replace_itself()
        {
            self.self_update = SelfUpdate::Stuck(None);
        }
        if found.is_none()
            && self.ui.sheet == Some(Sheet::LauncherUpdate)
            && matches!(self.self_update, SelfUpdate::Idle | SelfUpdate::Stuck(_))
        {
            // No sheet left to close: don't answer any other dialog on top of it.
            self.ui.sheet = None;
        }
    }

    pub(super) fn launcher_update_offer(&self) -> Option<Offer<'_>> {
        Offer::new(
            &self.self_update,
            self.updates.launcher.as_ref(),
            self.settings.dismissed_launcher_update.as_deref(),
            backend::self_update_allowed(),
            backend::package_managed_launcher(),
        )
    }

    pub(super) fn launcher_update_wait(&self) -> Option<Wait> {
        if self.client.is_some() || self.starting_client.is_some() || self.starting_pack.is_some() {
            Some(Wait::Game)
        } else if self.launch_busy()
            || self.finishing_game > 0
            || !self.auth_busy.is_empty()
            || (self.sign_in.is_some() && !matches!(self.sign_in_state, Some(SignInState::Done(_))))
        {
            Some(Wait::Work)
        } else {
            None
        }
    }

    pub(super) fn request_launcher_update(&mut self) {
        let Some(offer) = self.launcher_update_offer() else {
            return;
        };
        match offer.status {
            Status::Available | Status::Failed(_) => {
                if backend::cannot_replace_itself() {
                    self.self_update = SelfUpdate::Stuck(None);
                    self.open_sheet(Sheet::LauncherUpdate);
                    return;
                }
                let release = offer.release.clone();
                self.self_update = SelfUpdate::Downloading(release.clone(), 0.);
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let progress = |p| {
                        let _ = tx.send(Msg::LauncherProgress(p));
                    };
                    let _ = tx.send(Msg::LauncherUpdated(backend::stage_update(
                        &release, progress,
                    )));
                });
            }
            Status::Ready => {}
            Status::Downloading(_) | Status::Disabled | Status::Stuck(_) => return,
        }
        self.restart_for_launcher_update = true;
    }

    pub(super) fn launcher_update_finished(
        &mut self,
        result: Result<Option<backend::Staged>, String>,
    ) {
        let SelfUpdate::Downloading(release, _) = &self.self_update else {
            return;
        };
        let release = release.clone();
        self.self_update = match result {
            Ok(staged) => {
                update_handoff::set_pending(staged);
                SelfUpdate::Ready(release)
            }
            Err(error) => {
                self.restart_for_launcher_update = false;
                SelfUpdate::Failed(release, error)
            }
        };
    }

    /// Only the player's update action arms a restart. Closing the window still drops the stage.
    pub(super) fn restart_launcher_if_ready(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.self_update.ready_to_restart(
            self.restart_for_launcher_update,
            backend::self_update_allowed(),
            || self.launcher_update_wait().is_none(),
        ) else {
            return;
        };
        self.restart_for_launcher_update = false;
        let error = match update_handoff::start_now() {
            Ok(true) => {
                servers_page::keep_running_through_update();
                shut_down(cx);
                return;
            }
            Ok(false) => backend::restart(),
            Err(error) => error,
        };
        self.self_update = SelfUpdate::Failed(release.clone(), error);
        // This can run on a tick with no messages, just after an Update press.
        cx.notify();
    }

    pub(super) fn launcher_update_control(&self, cx: &mut Context<Self>) -> Option<Div> {
        let offer = self.launcher_update_offer()?;
        let p = palette();
        let wait = self.launcher_update_wait();
        let action = offer.action(wait, self.restart_for_launcher_update);
        let available = trf("Update available: {version}", &[("version", offer.version)]);
        let open = cx.listener(|this, _, _, cx| {
            this.open_sheet(Sheet::LauncherUpdate);
            cx.notify();
        });
        if offer.dot {
            return Some(
                div().flex_none().child(
                    button("launcher-update-dot", "")
                        .size(px(tokens::FIELD))
                        .px_0()
                        .tooltip(crate::glass_ui::tip(available))
                        .child(div().size(px(7.)).rounded_full().bg(rgb(p.accent)))
                        .on_click(open),
                ),
            );
        }
        let label = match (&action, wait, offer.status) {
            (Some(_), Some(Wait::Game), _) => tr("Update after game").into(),
            (Some(_), Some(Wait::Work), _) => tr("Update after task").into(),
            (Some(label), _, _) => label.clone(),
            (None, _, Status::Downloading(progress)) => trf(
                "Downloading · {percent}%",
                &[("percent", &format!("{:.0}", progress * 100.))],
            ),
            (None, _, Status::Ready) => tr("Update queued").into(),
            _ => available,
        };
        let tip = action.clone().unwrap_or_else(|| label.clone());
        let main = button("launcher-update", "")
            .max_w(px(172.))
            .overflow_hidden()
            .h(px(tokens::FIELD))
            .gap(px(6.))
            .tooltip(crate::glass_ui::tip(tip))
            .child(div().min_w_0().truncate().child(label))
            .when(action.is_none(), |b| {
                b.child(
                    crate::glass_ui::icon("information-circle", 16., rgb(p.chrome_muted).into())
                        .flex_none(),
                )
            })
            .when(action.is_some(), |b| {
                b.border_color(rgb(p.accent)).text_color(rgb(p.chrome_text))
            });
        let main = if action.is_some() {
            main.on_click(cx.listener(|this, _, _, cx| {
                this.request_launcher_update();
                cx.notify();
            }))
        } else {
            main.on_click(open)
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .child(main)
                .when(action.is_some(), |row| {
                    row.child(
                        button("launcher-update-notes", "")
                            .size(px(tokens::FIELD))
                            .px_0()
                            .tooltip(crate::glass_ui::tip(tr("What's new")))
                            .child(crate::glass_ui::icon(
                                "information-circle",
                                16.,
                                rgb(p.chrome_muted).into(),
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_sheet(Sheet::LauncherUpdate);
                                cx.notify();
                            })),
                    )
                }),
        )
    }

    pub(super) fn launcher_update_sheet(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let offer = self.launcher_update_offer()?;
        let title = trf("Sunrise Launcher {version}", &[("version", offer.version)]);
        let current = trf(
            "Installed version: {version}",
            &[("version", backend::VERSION)],
        );
        let notes = backend::notes_summary(&offer.release.body);
        let url = format!(
            "https://github.com/{}/releases/tag/{}",
            backend::LAUNCHER_REPO,
            offer.release.tag
        );
        let status = offer.about(self);
        let action = offer.action(
            self.launcher_update_wait(),
            self.restart_for_launcher_update,
        );
        let dismiss = (!offer.dot).then(|| {
            let version = offer.version.to_owned();
            button("launcher-update-hide", tr("Hide for this version")).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.settings.dismissed_launcher_update = Some(version.clone());
                    this.save_settings();
                    this.close_sheet();
                    cx.notify();
                },
            ))
        });
        let cancel = self.restart_for_launcher_update.then(|| {
            button("launcher-update-cancel-restart", tr("Cancel restart")).on_click(cx.listener(
                |this, _, _, cx| {
                    this.restart_for_launcher_update = false;
                    cx.notify();
                },
            ))
        });
        let p = palette();
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(sheets::title(title))
                    .child(
                        div()
                            .text_size(px(tokens::LABEL))
                            .text_color(rgb(p.muted))
                            .child(current),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .items_start()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("What's new")),
                    )
                    .children(notes.map(crate::selectable::selectable))
                    .child(
                        button("launcher-release-notes", tr("Read the full release notes"))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(14.))
                    .border_t_1()
                    .border_color(rgb(p.line))
                    .child(
                        div()
                            .text_size(px(tokens::LABEL))
                            .text_color(rgb(p.muted))
                            .child(crate::selectable::selectable(status)),
                    )
                    .children(match offer.status {
                        Status::Downloading(progress) => Some(crate::widgets::progress_bar(
                            Some((progress, progress)),
                            &self.theme,
                        )),
                        _ => None,
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(8.))
                    .children(dismiss)
                    .child(
                        button("launcher-update-later", tr("Later")).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.close_sheet();
                                cx.notify();
                            },
                        )),
                    )
                    .children(cancel)
                    .children(action.map(|label| {
                        primary("launcher-update-confirm", label).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.request_launcher_update();
                                cx.notify();
                            },
                        ))
                    })),
            );
        Some(sheets::frame("launcher-update-sheet", 520., body, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.into(),
            name: String::new(),
            body: "A shorter route to your installed packs.".into(),
            date: String::new(),
            time: String::new(),
            asset: None,
            prerelease: false,
            nightly: None,
        }
    }

    #[test]
    fn shows_a_discovered_update_and_keeps_showing_it_through_download_and_restart() {
        assert!(Offer::new(&SelfUpdate::Idle, None, None, true, false).is_none());
        let found = release("v1.3.0");
        let offer = Offer::new(&SelfUpdate::Idle, Some(&found), None, true, false).unwrap();
        assert_eq!(offer.status, Status::Available);
        assert_eq!(
            offer.action(None, false).as_deref(),
            Some("Update to 1.3.0")
        );
        assert!(!offer.dot);
        // A refresh cannot strip an active update of its version or notes.
        let downloading = SelfUpdate::Downloading(found.clone(), 0.5);
        let offer = Offer::new(&downloading, None, None, true, false).unwrap();
        assert_eq!(offer.version, "1.3.0");
        assert_eq!(offer.status, Status::Downloading(0.5));
        assert!(offer.action(None, true).is_none());
        let ready = SelfUpdate::Ready(found);
        let offer = Offer::new(&ready, None, None, true, false).unwrap();
        assert_eq!(offer.status, Status::Ready);
        assert_eq!(
            offer.action(None, false).as_deref(),
            Some("Restart to update")
        );
    }

    #[test]
    fn dismissal_survives_reopening_but_only_for_the_same_version() {
        let saved = serde_json::to_string(&Settings {
            dismissed_launcher_update: Some("1.3.0".into()),
            ..Settings::default()
        })
        .unwrap();
        let settings = Settings::parse(&saved);
        let dismissed = settings.dismissed_launcher_update.as_deref();
        for (tag, dot) in [("v1.3.0", true), ("v1.3.1", false)] {
            let found = release(tag);
            let offer =
                Offer::new(&SelfUpdate::Idle, Some(&found), dismissed, true, false).unwrap();
            assert_eq!(offer.dot, dot);
        }
        assert!(Settings::parse("{}").dismissed_launcher_update.is_none());
    }

    #[test]
    fn disabled_self_update_and_managed_installs_show_notes_but_no_install_action() {
        let found = release("v1.3.0");
        let offer = Offer::new(&SelfUpdate::Idle, Some(&found), None, false, false).unwrap();
        assert_eq!(offer.status, Status::Disabled);
        assert!(offer.action(None, false).is_none());
        assert_eq!(offer.version, "1.3.0");
        assert_eq!(offer.release.body, found.body);
        let offer = Offer::new(&SelfUpdate::Idle, Some(&found), None, true, true).unwrap();
        assert_eq!(offer.status, Status::Stuck(None));
        assert!(offer.action(None, false).is_none());
    }

    #[test]
    fn an_armed_update_restarts_only_after_verification_and_when_idle() {
        let found = release("v1.3.0");
        let offer = Offer::new(&SelfUpdate::Idle, Some(&found), None, true, false).unwrap();
        assert_eq!(
            offer.action(Some(Wait::Game), false).as_deref(),
            Some("Update when you close the game")
        );
        assert!(SelfUpdate::Downloading(found.clone(), 1.0)
            .ready_to_restart(true, true, || true)
            .is_none());
        let ready = SelfUpdate::Ready(found);
        assert!(ready.ready_to_restart(true, true, || false).is_none());
        assert!(ready.ready_to_restart(false, true, || true).is_none());
        assert!(ready.ready_to_restart(true, false, || true).is_none());
        assert!(ready.ready_to_restart(true, true, || true).is_some());
    }
}
