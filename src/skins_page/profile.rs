//! Opening the page, the offline mode, reading the profile, and the answers that come back.
use super::*;

impl Launcher {
    /// The prepared game files the default skins are read from, and the default skins they
    /// have: the version the selected profile plays when it has them, else the first installed
    /// version that does. None when no game is prepared: nothing is looked for relative to the
    /// working folder. Looked for once for the build played and the builds installed, not at
    /// every drawing.
    pub(super) fn skin_resources_found(&self) -> Option<(PathBuf, Vec<&'static str>)> {
        let selected = self.selected().cloned();
        if let Some((played, installed, found)) = &*self.skins.resources.borrow() {
            if *played == selected && *installed == self.installed {
                return found.clone();
            }
        }
        let has = |resources: &Path| default_skin_path(resources, "steve", false).is_file();
        let found = selected
            .iter()
            .chain(self.installed.iter())
            .map(|tag| backend::resources(tag))
            .find(|resources| has(resources))
            .map(|resources| {
                let defaults = DEFAULT_SKINS
                    .into_iter()
                    .filter(|name| default_skin_path(&resources, name, false).is_file())
                    .collect();
                (resources, defaults)
            });
        *self.skins.resources.borrow_mut() =
            Some((selected, self.installed.clone(), found.clone()));
        found
    }

    pub(super) fn skin_resources(&self) -> Option<PathBuf> {
        self.skin_resources_found().map(|(resources, _)| resources)
    }

    /// Whether the page is in the offline developer view now: the mode in force, not the one
    /// the page was opened in, since another profile can be chosen while it is open. The
    /// environment is checked first, so a launcher not started for offline play reads no files
    /// for it. In one that was, the build the selected profile plays is asked
    /// (`developer_offline::enabled`) again only when that build, or a file `enabled` reads for
    /// it (`developer_offline::inputs`), changed.
    ///
    /// Each answer is also stored for the page's queued texture jobs, which read it as they
    /// start, so they go by the same answer as the page.
    pub(crate) fn skins_offline(&self) -> bool {
        let offline = self.skins_offline_worked_out();
        self.skins.offline_flag.store(offline, Ordering::Relaxed);
        offline
    }

    pub(super) fn skins_offline_worked_out(&self) -> bool {
        #[cfg(test)]
        if let Some(offline) = self.skins.offline_for_test {
            return offline;
        }
        if !sunrise_launcher::developer_offline::requested() {
            return false;
        }
        self.offline_cached(sunrise_launcher::developer_offline::enabled)
    }

    /// The offline developer mode for the build played, as `enabled` says, asked only when what
    /// it depends on ([`OfflineKey`]) changed since it was last asked.
    ///
    /// At a drawing this costs four file times, no reads: the base build named by the build's
    /// lineage records is read again only when those records change.
    pub(super) fn offline_cached(&self, enabled: impl FnOnce(&str) -> bool) -> bool {
        use sunrise_launcher::developer_offline as offline_play;
        let played = self.selected().cloned();
        let Some(tag) = played.clone() else {
            return false;
        };
        let own: Vec<_> = offline_play::records(&tag)
            .into_iter()
            .map(offline_play::stamp)
            .collect();
        let known_base = self.skins.offline.borrow().as_ref().and_then(|(key, _)| {
            (key.played == played && key.own == own).then(|| key.base.clone())
        });
        let base = known_base.unwrap_or_else(|| offline_play::base(&tag));
        let base_files = offline_play::base_records(&base)
            .into_iter()
            .map(offline_play::stamp)
            .collect();
        let key = OfflineKey {
            played,
            own,
            base,
            base_files,
        };
        if let Some((known, offline)) = &*self.skins.offline.borrow() {
            if *known == key {
                return *offline;
            }
        }
        let offline = enabled(&tag);
        *self.skins.offline.borrow_mut() = Some((key, offline));
        offline
    }

