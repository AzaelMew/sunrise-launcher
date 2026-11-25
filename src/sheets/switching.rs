use super::*;
use crate::mods::PortSwitch;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher::{
    browser::sunrise::{
        install_target, save_build_choice, switch_profile_build_v2, switch_provider_v2,
        BuildChoice, InstallError, InstallHooks, InstallProgress, ProviderSwitchPreview,
        SwitchDecision, SwitchHooks, SwitchQuestion, Switched, V2Hooks, V2InstallOptions,
        V2ProviderSwitchHooks, V2SwitchHooks,
    },
    managed_play::{self, Active, FirstBuild, Profile, ReceiptHint},
    profile_txn::Identity,
};

#[derive(Default)]
pub(crate) struct State {
    pub(super) cache_download: Option<String>,
    running: Option<Running>,
    provider: Option<ProviderRunning>,
    ack: bool,
    receipt: Option<ReceiptRead>,
}

struct ProviderRunning {
    switch: PortSwitch,
    profile_name: String,
    build_label: String,
    progress: InstallProgress,
    eta: crate::eta::StageEta,
    stop: native_mods::Cancellation,
    question: Option<(Box<ProviderSwitchPreview>, Sender<bool>)>,
}

struct ReceiptRead {
    home: PathBuf,
    hint: Result<Option<ReceiptHint>, String>,
    ticket: Arc<()>,
    status: Load<Option<(Identity, Active)>>,
}

struct Running {
    tag: String,
    /// The profile and the build it played when the switch began, shown until the switch ends.
    from: (PathBuf, Option<String>),
    back: Option<Sheet>,
    pending: Option<Pending>,
    progress: InstallProgress,
    eta: crate::eta::StageEta,
    stop: native_mods::Cancellation,
    question: Option<(Box<SwitchQuestion>, Sender<SwitchDecision>)>,
}

/// The selected profile and build survive download and preparation, not a new read of Home.
struct Pending {
    instance: instances::Instance,
    identity: Identity,
    choice: Option<BuildChoice>,
}

impl Launcher {
    pub(crate) fn download_for_switch(&mut self, release: &backend::Release) {
        if self.installing.is_some()
            || self.installed.contains(&release.tag)
            || self.removing.contains(&release.tag)
            || release.asset.is_none()
        {
            return;
        }
        self.ui.sheets.switching.cache_download = Some(release.tag.clone());
        self.install(release);
    }

    pub(crate) fn finish_switch_download(
        &mut self,
        tag: &str,
        result: &Result<backend::InstallOutcome, String>,
    ) -> bool {
        if self
            .ui
            .sheets
            .switching
            .cache_download
            .take_if(|current| current == tag)
            .is_none()
        {
            return false;
        }
        if let Some(running) = &self.ui.sheets.switching.running {
            let pending = running.pending.as_ref().expect("switch download is queued");
            if running.stop.is_stopped() {
                let home = pending.instance.home.clone();
                self.switch_finished(&home, Ok(Switched::Stayed));
            } else if let Err(error) = result {
                let home = pending.instance.home.clone();
                self.switch_finished(&home, Err(error.clone()));
            } else {
                self.prepare_switch();
            }
        } else {
            self.ui.sheets.error = result.as_ref().err().cloned();
        }
        true
    }

    pub(crate) fn switch_download_progress(&mut self, tag: &str, fraction: f32) {
        if self.ui.sheets.switching.cache_download.as_deref() == Some(tag) {
            self.switch_progress(InstallProgress::DownloadingBuild(fraction));
        }
    }

    /// The build `home` played when a switch running on it began.
    pub(crate) fn switch_base(&self, home: &Path) -> Option<&str> {
        let running = self.ui.sheets.switching.running.as_ref()?;
        (running.from.0 == home)
            .then_some(running.from.1.as_deref())
            .flatten()
    }

    pub(crate) fn switch_busy(&self) -> bool {
        self.ui.sheets.switching.running.is_some()
            || self.ui.sheets.switching.provider.is_some()
            || self.ui.sheets.switching.ack
    }

