//! The backend's messages, drained as they arrive (`Launcher::drain`, `wake.rs`).
use super::*;
use sunrise_launcher::managed_play::StartError;

/// A canceled Play is not a build failure. Otherwise keep the window open for the native
/// build diagnostic instead of restarting as soon as Play stops.
fn settle_managed_play_error(
    error: &StartError,
    active_play_build: bool,
    cancel_restart: impl FnOnce(),
) -> bool {
    let cancelled = matches!(error,
        StartError::Native(native_mods::Failed::Other(detail), _) | StartError::Other(detail)
            if detail == native_mods::CANCELLED);
    if !cancelled && (active_play_build || matches!(error, StartError::Native(..))) {
        cancel_restart();
    }
    cancelled
}

impl Launcher {
    pub(super) fn refresh_installed(&mut self) {
        (self.installed, self.known_nightly) =
            backend::play_installations(self.settings.selected.as_deref());
        self.refresh_open_storage_builds();
    }

    pub(crate) fn drain(&mut self, window: gpui::AnyWindowHandle, cx: &mut Context<Self>) {
        let mut changed = false;
        self.perf.tick();
        if self.bench.as_ref().is_some_and(perf::Bench::ready_for_idle) {
            // Finish one-time fallback font loading before the benchmark starts its idle sample.
            let _ = cx.update_window(window, |_, window, _| {
                language_page::warm_all_language_names(window)
            });
        } else if self.bench.is_none() && self.perf.resting(Duration::from_millis(250)) {
            let _ = cx.update_window(window, |_, window, _| {
                language_page::warm_language_name(window)
            });
        }
        #[cfg(feature = "frame-stats")]
        if self.bench.is_some() {
            self.perf.draws = cx.update_window(window, |_, w, _| perf::draws(w)).ok();
        }
        match self.bench.as_mut().map(|b| b.tick(&self.perf)) {
            // Through the page the frame draws (`navigate`, as a click on a tab does): `go` sets
            // the older `page` field, which nothing on screen reads, so the benchmark measured a
            // window that never changed.
            Some(perf::Step::Go(page)) => {
                let pages = crate::frame::Page::BENCH;
                self.navigate(pages[page % pages.len()]);
                changed = true;
            }
            Some(perf::Step::Scroll) => {
                // The build picker's list, the longest the launcher has, or Installed, scrolled
                // as a wheel does it rather than from inside a frame.
                match self.bench.as_ref().map(perf::Bench::list) {
                    Some(perf::List::Installed) => self.show_installed_mods(),
                    _ => self.open_versions(),
                }
                let launcher = cx.weak_entity();
                let _ = cx.update_window(window, |_, w, _| crate::sheets::bench_wheel(launcher, w));
                changed = true;
            }
            Some(perf::Step::Quit) => cx.quit(),
            Some(perf::Step::Wait) | None => {}
        }
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            match msg {
                Msg::Keychain(_)
                | Msg::Owner(_)
                | Msg::AccountHead(_, _, _, _)
                | Msg::SkinsProfile(_, _, _)
                | Msg::Picture(_, _)
                | Msg::LookSaved(_, _, _)
                | Msg::SkinsAdded(_, _, _)
                | Msg::SkinsListed(_, _)
                | Msg::LookRecorded(_, _, _)
                | Msg::SkinRemoved(_, _, _, _)
                | Msg::SkinFile(_, _, _)
                | Msg::SkinKept(_, _, _, _)
                | Msg::AccountProgress(_, _, _)
                | Msg::AccountStatus(_, _, _)
                | Msg::AccountRemoved(_, _, _) => self.on_account_message(msg),
                Msg::Releases(_)
                | Msg::Links(_)
                | Msg::Reused(_)
                | Msg::Progress(_, _)
                | Msg::Installed(_, _)
                | Msg::ConfirmUnreviewed(_, _) => self.on_install_message(msg),
                Msg::Playing(playing) => self.playing = playing,
                Msg::PlayStep(step) => {
                    if self.starting_client.is_some() {
                        self.play_step = Some(step);
                    }
                    if step == backend::PlayStep::SignIn && self.native_step.take().is_some() {
                        self.native_progress = None;
                        if let Some(run) = &mut self.ui.mods.build {
                            run.done();
                        }
                        if self.ui.sheet == Some(crate::sheets::Sheet::ModBuild) {
                            self.close_sheet();
                        }
                        self.refresh_installed();
                    }
                }
                Msg::ManagedBuild {
                    instance,
                    profile,
                    base,
                    mods,
                } => {
                    let selected = self.selected_instance().is_some_and(|i| i.id == instance);
                    let lines = self.build_lines(mods, selected);
                    self.native_for = Some(instance.clone());
                    self.native_failed_mods.clear();
                    self.ui.mods.build = Some(crate::mods::build_run::Run::start(
                        instance,
                        profile,
                        crate::sheets::build_name(&base),
                        lines,
                        true,
                    ));
                    self.open_sheet(crate::sheets::Sheet::ModBuild);
                }
                Msg::ManagedBuildLog(output) => {
                    if let Some(run) = self.ui.mods.build.as_mut().filter(|run| run.for_play) {
                        run.log = Some(output);
                    }
                }
                Msg::ManagedClient { instance, result } => {
                    self.native_cancellation = None;
                    self.native_step = None;
                    self.native_progress = None;
                    let active_play_build = self
                        .ui
                        .mods
                        .build
                        .as_ref()
                        .is_some_and(|run| run.for_play && run.running());
                    let cancelled = result.as_ref().as_ref().is_err_and(|error| {
                        settle_managed_play_error(error, active_play_build, || {
                            self.cancel_launcher_restart()
                        })
                    });
                    let mut captured_build_error = false;
                    if let (Some(run), Err(error)) = (&mut self.ui.mods.build, result.as_ref()) {
                        if active_play_build {
                            match error {
                                _ if cancelled => run.cancelled(),
                                StartError::Native(error, build) => {
                                    run.failed_build = build.clone();
                                    run.native_failed(error, &[]);
                                }
                                StartError::Other(error) => {
                                    run.stopped(
                                        tr("The build could not finish. No game was started.")
                                            .into(),
                                        error.clone(),
                                    );
                                    captured_build_error = true;
                                }
                                StartError::Journal(error) => {
                                    run.stopped(error.summary().into(), error.details())
                                }
                                _ => run.stopped(
                                    tr("The build could not be published.").into(),
                                    String::new(),
                                ),
                            }
                        }
                    }
                    if !self.ui.mods.build.as_ref().is_some_and(|run| {
                        run.for_play
                            && matches!(run.end, Some(crate::mods::build_run::End::Stopped(_)))
                    }) {
                        self.play_retry = None;
                    }
                    self.starting_client = None;
                    self.play_step = None;
                    self.accounts = Accounts::load();
                    self.load_auth_statuses(false);
                    match *result {
                        Ok(game) => {
                            self.client_error = game.notice;
                            self.play_registry_notices = game.registry_notices;
                            for notice in &self.play_registry_notices {
                                console_page::keep_line(
                                    &mut self.prepare_log,
                                    &mut self.log_revision,
                                    notice.clone(),
                                );
                            }
                            if game.owner {
                                self.owner = Some(true);
                            }
                            self.refresh_installed();
                            self.client_home = Some(game.instance.home.clone());
                            self.client_run = Some((game.instance, Instant::now()));
                            self.client = Some(backend::monitor_game(
                                game.child,
                                self.tx.clone(),
                                game.runtime,
                            ));
                            if self.play_registry_notices.is_empty() {
                                self.step_aside(cx);
                            }
                        }
                        Err(StartError::Journal(problem)) => {
                            self.play_install_recovery = Some((instance.home, problem.details()));
                        }
                        Err(StartError::Account(refused)) => {
                            if refused.keyring.is_none() {
                                self.owner = Some(false);
                                self.sign_in_screen.open = true;
                            }
                            self.client_error = Some(refused.in_window());
                        }
                        Err(_) if cancelled => (),
                        Err(StartError::Native(error, _)) => {
                            self.native_for = Some(instance.id);
                            if !active_play_build {
                                self.native_failed(error.to_string(), &[]);
                            }
                        }
                        Err(StartError::Hook(error)) => self.client_error = Some(error.to_string()),
                        Err(StartError::Other(_)) if captured_build_error => (),
                        Err(StartError::UnknownSpawn(error) | StartError::Other(error)) => {
                            self.client_error = Some(error);
                        }
                    }
                }
                Msg::PlayRecorded => self.reload_instances(),
                Msg::GameExitSettled => self.finishing_game -= 1,
                Msg::PlayAdmission(home, result) => {
                    let Some(intent) = self.play_admission.take() else {
                        continue;
                    };
                    if self
                        .selected_instance()
                        .is_none_or(|i| i.home != home || !intent.matches_profile(i))
                    {
                        continue;
                    }
                    self.play_install_recovery = None;
                    match result {
                        Ok(()) => self.play_after_journal_check(intent, &home),
                        Err(problem) => {
                            self.play_install_recovery = Some((home, problem.details()))
                        }
                    }
                }
                Msg::SwitchProgress(_) | Msg::SwitchQuestion(_, _) | Msg::Switched(_, _) => {
                    self.on_switch_message(msg)
                }
                Msg::ProviderSwitchProgress(_)
                | Msg::ProviderSwitchQuestion(_, _)
                | Msg::ProviderSwitched(_, _)
                | Msg::ProviderChoice(_, _)
                | Msg::PublisherChangeQuestion(_, _)
                | Msg::ReceiptAcknowledged(_, _) => self.on_provider_message(msg),
                }
                Msg::Removed(_)
                | Msg::BuildInventory(_, _)
                | Msg::Storage(_)
                | Msg::Imported(_)
                | Msg::Pruned(_) => self.on_storage_message(msg),
                Msg::Updates(_)
                | Msg::LauncherChecked(_)
                | Msg::LauncherProgress(_)
                | Msg::LauncherUpdated(_) => self.on_update_message(msg),
                }
                Msg::Exit(_, _) => self.on_game_exit_message(msg, cx),
            }
        }
        changed |= self.servers_tick();
        changed |= self.mods_tick();
        changed |= self.mods_drain();
        changed |= self.github_tick();
        changed |= self.toast_tick();
        changed |= self.artwork_tick();
        if let Some(sign_in) = &self.sign_in {
            if let Some(state) = sign_in_change(&mut self.sign_in_state, sign_in.state()) {
                if let SignInState::Done(Ok(ref account)) = state {
                    self.auth_busy.remove(&account.uuid);
                    self.sign_in_screen.target = None;
                    self.accounts = Accounts::load();
                    self.load_profile_heads();
                    self.load_auth_statuses(false);
                    // Sign-in refuses accounts that don't own the game.
                    self.owner = Some(true);
                    self.sign_in_screen.open = false;
                    self.client_error = None;
                    if !self.settings.owner_notice_seen {
                        self.settings.owner_notice_seen = true;
                        self.save_settings();
                    }
                    if let Some(mut intent) = self.ui.after_sign_in.take().filter(|asked| {
                        account_chip::plays_after_sign_in(
                            Some(asked.instance.clone()),
                            self.selected_instance().map(|i| i.id.as_str()),
                        )
                    }) {
                        intent.account = Some(account.uuid.clone());
                        self.resume_play(intent);
                    }
                }
                changed = true;
            }
        }
        if !self.checking_updates
            && self
                .checked
                .is_some_and(|at| at.elapsed() > backend::CHECK_EVERY)
        {
            self.check_updates(UpdateCheck::Scheduled);
            changed = true;
        }
        self.restart_launcher_if_ready(cx);
        // A mod rewritten where it lies, seen while the page sits still.
        changed |= self.mods_check_due(std::time::Instant::now());
        // Repaint once a second while a loading sheet or status-strip stage is active so ETAs
        // tick down and stall/expiry hides take effect even if the worker goes quiet.
        if !changed && (self.switch_busy() || self.registry_stage().is_some()) {
            static LAST_ETA_TICK: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
            let now = Instant::now();
            if let Ok(mut last) = LAST_ETA_TICK.lock() {
                if last.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1)) {
                    *last = Some(now);
                    changed = true;
                }
            }
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_managed_play_build_failure_disarms_the_update_but_a_cancellation_does_not() {
        let native = StartError::Native(native_mods::Failed::Other("compiler failed".into()), None);
        let publication = StartError::Other("could not write the profile pointer".into());
        for (error, build_running) in [(&native, false), (&publication, true)] {
            let mut restart_armed = true;
            assert!(!settle_managed_play_error(error, build_running, || {
                restart_armed = false
            }));
            assert!(!restart_armed);
        }
        assert!(!settle_managed_play_error(&publication, false, || panic!(
            "no active Play build"
        )));

        for error in [
            StartError::Native(
                native_mods::Failed::Other(native_mods::CANCELLED.into()),
                None,
            ),
            StartError::Other(native_mods::CANCELLED.into()),
        ] {
            assert!(settle_managed_play_error(&error, true, || panic!(
                "Play was canceled"
            )));
        }
    }
}