    /// The account whose look the page shows: the one that plays (the selected one, else the
    /// first), as the game picks it.
    pub(super) fn look_account(&self) -> Option<account::Account> {
        self.accounts
            .selected()
            .or_else(|| self.accounts.accounts.first())
            .cloned()
    }

    /// The page opened, another account plays, or the offline developer mode changed: what
    /// the account wears is read again, and what couldn't be had before is asked for again.
    /// The files the last import couldn't add stay listed until the next import.
    pub(crate) fn skins_open(&mut self) {
        // Looked for again: a game may have been prepared since.
        *self.skins.resources.get_mut() = None;
        *self.skins.offline.get_mut() = None;
        let offline = self.skins_offline();
        self.skins.opened_offline = Some(offline);
        let Some(account) = self.look_account() else {
            return;
        };
        let accounts = &self.accounts.accounts;
        self.skins.show(&account.uuid, |uuid| {
            accounts.iter().any(|a| a.uuid == uuid)
        });
        self.skins
            .files
            .get_mut()
            .retain(|_, file| !matches!(file, File::Missing));
        if offline {
            // A reading under way is dropped: its answer starts nothing.
            self.skins.reading = None;
            return;
        }
        if std::mem::take(&mut self.skins.heads_stale) {
            self.load_profile_heads();
        }
        self.skins_list(&account.uuid);
        if self.skins.saving.is_none() {
            self.profile_read(&account);
        }
        if let Some((url, _)) = &account.skin {
            self.picture_ask(url.clone());
        }
    }

    /// Lists `uuid`'s saved skins on the writes queue, after the changes queued before it.
    pub(super) fn skins_list(&mut self, uuid: &str) {
        let (tx, account, id) = (self.tx.clone(), uuid.to_owned(), request_id());
        let job = Box::new(move || {
            answer(
                &tx,
                || Msg::SkinsListed(account.clone(), listing(&account, id)),
                || Msg::SkinsListed(account.clone(), (id, Err(stopped()))),
            )
        });
        if self.skins.writes.push("skins-index", job).is_err() {
            if let Some(page) = self.skins.page_mut(uuid) {
                page.listing((
                    id,
                    Err(tr(
                        "The launcher is busy with your saved skins. Open Skins again in a moment.",
                    )
                    .to_string()),
                ));
            }
        }
    }

    pub(crate) fn on_account_message(&mut self, msg: Msg) {
        match msg {
            Msg::Keychain(said) => self.keychain = said,
            Msg::Owner(owns) => self.owner = Some(owns),
            Msg::AccountHead(uuid, url, face, failed) => {
                self.profile_heads.loaded(&uuid, &url, face, failed)
            }
            Msg::SkinsProfile(uuid, id, result) => self.skins_profile(uuid, id, result),
            Msg::Picture(url, png) => self.skins_picture(url, png),
            Msg::LookSaved(uuid, id, outcome) => self.look_saved(uuid, id, *outcome),
            Msg::SkinsAdded(uuid, added, listing) => self.skins_added(uuid, added, listing),
            Msg::SkinsListed(uuid, listing) => self.skins_listed(uuid, listing),
            Msg::LookRecorded(uuid, record, recorded) => self.look_recorded(uuid, record, recorded),
            Msg::SkinRemoved(uuid, id, removed, listing) => {
                self.skin_removed(uuid, id, removed, listing)
            }
            Msg::SkinFile(key, id, png) => self.skins_file(key, id, png),
            Msg::SkinKept(uuid, url, kept, listing) => self.skins_kept(uuid, url, kept, listing),
            Msg::AccountProgress(uuid, generation, phase) => {
                if self.auth_busy.get(&uuid) == Some(&generation) {
                    self.auth_progress.insert(uuid, phase);
                }
            }
            Msg::AccountStatus(uuid, generation, status) => {
                if self.auth_busy.get(&uuid) != Some(&generation) {
                    return;
                }
                self.auth_busy.remove(&uuid);
                self.auth_progress.remove(&uuid);
                if self.accounts.find(&uuid).is_some() {
                    if status.state == account::session::State::Ready {
                        self.owner = Some(true);
                    }
                    self.auth_status.insert(uuid, status);
                    // A refresh may have saved a new name or skin: show it now.
                    let saved = Accounts::load();
                    if saved != self.accounts {
                        self.accounts = saved;
                        self.load_profile_heads();
                    }
                }
            }
            Msg::AccountRemoved(uuid, generation, result) => {
                if self.auth_busy.get(&uuid) != Some(&generation) {
                    return;
                }
                self.auth_busy.remove(&uuid);
                self.auth_progress.remove(&uuid);
                match result {
                    Ok(owns) => {
                        self.accounts = Accounts::load();
                        self.auth_status.remove(&uuid);
                        self.owner = Some(owns);
                    }
                    Err(e) => self.sign_in_state = Some(SignInState::Done(Err(e))),
                }
            }
            _ => unreachable!("not an account or skins message"),
        }
    }