    pub(super) fn begin_switch(&mut self, tag: String, choice: Option<BuildChoice>) {
        if self.launch_busy() || self.client.is_some() || self.removing.contains(&tag) {
            return;
        }
        let Some(instance) = self.selected_instance().cloned() else {
            return;
        };
        let identity = match install_target(&instance) {
            Ok(identity) => identity,
            Err(error) => {
                self.open_notice(error, Some(WayOut::ProfileFolder(instance.home)));
                return;
            }
        };
        // Changing only the channel/pin preference when the profile is already on `tag` (and has
        // no pending recovery or first-Play bootstrap) needs no switch modal, registry fetch, or
        // mod rebuild. Keep artifact hashing off the UI thread.
        let unplayed_target = self
            .target_version(instance.version.as_ref(), instance.channel)
            .map(String::as_str);
        if can_fast_save_choice(
            &instance,
            &tag,
            unplayed_target,
            self.installed.contains(&tag),
        ) {
            if let Some(choice) = choice {
                if let Err(error) = save_build_choice(&instance.id, &tag, choice) {
                    self.open_notice(error, Some(WayOut::ProfileFolder(instance.home)));
                    return;
                }
                self.reload_instances();
            }
            self.close_sheet();
            return;
        }
        let release = self.release_to_install(&tag);
        let stop = native_mods::Cancellation::default();
        let from = (instance.home.clone(), self.profile_target(&instance));
        self.ui.sheets.switching.running = Some(Running {
            tag: tag.clone(),
            from,
            back: self.ui.sheet,
            pending: Some(Pending {
                instance,
                identity,
                choice,
            }),
            progress: InstallProgress::PreparingBuild,
            eta: crate::eta::StageEta::default(),
            stop: stop.clone(),
            question: None,
        });
        self.open_sheet(Sheet::Switching);
        if let Some(release) = release {
            self.ui.sheets.switching.cache_download = Some(tag);
            self.switch_progress(InstallProgress::DownloadingBuild(0.));
            self.install_cancellable(&release, Some(stop));
        } else {
            self.prepare_switch();
        }
    }

    fn prepare_switch(&mut self) {
        let running = self
            .ui
            .sheets
            .switching
            .running
            .as_ref()
            .expect("switch queued");
        if backend::check_prepared(&running.tag) {
            self.start_switch_worker();
        } else {
            let (tag, stop) = (running.tag.clone(), running.stop.clone());
            self.switch_progress(InstallProgress::Minecraft(
                backend::PrepareStep::Files,
                None,
            ));
            self.prepare_version_cancellable(tag, backend::MinecraftFrom::Install, Some(stop));
        }
    }

    /// A line from the preparation a switch waits on: the dialog follows its step, and the
    /// files done while Mojang's download runs.
    pub(crate) fn follow_switch_preparation(&mut self) {
        if let Some(running) = self
            .ui
            .sheets
            .switching
            .running
            .as_mut()
            .filter(|run| matches!(run.progress, InstallProgress::Minecraft(..)))
        {
            let stage = InstallProgress::Minecraft(
                backend::prepare_step(),
                sunrise_launcher::mojang::progress(),
            );
            running.progress = stage;
            running.eta.update(
                crate::eta::install_stage_key(stage),
                crate::mods_page::install::stage_fraction(stage),
                Instant::now(),
            );
        }
    }

    pub(crate) fn finish_switch_preparation(&mut self) {
        let Some(running) = self.ui.sheets.switching.running.as_ref() else {
            return;
        };
        let Some(pending) = running.pending.as_ref() else {
            return;
        };
        let error = self.prepare_error.take();
        if running.stop.is_stopped() {
            let home = pending.instance.home.clone();
            self.switch_finished(&home, Ok(Switched::Stayed));
        } else if let Some(error) = error {
            let home = pending.instance.home.clone();
            self.switch_finished(&home, Err(error));
        } else {
            self.start_switch_worker();
        }
    }

