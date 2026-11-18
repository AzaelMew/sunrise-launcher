//! Installing and removing builds, preparing and starting the game, and the accounts' sign-in.
use super::*;

impl Launcher {
    /// After a crash with no report: when the game's output ends while its Vulkan renderer
    /// started, a native crash in the graphics driver (#746), switches the game to
    /// `backend::vulkan_crash_fallback` (DirectX 12 on Windows, OpenGL on Linux; only where the
    /// player left the graphics API on Auto). The API's label when it did.
    pub(crate) fn switched_from_vulkan(&self) -> Option<&'static str> {
        let (api, label) = backend::vulkan_crash_fallback(std::env::consts::OS)?;
        if !backend::vulkan_start_crashed(&self.prepare_log) {
            return None;
        }
        // The GPU is the same for every profile: the game home's setting and the crashed run's.
        let home = backend::game_home();
        let mut homes = vec![home.clone()];
        homes.extend(self.client_home.clone().filter(|h| *h != home));
        let mut switched = false;
        for home in homes {
            match backend::switch_from_auto(&home, api) {
                Ok(changed) => switched |= changed,
                Err(e) => eprintln!("Sunrise launcher: could not switch to {label}: {e}"),
            }
        }
        switched.then_some(label)
    }

    /// The version Play starts: the selected instance's own, else the launcher's.
    pub(crate) fn selected(&self) -> Option<&String> {
        self.selected_instance()
            .and_then(|i| self.version_of(i))
            .or_else(|| self.launcher_version())
    }

    /// The launcher's version: what instances without one of their own play.
    pub(crate) fn launcher_version(&self) -> Option<&String> {
        self.settings
            .selected
            .as_ref()
            .filter(|s| self.installed.contains(s))
    }

    /// Installs `release` for an instance made or edited to play it, after the install running
    /// now if there is one.
    pub(crate) fn install_for_instance(&mut self, release: Release) {
        match &self.installing {
            Some((now, _)) if *now == release.tag => {}
            Some(_) => self.install_next = Some(release),
            None => {
                self.instance_install = Some(release.tag.clone());
                self.install(&release);
            }
        }
    }

    pub(crate) fn install(&mut self, release: &Release) {
        if release.asset.is_none() || self.removing.contains(&release.tag) {
            return;
        }
        let (release, tx) = (release.clone(), self.tx.clone());
        let tag = release.tag.clone();
        self.installing = Some((tag.clone(), 0.0));
        self.install_started = Some(Instant::now());
        self.bar = (0., 0.);
        self.install_error = None;
        std::thread::spawn(move || {
            let progress = |p| {
                let _ = tx.send(Msg::Progress(tag.clone(), p));
            };
            let result = backend::install(&release, progress);
            let _ = tx.send(Msg::Installed(tag, result));
        });
    }

    /// Deletes an installed version on a background thread: a version is thousands of files and
    /// the window must keep drawing meanwhile. `Msg::Removed` ends it.
    pub(crate) fn remove(&mut self, tag: &str) {
        self.confirm_remove = None;
        let installing = self.installing.as_ref().map(|(t, _)| t.as_str());
        if !removable(
            &self.installed,
            installing,
            &self.removing,
            &self.instances_using(tag),
            tag,
        ) {
            return;
        }
        self.install_error = None;
        self.removing.push(tag.to_string());
        self.installed.retain(|t| t != tag);
        self.settings.selected =
            selection_after_removal(&self.installed, self.settings.selected.take());
        let (tag, tx) = (tag.to_string(), self.tx.clone());
        std::thread::spawn(move || {
            let result = backend::remove_version(&tag);
            let _ = tx.send(Msg::Removed(tag, result));
        });
    }


    /// Prepares the selected version, its Minecraft files from the player's install or, for an
    /// owner who asks, from Mojang.
    pub(crate) fn prepare(&mut self, from: backend::MinecraftFrom) {
        let Some(tag) = self.selected().cloned() else {
            return;
        };
        sunrise_launcher::mojang::CANCEL.store(false, Ordering::Relaxed);
        let minecraft = self
            .settings
            .minecraft_dir
            .clone()
            .unwrap_or_else(|| sunrise_launcher::preparer::minecraft_dir(None));
        self.prepare_log.clear();
        self.prepare_error = None;
        self.prepare_log
            .push_back("Checking Java and your Minecraft install".into());
        let tx = self.tx.clone();
        // The checks may download the server jar, so they run on the thread too.
        std::thread::spawn(move || {
            let log = |line| {
                let _ = tx.send(Msg::Line(Job::Prepare, line));
            };
            let lock = match backend::prepare_lock(&tag) {
                Ok(lock) => lock,
                Err(e) => {
                    let _ = tx.send(Msg::Exit(Job::Prepare, Err(e)));
                    return;
                }
            };
            let result = backend::preparer(&tag, &minecraft, from, &log)
                .inspect(|(_, minecraft)| backend::sounds(&tag, minecraft, &log))
                .map(|(p, _)| {
                    backend::set_prepare_step(backend::PrepareStep::Prepare);
                    p
                })
                .and_then(|cmd| backend::spawn_holding(Job::Prepare, cmd, tx.clone(), Some(lock)));
            // A running preparer sends its own Exit when it ends.
            if let Err(e) = result {
                let _ = tx.send(Msg::Exit(Job::Prepare, Err(e)));
            }
        });
        self.preparing = true;
        self.prepare_started = Some(Instant::now());
        self.download_seen = None;
    }

    pub(crate) fn play(&mut self) {
        let java = self
            .selected_instance()
            .filter(|i| i.java.is_some())
            .cloned();
        if let Some(instance) = java {
            // Mojang's game: no Sunrise version or preparation, only no game already running.
            // It joins a server; a friend's code and opening a world are Sunrise's.
            self.joining = None;
            let server = self
                .quick_play
                .take()
                .filter(|(flag, _)| *flag == "--server")
                .map(|(_, ip)| ip);
            if self.client.is_some() || self.starting_client.is_some() {
                self.client_error = Some(tr("The game is already running.").into());
                return;
            }
            self.starting_pack = None;
            self.start_java(instance, server);
            return;
        }
        // A press that starts nothing says why, where Play's errors show: the button can look
        // ready when it is not (a version prepared elsewhere and since removed, a debug preview).
        let Some(tag) = self.selected().cloned() else {
            self.client_error = Some(tr("Install a Sunrise game build first.").into());
            return;
        };
        if let Some(why) = self.why_not_now(&tag) {
            self.client_error = Some(why);
            return;
        }
        self.starting_pack = None;
        self.start_client(tag, self.selected_instance().map(|i| i.id.clone()));
    }

    /// Starts a Java instance's Minecraft, joining `server` if given: its files and Java first
    /// (downloaded the first time, their lines in the log), then `drain` starts the game when
    /// the command arrives.
    pub(crate) fn start_java(&mut self, instance: instances::Instance, server: Option<String>) {
        self.client_error = None;
        self.crash = None;
        self.prepare_log.clear();
        self.client_started =
            Some(std::time::SystemTime::now() - std::time::Duration::from_secs(2));
        self.starting_client = Some(instance.id.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let log = |line: String| {
                let _ = tx.send(Msg::Line(Job::Client, line));
            };
            let cmd = backend::java_game(&instance, server.as_deref(), &log).map(|mut cmd| {
                let vars = instance.vars(cmd.get_program());
                cmd.envs(vars.iter().map(|(k, v)| (k, v)));
                (sunrise_launcher::launch::wrap(cmd, &instance.launch), vars)
            });
            let _ = tx.send(Msg::JavaGame(instance.id, instance.home, cmd));
        });
    }

    /// Why `tag` cannot start now, as the player reads it; None when it can ([`Self::can_start`]).
    fn why_not_now(&self, tag: &str) -> Option<String> {
        if self.can_start(tag) {
            None
        } else if self.client.is_some() || self.starting_client.is_some() {
            Some(tr("The game is already running.").into())
        } else if self.preparing {
            Some(tr("Wait for the preparation to finish, then press Play.").into())
        } else {
            Some(trf(
                "{version} isn't prepared yet: choose Prepare in Play's menu.",
                &[("version", &self.version_label(tag))],
            ))
        }
    }

    /// Whether `tag` can start now ([`starts_now`]): prepared, no preparation under way, no game
    /// running or starting.
    pub(crate) fn can_start(&self, tag: &str) -> bool {
        let running = self.client.is_some() || self.starting_client.is_some();
        starts_now(backend::is_prepared(tag), self.preparing, running)
    }

    /// Starts the game: the session first (a network round trip when the token needs
    /// renewing), then `drain` starts the process when it arrives.
    pub(crate) fn start_client(&mut self, tag: String, instance_id: Option<String>) {
        self.client_error = None;
        self.crash = None;
        self.prepare_log.clear();
        // A little earlier than the spawn: file times can be coarse.
        self.client_started =
            Some(std::time::SystemTime::now() - std::time::Duration::from_secs(2));
        // What the resources lack (built-in packs, panorama, sounds) and the session (a network
        // round trip when the token needs renewing) come first; `drain` starts the game when the
        // session arrives.
        self.starting_client = Some(tag.clone());
        self.starting_instance = instance_id.clone();
        self.play_pressed = Some(Instant::now());
        let tx = self.tx.clone();
        let auto = self
            .instances
            .list
            .iter()
            .find(|i| Some(&i.id) == instance_id.as_ref())
            .filter(|_| self.starting_pack.is_none())
            .map(|i| mods_page::AutoTarget {
                instance: i.clone(),
                game: backend::played_version_id_by(Some(&tag)),
            });
        let mods_tx = self.mods.tx.clone();
        let auto_enabled = self.settings.auto_updates_mods();
        std::thread::spawn(move || {
            // One line per step: a terminal run shows where Play stops (#12179).
            backend::play_step("checking the game's assets");
            backend::complete_assets(&tag);
            if let Some(auto) = auto {
                backend::play_step("updating registry mods");
                let lines = mods_page::auto_update(&[auto], auto_enabled);
                if !lines.is_empty() {
                    let _ = mods_tx.send(mods_page::Event::AutoUpdated(lines));
                }
            }
            backend::play_step("reading the sign-in");
            let session = if sunrise_launcher::developer_offline::enabled(&tag) {
                Ok(None)
            } else {
                account::session_for_game(&backend::game_home()).map(Some)
            };
            backend::play_step("starting the game");
            let _ = tx.send(Msg::Session(session));
        });
    }

    /// The prepared game files of the selected version (else the first installed): where the
    /// default skins are.
    pub(crate) fn game_resources(&self) -> std::path::PathBuf {
        self.selected()
            .or_else(|| self.installed.first())
            .map(|tag| backend::resources(tag))
            .unwrap_or_default()
    }

    pub(crate) fn load_profile_heads(&mut self) {
        let resources = self.game_resources();
        self.profile_heads
            .request(&self.accounts.accounts, resources, &self.tx);
    }

    pub(crate) fn begin_auth_job(&mut self, uuid: &str) -> Option<u64> {
        if self.auth_busy.contains_key(uuid) {
            return None;
        }
        self.auth_next += 1;
        self.auth_busy.insert(uuid.into(), self.auth_next);
        self.auth_progress
            .insert(uuid.into(), account::session::Phase::Queued);
        Some(self.auth_next)
    }

    /// Only workers read the keychain or contact authentication services. Cached public status
    /// keeps drawing independent of keychain prompts, network timeouts and refresh operations.
    pub(crate) fn load_auth_statuses(&mut self, renew_due: bool) {
        self.auth_checked = Some(Instant::now());
        for account in self.accounts.accounts.clone() {
            let Some(generation) = self.begin_auth_job(&account.uuid) else {
                continue;
            };
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let progress = |phase| {
                    let _ = tx.send(Msg::AccountProgress(
                        account.uuid.clone(),
                        generation,
                        phase,
                    ));
                };
                progress(account::session::Phase::Keychain);
                let store = account::store();
                let mut status =
                    account::session::status_with_progress(store, &account.uuid, &progress);
                if renew_due && status.state == account::session::State::NeedsRefresh {
                    if let Ok(renewed) =
                        account::session::refresh_with_progress(&account.uuid, &progress)
                    {
                        status = renewed;
                    }
                }
                let _ = tx.send(Msg::AccountStatus(account.uuid, generation, status));
            });
        }
    }

    pub(crate) fn refresh_account(&mut self, uuid: String) {
        let Some(generation) = self.begin_auth_job(&uuid) else {
            return;
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let progress = |phase| {
                let _ = tx.send(Msg::AccountProgress(uuid.clone(), generation, phase));
            };
            progress(account::session::Phase::Keychain);
            if let Ok(status) = account::session::refresh_with_progress(&uuid, &progress) {
                let _ = tx.send(Msg::AccountStatus(uuid, generation, status));
            } else {
                // It may have been removed through the CLI while this window was open.
                let _ = tx.send(Msg::AccountRemoved(
                    uuid,
                    generation,
                    Ok(Accounts::load().has_owner(account::store())),
                ));
            }
        });
    }

    /// Generic Add account and targeted re-sign-in share the launcher-owned device-code flow.
    pub(crate) fn start_sign_in(&mut self) {
        self.begin_sign_in(None);
    }

    pub(crate) fn re_sign_in(&mut self, uuid: &str) {
        if let Some(account) = self.accounts.find(uuid).cloned() {
            self.begin_sign_in(Some(account));
        }
    }

    pub(crate) fn begin_sign_in(&mut self, target: Option<account::Account>) {
        self.cancel_sign_in();
        // The code page offers a private window: find the default browser meanwhile.
        std::thread::spawn(|| {
            sunrise_launcher::private_window::detected();
        });
        self.sign_in = Some(match &target {
            Some(account) => SignIn::start_for(account.clone()),
            None => SignIn::start(),
        });
        self.sign_in_target = target;
        self.sign_in_state = Some(SignInState::Starting);
    }

    pub(crate) fn cancel_sign_in(&mut self) {
        if self.alt.after_sign_in.take().is_some() {
            self.quick_play = None;
            self.joining = None;
        }
        if let Some(old) = self.sign_in.take() {
            old.cancel();
        }
        self.sign_in_state = None;
        self.sign_in_target = None;
    }

    pub(crate) fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = paths.await {
                if let Some(path) = paths.pop() {
                    let _ = this.update(cx, |this, cx| {
                        this.settings.minecraft_dir = Some(path);
                        this.save_settings();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}