    /// The saved skins were listed (`Msg::SkinsListed`).
    pub(crate) fn skins_listed(&mut self, uuid: String, listed: skin_library::Listing) {
        if let Some(page) = self.skins.page_mut(&uuid) {
            page.listing(listed);
        }
    }

    /// Reads what the shown page's account wears, on a thread of its own: one reading at a
    /// time for a page, so asking again while one is under way (Try again, the page opened
    /// again) waits for that one's answer instead of starting another.
    pub(super) fn profile_read(&mut self, account: &account::Account) {
        if self.skins_offline()
            || self.skins.uuid.as_deref() != Some(&account.uuid)
            || self.skins.reading.is_some()
        {
            return;
        }
        let id = self.skins.read_asked();
        let (account, tx) = (account.clone(), self.tx.clone());
        let other =
            tr("The Minecraft services answered with another account's profile.").to_string();
        let started = spawn("skins-profile", move || {
            let uuid = account.uuid.clone();
            answer(
                &tx,
                || {
                    let ep = account::Endpoints::live();
                    let result = account::token_for(
                        &ep,
                        account::client_id().as_deref(),
                        account::store(),
                        &account,
                    )
                    .and_then(|token| capes::read(&ep, &token))
                    .and_then(|profile| {
                        profile
                            .is_for(&account.uuid)
                            .then_some(profile)
                            .ok_or(other)
                    });
                    Msg::SkinsProfile(account.uuid.clone(), id, result)
                },
                || Msg::SkinsProfile(uuid, id, Err(stopped())),
            )
        });
        if !started {
            self.skins.reading = None;
            if !matches!(self.skins.profile, Some(Ok(_))) {
                self.skins.profile = Some(Err(tr(
                    "The launcher couldn't start looking up your profile. Try again.",
                )
                .to_string()));
            }
        }
    }

    /// Fetches a picture from Mojang's texture server, on the texture queue; one it won't take
    /// counts as one that couldn't be had (asked for again at the next reading).
    pub(super) fn picture_ask(&mut self, url: String) {
        let offline = self.skins_offline();
        if !self.skins.ask_picture(&url, offline) {
            return;
        }
        let job = texture_job(
            self.tx.clone(),
            url.clone(),
            self.skins.offline_flag.clone(),
            capes::picture,
        );
        if self.skins.net.push("skins-textures", job).is_err() {
            self.skins_picture(url, None);
        }
    }

    /// A reading answered (`Msg::SkinsProfile`): taken in by the page that asked for it, shown
    /// or not, and what it confirmed is recorded; only the shown page, online, asks for the
    /// pictures it names.
    pub(crate) fn skins_profile(&mut self, uuid: String, id: u64, result: Result<Profile, String>) {
        let offline = self.skins_offline();
        let shown = self.skins.uuid.as_deref() == Some(&uuid);
        let Some(page) = self.skins.page_mut(&uuid) else {
            return;
        };
        if !page.read_answered(&uuid, id, result, offline) {
            return;
        }
        self.record_confirmed(&uuid);
        if shown {
            self.after_profile();
        }
    }