    fn start_switch_worker(&mut self) {
        let running = self
            .ui
            .sheets
            .switching
            .running
            .as_mut()
            .expect("switch queued");
        let Pending {
            instance,
            identity,
            choice,
        } = running.pending.take().expect("switch queued");
        // Both switch routes start by checking the profile's mods; a v2 profile first
        // refreshes its index, so don't leave the last download or preparation step showing.
        running.progress = InstallProgress::PreparingBuild;
        running.eta.update(
            crate::eta::install_stage_key(InstallProgress::PreparingBuild),
            None,
            Instant::now(),
        );
        let (tag, stop) = (running.tag.clone(), running.stop.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let hooks = SwitchHooks {
                cancellation: &stop,
                progress: &|stage| {
                    let _ = tx.send(Msg::SwitchProgress(stage));
                },
                at_cutoff: &|| Ok(()),
                approve: &|question| {
                    if stop.is_stopped() {
                        return Ok(SwitchDecision::Cancel);
                    }
                    let (answer, reply) = std::sync::mpsc::channel();
                    tx.send(Msg::SwitchQuestion(Box::new(question.clone()), answer))
                        .map_err(|_| tr("Switch cancelled.").to_owned())?;
                    Ok(reply.recv().unwrap_or(SwitchDecision::Cancel))
                },
            };
            // Mods installed before the signed registry keep their exact bytes through it.
            let outcome = switch_profile_build_v2(
                &instance,
                identity,
                &tag,
                choice,
                V2SwitchHooks {
                    switch: hooks,
                    confirm_publisher_change: Some(&|question| {
                        let (answer, reply) = std::sync::mpsc::channel();
                        tx.send(Msg::PublisherChangeQuestion(
                            Box::new(question.clone()),
                            answer,
                        ))
                        .is_ok()
                            && reply.recv().unwrap_or(false)
                    }),
                    pin_dir: None,
                },
            );
            let _ = tx.send(Msg::Switched(instance.home, Box::new(outcome)));
        });
    }

    pub(super) fn cancel_switch(&mut self) {
        let Some(running) = &mut self.ui.sheets.switching.running else {
            return;
        };
        if running.progress != InstallProgress::Publishing {
            running.stop.stop();
            if let Some((_, reply)) = running.question.take() {
                let _ = reply.send(SwitchDecision::Cancel);
            }
        }
    }

    pub(crate) fn switch_progress(&mut self, stage: InstallProgress) {
        if let Some(running) = &mut self.ui.sheets.switching.running {
            running.progress = stage;
            running.eta.update(
                crate::eta::install_stage_key(stage),
                crate::mods_page::install::stage_fraction(stage),
                Instant::now(),
            );
        }
    }

    pub(crate) fn switch_question(
        &mut self,
        question: Box<SwitchQuestion>,
        reply: Sender<SwitchDecision>,
    ) {
        if let Some(running) = self
            .ui
            .sheets
            .switching
            .running
            .as_mut()
            .filter(|run| self.ui.sheet == Some(Sheet::Switching) && !run.stop.is_stopped())
        {
            running.question = Some((question, reply));
            self.ui.sheets.details = false;
        } else {
            let _ = reply.send(SwitchDecision::Cancel);
        }
    }

    pub(crate) fn switch_finished(
        &mut self,
        home: &std::path::Path,
        result: Result<Switched, String>,
    ) {
        let running = self.ui.sheets.switching.running.take();
        let stopped = running.as_ref().is_some_and(|run| run.stop.is_stopped());
        let back = running.and_then(|run| run.back);
        let result = if result.is_err() && stopped {
            Ok(Switched::Stayed)
        } else {
            result
        };
        self.refresh_installed();
        if matches!(result, Ok(Switched::Changed(_) | Switched::AlreadySelected)) {
            self.refresh_instances_list();
        }
        if self.selected_instance().is_none_or(|i| i.home != home) {
            return;
        }
        let visible = self.ui.sheet == Some(Sheet::Switching);
        match result {
            Ok(Switched::Changed(active)) => {
                if self
                    .updates
                    .sunrise
                    .as_ref()
                    .is_some_and(|r| r.tag == active.base())
                {
                    self.updates.sunrise = None;
                }
                if active.pending_changes().is_some() {
                    self.ui.sheets.switching.receipt = None;
                    self.open_sheet(Sheet::BuildChanges);
                } else if visible {
                    self.close_sheet();
                }
            }
            Ok(Switched::Stayed) => {
                if visible {
                    if let Some(back) = back {
                        self.open_sheet(back);
                    } else {
                        self.close_sheet();
                    }
                }
            }
            Ok(Switched::AlreadySelected) => {
                if visible {
                    self.close_sheet();
                }
            }
            Err(error) if visible => {
                self.open_notice(error, Some(WayOut::ProfileFolder(home.into())))
            }
            Err(_) => (),
        }
    }

    pub(super) fn switch_sheet(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use crate::mods_page::install::{stage_fraction, stage_label};
        let running = self
            .ui
            .sheets
            .switching
            .running
            .as_ref()
            .expect("Switch sheet follows its worker");
        let build = build_name(&running.tag);
        let body = if let Some((question, _)) = &running.question {
            let show = self.ui.sheets.details;
            let omissions = question.has_omissions();
            let one = question.held.len() + question.unproved.len() == 1;
            let answer = |decision| {
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some((_, reply)) = this
                        .ui
                        .sheets
                        .switching
                        .running
                        .as_mut()
                        .and_then(|running| running.question.take())
                    {
                        let _ = reply.send(decision);
                    }
                    cx.notify();
                })
            };
            let has_mod_block = omissions || !question.removed.is_empty();
            let mod_block = has_mod_block.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(omissions.then(|| muted(question.omission_summary())))
                    .child(
                        div()
                            .id("switch-mods")
                            .max_h(px(184.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .children((omissions && !show).then(|| {
                                let mut names: Vec<_> = question
                                    .held
                                    .iter()
                                    .chain(&question.unproved)
                                    .map(String::as_str)
                                    .collect();
                                names.sort_unstable();
                                div().child(names.join(", "))
                            }))
                            .children(
                                show.then(|| question.hold_lines().map(|line| div().child(line)))
                                    .into_iter()
                                    .flatten(),
                            )
                            .children(question.removed.iter().map(|(name, reason)| {
                                div().child(if show {
                                    trf(
                                        "{mod} was taken down: {reason}",
                                        &[("mod", name), ("reason", reason)],
                                    )
                                } else {
                                    trf(
                                        "{mod} was taken down and will be removed.",
                                        &[("mod", name)],
                                    )
                                })
                            })),
                    )
            });
            let world_block = question.worlds.needed().then(|| {
                let mut block = div().flex().flex_col().gap(px(6.));
                if has_mod_block {
                    block = block
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(rgb(palette().line));
                }
                block
                    .child(muted(tr(
                        "Your worlds stay in place. Check what could change before you switch.",
                    )))
                    .child(
                        div()
                            .id("switch-worlds")
                            .max_h(px(150.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .children(
                                question
                                    .world_lines()
                                    .into_iter()
                                    .map(|line| div().child(line)),
                            ),
                    )
            });
            div()
                .flex()
                .flex_col()
                .child(title(trf("Switch to {build}?", &[("build", &build)])))
                .children(mod_block)
                .children(world_block)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(px(8.))
                        .children((omissions || !question.removed.is_empty()).then(|| {
                            button(
                                "switch-why",
                                if show {
                                    tr("Hide details")
                                } else {
                                    tr("Show why")
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ui.sheets.details ^= true;
                                cx.notify();
                            }))
                        }))
                        .child(
                            button("switch-stay", tr("Cancel"))
                                .on_click(answer(SwitchDecision::Cancel)),
                        )
                        .children(omissions.then(|| {
                            button(
                                "switch-remove",
                                if one {
                                    tr("Switch and remove it")
                                } else {
                                    tr("Switch and remove them")
                                },
                            )
                            .text_color(rgb(palette().danger))
                            .on_click(answer(SwitchDecision::Remove))
                        }))
                        .child(
                            crate::frame::primary(
                                "switch-anyway",
                                if omissions && one {
                                    tr("Switch and turn it off")
                                } else if omissions {
                                    tr("Switch and turn them off")
                                } else {
                                    tr("Switch anyway")
                                },
                            )
                            .on_click(answer(SwitchDecision::TurnOff)),
                        ),
                )
        } else {
            let stopping = running.stop.is_stopped();
            let publishing = running.progress == InstallProgress::Publishing;
            let cancel = button("cancel-switch", tr("Cancel"));
            // The step's line, the bar and Cancel keep their places while the steps change, so
            // the dialog never jumps: one line of text, a bar that shuttles while a step has no
            // known length, and Cancel dimmed once it can no longer stop the switch.
            div()
                .flex()
                .flex_col()
                .child(title(trf("Switch to {build}", &[("build", &build)])))
                .child(
                    muted(if stopping {
                        tr("Cancelling…").into()
                    } else if publishing {
                        tr("Applying the switch.").into()
                    } else {
                        let now = Instant::now();
                        let remaining = match running.progress {
                            InstallProgress::Minecraft(backend::PrepareStep::Prepare, _) => {
                                match backend::prepare_step_state() {
                                    (backend::PrepareStep::Prepare, Some(started)) => {
                                        crate::eta::expected_prepare_duration().and_then(
                                            |expected| {
                                                crate::eta::remaining_from_expected(
                                                    started, expected, now,
                                                )
                                            },
                                        )
                                    }
                                    _ => None,
                                }
                            }
                            _ => running.eta.remaining(now),
                        };
                        crate::eta::with_eta(stage_label(running.progress), remaining)
                    })
                    .truncate(),
                )
                .child(crate::widgets::progress_bar(
                    stage_fraction(running.progress)
                        .filter(|_| !stopping)
                        .map(|f| (f, f)),
                    &self.theme,
                ))
                .child(div().flex().justify_end().child(if stopping || publishing {
                    crate::widgets::disabled(cancel)
                } else {
                    cancel.on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_switch();
                        cx.notify();
                    }))
                }))
        };
        frame("switch-game", 610., body, cx)
    }

    pub(crate) fn check_build_changes(&mut self, home: &Path, cx: &mut Context<Self>) -> bool {
        self.read_receipt(home, false, cx);
        self.receipt_status(home)
            .is_some_and(|status| matches!(status, Load::Ready(Some(_)) | Load::Failed(_)))
    }

    pub(crate) fn open_build_changes(&mut self, cx: &mut Context<Self>) {
        if let Some(home) = self.selected_instance().map(|profile| profile.home.clone()) {
            self.open_sheet(Sheet::BuildChanges);
            self.read_receipt(&home, true, cx);
        }
    }

    fn receipt_status(&self, home: &Path) -> Option<&Load<Option<(Identity, Active)>>> {
        self.ui
            .sheets
            .switching
            .receipt
            .as_ref()
            .filter(|read| read.home == home)
            .map(|read| &read.status)
    }

    fn read_receipt(&mut self, home: &Path, explicit: bool, cx: &mut Context<Self>) {
        if !explicit && (self.launch_busy() || self.client.is_some()) {
            return;
        }
        let hint = managed_play::build_changes_hint(home);
        let read = &mut self.ui.sheets.switching.receipt;
        if read.as_ref().is_some_and(|read| {
            read.home == home
                && (matches!(read.status, Load::Loading) || !explicit && read.hint == hint)
        }) {
            return;
        }
        if !explicit && hint == Ok(None) {
            *read = None;
            return;
        }
        let home = home.to_owned();
        let ticket = Arc::new(());
        *read = Some(ReceiptRead {
            home: home.clone(),
            hint,
            ticket: ticket.clone(),
            status: Load::Loading,
        });
        let work = cx
            .background_executor()
            .spawn(async move { managed_play::read_build_changes(&home) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(read) = this
                    .ui
                    .sheets
                    .switching
                    .receipt
                    .as_mut()
                    .filter(|read| Arc::ptr_eq(&read.ticket, &ticket))
                {
                    read.status = result.map_or_else(Load::Failed, Load::Ready);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn switch_receipt_sheet(&mut self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if self.ui.sheets.switching.ack {
            // The receipt worker owns the exclusive publication claim until its reply.
            // Don't turn an expected busy snapshot during that window into a false error.
            return Some(frame(
                "switch-receipt",
                600.,
                div()
                    .flex()
                    .flex_col()
                    .child(title(tr("Build changes").into()))
                    .child(muted(tr("Closing this report…"))),
                cx,
            ));
        }
        let instance = self.selected_instance()?;
        let home = instance.home.clone();
        if self.receipt_status(&home).is_none() {
            self.read_receipt(&home, true, cx);
        }
        let (identity, active) = match self.receipt_status(&home)? {
            Load::Ready(Some((identity, active))) => (*identity, active.clone()),
            Load::Loading | Load::Ready(None) => {
                let message = if matches!(self.receipt_status(&home), Some(Load::Loading)) {
                    tr("Reading build changes…")
                } else {
                    tr("There are no pending build changes for this profile.")
                };
                return Some(frame(
                    "switch-receipt",
                    600.,
                    div()
                        .flex()
                        .flex_col()
                        .child(title(tr("Build changes").into()))
                        .child(muted(message)),
                    cx,
                ));
            }
            Load::Failed(error) => {
                let error = error.clone();
                self.open_notice(error, Some(WayOut::ProfileFolder(home)));
                return Some(self.notice_sheet(cx));
            }
        };
        let changes = active.pending_changes()?;
        let id = changes.id;
        let removed = changes.removed;
        let show = self.ui.sheets.details;
        let removed_line = |(name, reason): &(String, String)| {
            trf(
                "{mod} was removed: {reason}",
                &[("mod", name), ("reason", reason)],
            )
        };
        let lines = changes
            .held_lines()
            .chain(removed.iter().map(removed_line))
            .collect::<Vec<_>>();
        let report = std::sync::Arc::new(format!(
            "{}\n{}\n",
            changes.base().map_or_else(
                || tr("Original game build not recorded.").to_owned(),
                |base| trf("Profile game build: {build}", &[("build", base)]),
            ),
            lines.join("\n")
        ));
        let copy = report.clone();
        let list = div()
            .id("switch-receipt-lines")
            .max_h(px(220.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(5.))
            .children(changes.held_lines().map(|line| div().child(line)))
            .children(removed.iter().map(|row| {
                div().child(if show {
                    removed_line(row)
                } else {
                    trf("{mod} was removed.", &[("mod", &row.0)])
                })
            }));
        let body = div()
            .flex()
            .flex_col()
            .child(title(tr("Build changes").into()))
            .child(list)
            .children(self.ui.sheets.error.clone().map(muted))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(8.))
                    .children(show.then(|| {
                        crate::frame::copy_button(
                            "receipt-copy",
                            tr("Copy report"),
                            copy.as_ref().clone(),
                        )
                    }))
                    .children(show.then(|| {
                        button("receipt-save", tr("Save report")).on_click(cx.listener(
                            move |this, _, _, cx| {
                                use std::io::Write;
                                let saved = (|| {
                                    let folder = dirs::download_dir().ok_or_else(|| {
                                        tr("The Downloads folder isn't available.").to_owned()
                                    })?;
                                    let key = u128::from_be_bytes(id);
                                    let file = folder
                                        .join(format!("sunrise-build-changes-{key:032x}.txt"));
                                    publish_report(&folder, &file, |output| {
                                        output.write_all(report.as_bytes())
                                    })
                                    .map_err(|e| e.to_string())?;
                                    Ok::<_, String>(file)
                                })();
                                this.ui.sheets.error = Some(match saved {
                                    Ok(file) => trf(
                                        "Saved the report to {file}.",
                                        &[("file", &file.display().to_string())],
                                    ),
                                    Err(error) => trf(
                                        "Couldn't save the build report: {error}",
                                        &[("error", &error)],
                                    ),
                                });
                                cx.notify();
                            },
                        ))
                    }))
                    .child(
                        button(
                            "receipt-why",
                            if show {
                                tr("Hide details")
                            } else {
                                tr("Show why")
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ui.sheets.details ^= true;
                            cx.notify();
                        })),
                    )
                    .child(button("receipt-done", tr("Done")).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if this.ui.sheets.switching.ack {
                                return;
                            }
                            this.ui.sheets.switching.ack = true;
                            let (tx, home, active) =
                                (this.tx.clone(), home.clone(), active.clone());
                            std::thread::spawn(move || {
                                let result = Profile::open_for(&home, None, Some(identity))
                                    .map_err(String::from)
                                    .and_then(|mut profile| {
                                        profile.acknowledge_receipt(&active, id)
                                    });
                                let _ = tx.send(Msg::ReceiptAcknowledged(home, result));
                            });
                            cx.notify();
                        },
                    ))),
            );
        Some(frame("switch-receipt", 600., body, cx))
    }

    pub(crate) fn begin_provider_switch(&mut self, switch: PortSwitch) {
        if self.launch_busy() || self.client.is_some() || self.switch_busy() {
            return;
        }
        let Some(instance) = self.selected_instance().cloned() else {
            return;
        };
        let identity = match install_target(&instance) {
            Ok(identity) => identity,
            Err(error) => {
                self.open_notice(error, Some(WayOut::ProfileFolder(instance.home)));
                return;
            }
        };
        let target = self.version_of(&instance).cloned();
        let release = target
            .as_deref()
            .and_then(|tag| self.release_to_install(tag));
        let stop = native_mods::Cancellation::default();
        self.ui.sheets.switching.provider = Some(ProviderRunning {
            switch: switch.clone(),
            profile_name: instance.name.clone(),
            build_label: target.as_deref().map(build_name).unwrap_or_default(),
            progress: InstallProgress::Checking,
            eta: crate::eta::StageEta::default(),
            stop: stop.clone(),
            question: None,
        });
        self.open_sheet(Sheet::SwitchProvider);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let choose = || {
                if let Some(tag) = target
                    .as_deref()
                    .filter(|tag| backend::installed().iter().any(|local| local == tag))
                {
                    FirstBuild::installed(tag)
                } else if let Some(release) = &release {
                    FirstBuild::download(release)
                } else {
                    Err(backend::no_version_installed())
                }
            };
            // Each question is a fresh round trip to the window; a stopped switch asks nothing.
            let ask = |send: &dyn Fn(Sender<bool>) -> Msg| {
                if stop.is_stopped() {
                    return false;
                }
                let (answer, reply) = std::sync::mpsc::channel();
                tx.send(send(answer)).is_ok() && reply.recv().unwrap_or(false)
            };
            let outcome = switch_provider_v2(
                &instance,
                identity,
                &switch.mod_id,
                &switch.to_project_id,
                V2InstallOptions::default(),
                choose,
                V2ProviderSwitchHooks {
                    install: V2Hooks {
                        install: InstallHooks {
                            progress: &|stage| {
                                let _ = tx.send(Msg::ProviderSwitchProgress(stage));
                            },
                            continue_after_failure: &|_| false,
                            cancellation: None,
                        },
                        choose_provider: Some(&|need| {
                            let (answer, reply) = std::sync::mpsc::channel();
                            tx.send(Msg::ProviderChoice(Box::new(need.clone()), answer))
                                .ok()
                                .and_then(|()| reply.recv().ok().flatten())
                        }),
                        confirm_publisher_change: Some(&|question| {
                            ask(&|answer| {
                                Msg::PublisherChangeQuestion(Box::new(question.clone()), answer)
                            })
                        }),
                    },
                    approve_switch: &|preview| {
                        ask(&|answer| {
                            Msg::ProviderSwitchQuestion(Box::new(preview.clone()), answer)
                        })
                    },
                },
            );
            let outcome = match outcome {
                Ok(_) => Ok(true),
                Err(InstallError::Cancelled) => Ok(false),
                Err(error) => Err(error.to_string()),
            };
            let _ = tx.send(Msg::ProviderSwitched(instance.home, outcome));
        });
    }

    pub(super) fn cancel_provider_switch(&mut self) {
        let Some(running) = &mut self.ui.sheets.switching.provider else {
            return;
        };
        if running.progress != InstallProgress::Publishing {
            running.stop.stop();
            if let Some((_, reply)) = running.question.take() {
                let _ = reply.send(false);
            }
        }
    }

    pub(crate) fn provider_switch_progress(&mut self, stage: InstallProgress) {
        if let Some(running) = &mut self.ui.sheets.switching.provider {
            running.progress = stage;
            running.eta.update(
                crate::eta::install_stage_key(stage),
                crate::mods_page::install::stage_fraction(stage),
                Instant::now(),
            );
        }
    }

    pub(crate) fn provider_switch_question(
        &mut self,
        preview: Box<ProviderSwitchPreview>,
        reply: Sender<bool>,
    ) {
        if let Some(running) =
            self.ui.sheets.switching.provider.as_mut().filter(|run| {
                self.ui.sheet == Some(Sheet::SwitchProvider) && !run.stop.is_stopped()
            })
        {
            running.question = Some((preview, reply));
        } else {
            let _ = reply.send(false);
        }
    }

    pub(crate) fn provider_switch_finished(&mut self, home: &Path, result: Result<bool, String>) {
        let running = self.ui.sheets.switching.provider.take();
        let stopped = running.as_ref().is_some_and(|run| run.stop.is_stopped());
        let result = if result.is_err() && stopped {
            Ok(false)
        } else {
            result
        };
        self.refresh_installed();
        if self.selected_instance().is_none_or(|i| i.home != home) {
            return;
        }
        let visible = self.ui.sheet == Some(Sheet::SwitchProvider);
        match (result, running) {
            (Ok(true), Some(run)) => {
                let notice = trf(
                    "Switched to {repo} ({version}).",
                    &[
                        ("repo", &run.switch.to_repo),
                        ("version", &run.switch.to_version),
                    ],
                );
                self.refresh_mods_after_port_switch(Ok(notice), Some(run.switch.to_project_id));
                if visible {
                    self.close_sheet();
                }
            }
            (Ok(_), _) => {
                if visible {
                    self.close_sheet();
                }
            }
            (Err(error), _) if visible => {
                self.open_notice(error, Some(WayOut::ProfileFolder(home.into())))
            }
            (Err(error), _) => {
                self.refresh_mods_after_port_switch(Err(error), None);
            }
        }
    }

    pub(crate) fn on_provider_message(&mut self, msg: Msg) {
        match msg {
            Msg::ProviderSwitchProgress(stage) => self.provider_switch_progress(stage),
            Msg::ProviderSwitchQuestion(question, reply) => {
                self.provider_switch_question(question, reply)
            }
            Msg::ProviderSwitched(home, result) => self.provider_switch_finished(&home, result),
            Msg::ProviderChoice(need, reply) => self.provider_choice_question(need, reply),
            Msg::PublisherChangeQuestion(question, reply) => {
                self.publisher_change_question(question, reply)
            }
            Msg::ReceiptAcknowledged(home, result) => self.receipt_acknowledged(&home, result),
            _ => unreachable!("not a provider or publisher message"),
        }
    }

    pub(super) fn provider_switch_sheet(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        use crate::mods_page::install::{stage_fraction, stage_label};
        let running = self.ui.sheets.switching.provider.as_ref()?;
        let switch = &running.switch;
        let body = if let Some((preview, _)) = &running.question {
            let answer = |switch: bool| {
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some((_, reply)) = this
                        .ui
                        .sheets
                        .switching
                        .provider
                        .as_mut()
                        .and_then(|run| run.question.take())
                    {
                        let _ = reply.send(switch);
                    }
                    cx.notify();
                })
            };
            let profile_line = if running.build_label.is_empty() {
                trf("Profile: {profile}", &[("profile", &running.profile_name)])
            } else {
                trf(
                    "Profile: {profile} ({build})",
                    &[
                        ("profile", &running.profile_name),
                        ("build", &running.build_label),
                    ],
                )
            };
            let summary = trf(
                "This replaces {from_repo} ({from_version}) with {to_repo} ({to_version}). Both provide the {id} in-game ID; only one can be installed at a time.",
                &[
                    ("from_repo", &switch.from_repo),
                    ("from_version", &switch.from_version),
                    ("to_repo", &switch.to_repo),
                    ("to_version", &preview.to_version),
                    ("id", &preview.mod_id),
                ],
            );
            let world_lines: Vec<String> = preview
                .worlds
                .iter()
                .filter(|risk| !risk.mods.is_empty())
                .map(|risk| {
                    trf(
                        "{world}: content from {mod} may be affected.",
                        &[("world", &risk.name), ("mod", &switch.mod_name)],
                    )
                })
                .collect();
            let world_box = (!world_lines.is_empty()).then(|| {
                card()
                    .p(px(14.))
                    .gap(px(6.))
                    .children(world_lines.into_iter().map(muted))
            });
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(title(trf(
                            "Switch {mod} to {repo}?",
                            &[("mod", &switch.mod_name), ("repo", &switch.to_repo)],
                        )))
                        .child(muted(profile_line)),
                )
                .child(
                    div()
                        .text_size(px(tokens::BODY))
                        .text_color(rgb(palette().text))
                        .child(crate::selectable::selectable(summary)),
                )
                .children(world_box)
                .child(muted(trf(
                    "Your current port stays intact if you cancel. Future updates on this profile will follow {repo}.",
                    &[("repo", &switch.to_repo)],
                )))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            button("provider-switch-cancel", tr("Cancel"))
                                .on_click(answer(false)),
                        )
                        .child(
                            crate::frame::primary("provider-switch-confirm", tr("Switch"))
                                .on_click(answer(true)),
                        ),
                )
        } else {
            let publishing = running.progress == InstallProgress::Publishing;
            let cancel = button("provider-switch-cancel", tr("Cancel"));
            let cancel = if publishing {
                cancel.opacity(0.45).cursor_default().tab_stop(false)
            } else {
                cancel.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.cancel_provider_switch();
                    cx.notify();
                }))
            };
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(title(trf(
                    "Switching {mod} to {repo}",
                    &[("mod", &switch.mod_name), ("repo", &switch.to_repo)],
                )))
                .child(muted(if publishing {
                    stage_label(running.progress)
                } else {
                    crate::eta::with_eta(
                        stage_label(running.progress),
                        running.eta.remaining(Instant::now()),
                    )
                }))
                .children(
                    stage_fraction(running.progress)
                        .map(|f| crate::widgets::progress_bar(Some((f, f)), &self.theme)),
                )
                .child(muted(tr("Your current port stays intact if you cancel.")))
                .child(div().flex().justify_end().child(cancel))
        };
        Some(frame("provider-switch-sheet", 560., body, cx))
    }

    pub(crate) fn receipt_acknowledged(
        &mut self,
        home: &std::path::Path,
        result: Result<bool, String>,
    ) {
        self.ui.sheets.switching.ack = false;
        if self.ui.sheet != Some(Sheet::BuildChanges)
            || self.selected_instance().is_none_or(|i| i.home != home)
        {
            return;
        }
        match result {
            Ok(true) => {
                self.ui.sheets.switching.receipt = None;
                self.close_sheet();
            }
            Ok(false) => self.ui.sheets.error = Some(tr("These build changes are no longer current. Close this report and open Build changes again.").into()),
            Err(error) => self.ui.sheets.error = Some(error),
        }
    }
}