    /// Records in accounts.json what profiles read after a Save showed of the changes it left
    /// unconfirmed ([`Skins::confirmed`]): only those fields, on the writes queue, and the
    /// page's next Save waits until they are written, so an account's records stay in the order
    /// of its Saves. One the queue won't take stays to record, and is tried again.
    pub(super) fn record_confirmed(&mut self, uuid: &str) {
        let Some(page) = self.skins.page_mut(uuid) else {
            return;
        };
        if page.confirmed == look::Record::default() {
            return;
        }
        let record = page.confirmed.clone();
        let (tx, account, job_record) = (self.tx.clone(), uuid.to_owned(), record.clone());
        let job = Box::new(move || {
            answer(
                &tx,
                || {
                    let saved = account::Accounts::save_look(
                        &account,
                        job_record.skin.clone(),
                        job_record.cape.clone(),
                    );
                    Msg::LookRecorded(account.clone(), job_record.clone(), saved)
                },
                || Msg::LookRecorded(account.clone(), job_record.clone(), Err(stopped())),
            )
        });
        let queued = self.skins.writes.push("skins-index", job).is_ok();
        let Some(page) = self.skins.page_mut(uuid) else {
            return;
        };
        if queued {
            page.confirmed = look::Record::default();
            page.recording += 1;
            self.skins.disk_writes += 1;
        }
        // Not queued: it stays to record, tried again at the next reading or picture.
    }

    /// What a later profile confirmed was recorded, or couldn't be (`Msg::LookRecorded`). Only
    /// once it is in accounts.json does the account chip show it, so the chip never shows a
    /// look the launcher hasn't kept.
    pub(crate) fn look_recorded(
        &mut self,
        uuid: String,
        record: look::Record,
        recorded: Result<(), String>,
    ) {
        self.skins.disk_writes = self.skins.disk_writes.saturating_sub(1);
        if let Some(page) = self.skins.page_mut(&uuid) {
            page.recording = page.recording.saturating_sub(1);
            if let Err(why) = &recorded {
                page.said.push(Err(trf(
                    "The launcher couldn't note down what was saved: {why}",
                    &[("why", why)],
                )));
            }
        }
        if recorded.is_err() {
            return;
        }
        if let Some(a) = self.accounts.accounts.iter_mut().find(|a| a.uuid == uuid) {
            if let Some(skin) = record.skin {
                a.skin = skin;
            }
            if let Some(cape) = record.cape {
                a.cape = cape;
            }
        }
        if self.skins_offline() {
            self.skins.heads_stale = true;
        } else {
            self.load_profile_heads();
        }
    }

    pub(super) fn after_profile(&mut self) {
        if self.skins_offline() {
            return;
        }
        if let Some(Ok(profile)) = &self.skins.profile {
            let urls: Vec<String> = profile
                .skin
                .iter()
                .map(|(url, _)| url)
                .chain(profile.capes.iter().map(|c| &c.url))
                .cloned()
                .collect();
            for url in self.skins.unasked(&urls) {
                self.picture_ask(url);
            }
        }
        self.remember_worn();
        self.settle_edits();
    }

    /// What account `uuid`'s look is worked out from besides its page: the skin accounts.json
    /// has for it, and the arms of its default skin.
    pub(super) fn worn_inputs(&self, uuid: &str) -> (Option<(String, String)>, bool) {
        let recorded = self
            .accounts
            .accounts
            .iter()
            .find(|a| a.uuid == uuid)
            .and_then(|a| a.skin.clone());
        (recorded, self.default_slim(uuid))
    }

    /// Whether the default skin the game gives `uuid` has slim arms.
    pub(super) fn default_slim(&self, uuid: &str) -> bool {
        self.skin_resources().is_some_and(|resources| {
            sunrise_launcher::heads::default_skin_file(&resources, uuid)
                .is_some_and(|(_, slim)| slim)
        })
    }

    pub(super) fn worn_look(&self, account: &account::Account) -> Worn {
        let (recorded, default_slim) = self.worn_inputs(&account.uuid);
        self.skins.worn(recorded.as_ref(), default_slim)
    }

    /// Settles the shown page's edits against what its own account wears (not the account
    /// that plays, which a switch may have changed before the page is drawn again).
    pub(super) fn settle_edits(&mut self) {
        let Some(uuid) = self.skins.uuid.clone() else {
            return;
        };
        let (recorded, default_slim) = self.worn_inputs(&uuid);
        let worn = self.skins.worn(recorded.as_ref(), default_slim);
        self.skins.settle(&worn);
    }

    /// A picture from the texture server arrived (`Msg::Picture`), or couldn't be had.
    pub(crate) fn skins_picture(&mut self, url: String, png: Option<Vec<u8>>) {
        self.skins.picture(&url, png);
        // A picture can confirm a skin a Save left unconfirmed, on any page that waits for it.
        let pages: Vec<String> = std::iter::once(&self.skins.current)
            .chain(self.skins.parked.values())
            .filter(|page| page.confirmed != look::Record::default())
            .filter_map(|page| page.uuid.clone())
            .collect();
        for uuid in pages {
            self.record_confirmed(&uuid);
        }
        self.remember_worn();
    }

    /// A skin file was read (`Msg::SkinFile`).
    pub(crate) fn skins_file(&mut self, key: String, id: u64, png: Option<Vec<u8>>) {
        self.skins.file_read(&key, id, png);
    }

    /// The skin the shown page's account wears now, as its profile says (else accounts.json).
    pub(super) fn worn_skin(&self) -> Option<(String, String)> {
        match &self.skins.profile {
            Some(Ok(profile)) => profile.skin.clone(),
            _ => {
                let uuid = self.skins.uuid.as_deref()?;
                let account = self.accounts.accounts.iter().find(|a| a.uuid == uuid)?;
                account.skin.clone()
            }
        }
    }

    /// Keeps the skin worn now among the saved skins, with the arms it is worn with, on the
    /// writes queue (once: the same skin is saved once).
    pub(super) fn remember_worn(&mut self) {
        if self.skins_offline() {
            return;
        }
        let (Some(uuid), Some((url, model))) = (self.skins.uuid.clone(), self.worn_skin()) else {
            return;
        };
        if self.skins.kept.contains_key(&url) || self.skins.keeping.contains(&url) {
            return;
        }
        let Some(Picture::Ready(png)) = self.skins.pictures.get(&url) else {
            return;
        };
        let (png, tx, address, id) = (png.clone(), self.tx.clone(), url.clone(), request_id());
        let job = Box::new(move || {
            answer(
                &tx,
                || {
                    // Kept without a name: its tile shows the skin alone.
                    let kept = skin_library::folder(&uuid)
                        .and_then(|dir| skin_library::keep_seen(&dir, &png, model == "slim"));
                    Msg::SkinKept(uuid.clone(), address.clone(), kept, listing(&uuid, id))
                },
                || {
                    let listed = (id, Err(stopped()));
                    Msg::SkinKept(uuid.clone(), address.clone(), Err(stopped()), listed)
                },
            )
        });
        match self.skins.writes.push("skins-index", job) {
            Ok(()) => {
                self.skins.keeping.insert(url);
                self.skins.disk_writes += 1;
            }
            Err(_) => {
                self.skins.keep_error = Some(
                    tr("The skin you wear couldn't be kept among your saved skins just now. Open Skins again to retry.")
                        .to_string(),
                );
            }
        }
    }
}