/// Whether a picker selection can be saved directly to `instances.json` without running a build
/// switch: the profile must be ordinary, have no pending transaction recovery or first-Play
/// bootstrap, and already be on `tag`.
fn can_fast_save_choice(
    instance: &instances::Instance,
    tag: &str,
    unplayed_target: Option<&str>,
    installed: bool,
) -> bool {
    if instance.kind == instances::Kind::Pack {
        return false;
    }
    if !matches!(
        sunrise_launcher::profile_txn::recovery_pending(&instance.home),
        Ok(false)
    ) {
        return false;
    }
    match managed_play::display_active(&instance.home) {
        Ok(Some(active)) => active.base() == tag,
        Ok(None) => unplayed_target == Some(tag) && installed,
        Err(_) => false,
    }
}

/// A partial write or an interrupted writer must not reserve the final report name.
fn publish_report(
    folder: &std::path::Path,
    file: &std::path::Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(file)
        .map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{can_fast_save_choice, publish_report};
    use std::io::{self, Write};
    use sunrise_launcher::instances::{Channel, Instance, Kind as InstanceKind};

    #[test]
    fn can_fast_save_choice_checks_kind_recovery_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut instance = Instance {
            id: "own-1".into(),
            name: "Own".into(),
            home: dir.path().to_path_buf(),
            work: dir.path().to_path_buf(),
            version: Some("dev-2026-11-24-abc".into()),
            channel: Channel::Dev,
            kind: InstanceKind::Own,
            group: None,
            icon: None,
            native_profile: Default::default(),
            last_played: None,
            played: Default::default(),
            launch: Default::default(),
            incarnation: None,
        };
        let tag = "dev-2026-11-24-abc";
        // Ordinary unplayed profile already targeting `tag` with `tag` installed -> fast path.
        assert!(can_fast_save_choice(&instance, tag, Some(tag), true));
        // Not installed yet or different target -> full switch.
        assert!(!can_fast_save_choice(&instance, tag, Some(tag), false));
        assert!(!can_fast_save_choice(
            &instance,
            tag,
            Some("dev-2026-11-20-old"),
            true
        ));
        // Pack profiles never take the fast path.
        instance.kind = InstanceKind::Pack;
        assert!(!can_fast_save_choice(&instance, tag, Some(tag), true));
        instance.kind = InstanceKind::Own;
        // Pending transaction/bootstrap record -> never take the fast path.
        let txn = dir.path().join("mod-browser/install-txn.json");
        std::fs::create_dir_all(txn.parent().unwrap()).unwrap();
        std::fs::write(&txn, "{}").unwrap();
        assert!(!can_fast_save_choice(&instance, tag, Some(tag), true));
    }

    #[test]
    fn report_save_can_retry_after_a_partial_write_and_never_clobbers() {
        let folder = tempfile::tempdir().unwrap();
        let report = folder.path().join("report.txt");
        let save =
            |text: &[u8]| publish_report(folder.path(), &report, |file| file.write_all(text));
        let error = publish_report(folder.path(), &report, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::new(io::ErrorKind::StorageFull, "fixture"))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert!(!report.exists());
        save(b"complete report\n").unwrap();
        assert_eq!(
            save(b"other").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&report).unwrap(), b"complete report\n");
    }
}
