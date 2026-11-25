//! Choosing, the textures shown, Save and its answer, and adding and forgetting skins.
use super::*;

impl Launcher {
    /// The skin worn was kept, or couldn't be (`Msg::SkinKept`).
    pub(crate) fn skins_kept(
        &mut self,
        uuid: String,
        url: String,
        kept: Result<Saved, String>,
        listed: skin_library::Listing,
    ) {
        self.skins.disk_writes = self.skins.disk_writes.saturating_sub(1);
        let (recorded, default_slim) = self.worn_inputs(&uuid);
        let Some(page) = self.skins.page_mut(&uuid) else {
            return;
        };
        page.keeping.remove(&url);
        page.listing(listed);
        match kept {
            Ok(saved) => {
                page.keep_error = None;
                page.forget_file(&saved_key(Some(&uuid), &saved.id));
                page.kept.insert(url, saved.id);
                let worn = page.worn(recorded.as_ref(), default_slim);
                page.settle(&worn);
            }
            Err(why) => page.keep_error = Some(why),
        }
    }

    /// Chooses `change` of the look; an edit that asks for what is worn is forgotten.
    pub(super) fn choose(&mut self, change: impl FnOnce(&mut Edit)) {
        let Some(uuid) = self.skins.uuid.clone() else {
            return;
        };
        if self.skins_offline() {
            return;
        }
        let (recorded, default_slim) = self.worn_inputs(&uuid);
        let worn = self.skins.worn(recorded.as_ref(), default_slim);
        self.skins.choose(change, &worn);
    }

    /// The key and picture of `skin` with wide or slim arms, for account `uuid`.
    pub(super) fn skin_png(&self, uuid: &str, skin: &SkinPick, slim: bool) -> (String, Png) {
        match skin {
            SkinPick::Texture(url) => (url.clone(), self.skins.picture_png(url)),
            SkinPick::AccountDefault => {
                let Some(resources) = self.skin_resources() else {
                    return (String::new(), Png::Missing);
                };
                // The account's default skin with the arms shown: each comes in both.
                let key = format!("uuid-default:{}:{uuid}:{slim}", resources.display());
                let png = self
                    .skins
                    .file(&key, || account_default_path(&resources, uuid, slim));
                (key, png)
            }
            SkinPick::Saved(id) => {
                let key = saved_key(Some(uuid), id);
                let png = self.skins.file(&key, || {
                    Some(skin_library::folder(uuid).ok()?.join(format!("{id}.png")))
                });
                (key, png)
            }
            SkinPick::Default(name) => {
                let Some(resources) = self.skin_resources() else {
                    return (String::new(), Png::Missing);
                };
                // Keyed by the game files they come from: another install or profile has its own.
                let key = format!("default:{}:{name}:{slim}", resources.display());
                let png = self
                    .skins
                    .file(&key, || Some(default_skin_path(&resources, name, slim)));
                (key, png)
            }
        }
    }

    /// Reads the files asked for while the page was drawn, on the reads queue. One it won't
    /// take now is asked for again when the page is next drawn (a file read ending redraws it).
    pub(super) fn skins_read_files(&mut self) {
        let wanted = std::mem::take(self.skins.wanted.get_mut());
        for (key, path, id) in wanted {
            let (tx, name) = (self.tx.clone(), key.clone());
            let job = Box::new(move || {
                answer(
                    &tx,
                    || Msg::SkinFile(name.clone(), id, skin_library::read_skin_file(&path).ok()),
                    || Msg::SkinFile(name.clone(), id, None),
                )
            });
            match self.skins.reads.push("skins-files", job) {
                Ok(()) => {}
                Err(Refused::Full) => {
                    self.skins.files.get_mut().remove(&key);
                }
                Err(Refused::NoThread) => {
                    self.skins.files.get_mut().insert(key, File::Missing);
                }
            }
        }
    }

    /// The skin's texture, or why there is none: still loading (true), or not to be had.
    pub(super) fn skin_texture(
        &self,
        uuid: &str,
        skin: &SkinPick,
        slim: bool,
    ) -> Result<(String, Rc<Texture>), bool> {
        let (key, png) = self.skin_png(uuid, skin, slim);
        let png = match png {
            Png::Ready(png) => png,
            Png::Pending => return Err(true),
            Png::Missing => return Err(false),
        };
        let texture = self
            .skins
            .texture(&key, &png, player_model::skin)
            .ok_or(false)?;
        Ok((key, texture))
    }

    pub(super) fn cape_texture(&self, cape: &CapePick) -> Option<(String, Rc<Texture>)> {
        let CapePick::Minecraft(id) = cape else {
            return None;
        };
        let Some(Ok(profile)) = &self.skins.profile else {
            return None;
        };
        let url = &profile.capes.iter().find(|c| &c.id == id)?.url;
        let Some(Picture::Ready(png)) = self.skins.pictures.get(url) else {
            return None;
        };
        let texture = self.skins.texture(url, png, player_model::cape)?;
        Some((url.clone(), texture))
    }

    /// Sends the edits to the Minecraft services ([`look::save`]): the skin when it (or its
    /// arms) changed, then the cape when it did; nothing that wasn't edited. New arms alone are
    /// sent as such, so the Save keeps the picture the account wears when it starts.
    pub(super) fn look_save(&mut self) {
        let offline = self.skins_offline();
        let Some(account) = self.look_account() else {
            return;
        };
        if offline
            || self.skins.uuid.as_deref() != Some(&account.uuid)
            || self.skins.saving.is_some()
            || self.skins.edit.is_empty()
        {
            return;
        }
        let worn = self.worn_look(&account);
        let (_, slim, _) = self.skins.chosen(&worn);
        let png = |skin: &SkinPick| match self.skin_png(&account.uuid, skin, slim).1 {
            Png::Ready(png) => Ok(png.as_ref().clone()),
            Png::Pending => Err(tr("This skin's picture isn't here yet.").to_string()),
            Png::Missing => {
                Err(tr("This skin's picture couldn't be loaded, so it can't be saved.").to_string())
            }
        };
        let picture = match self.skins.skin_sent(&worn) {
            None => None,
            Some(SkinSent::Arms(Some(url))) => {
                Some(Ok(look::Picture::Worn(look::WornPicture::Texture(url))))
            }
            Some(SkinSent::Arms(None)) => Some(
                png(&SkinPick::AccountDefault)
                    .map(|png| look::Picture::Worn(look::WornPicture::Default(png))),
            ),
            Some(SkinSent::Picture(skin)) => Some(png(&skin).map(|png| look::Picture::New {
                png,
                saved: match &skin {
                    SkinPick::Saved(id) => Some(id.clone()),
                    _ => None,
                },
            })),
        };
        let sent = match picture {
            None => None,
            Some(Ok(picture)) => Some(look::SkinSent { picture, slim }),
            Some(Err(why)) => {
                self.skins.said = vec![Err(why)];
                return;
            }
        };
        let Some(submitted) = self.skins.submit(offline) else {
            return;
        };
        let request = look::Request {
            uuid: account.uuid.clone(),
            skin: sent,
            cape: submitted.cape.clone().map(|cape| match cape {
                CapePick::Minecraft(id) => Some(id),
                CapePick::None => None,
            }),
        };
        // Pictures already here aren't fetched again; each one fetched is fetched once.
        let known: HashMap<String, Vec<u8>> = self
            .skins
            .pictures
            .iter()
            .filter_map(|(url, picture)| match picture {
                Picture::Ready(png) => Some((url.clone(), png.clone())),
                _ => None,
            })
            .collect();
        let (id, tx) = (submitted.id, self.tx.clone());
        let started = spawn("skins-save", move || {
            let uuid = account.uuid.clone();
            // Stopped unexpectedly, the Save may have changed something already: nothing it
            // asked for is sent again, and the profile is read again (`stopped_outcome`).
            let asked = (request.skin.is_some(), request.cape.is_some());
            answer(
                &tx,
                || {
                    let ep = account::Endpoints::live();
                    let token = account::token_for(
                        &ep,
                        account::client_id().as_deref(),
                        account::store(),
                        &account,
                    );
                    let known = RefCell::new(known);
                    let fetch = |url: &str| {
                        if let Some(png) = known.borrow().get(url) {
                            return Some(png.clone());
                        }
                        let png = capes::picture(url)?;
                        known.borrow_mut().insert(url.to_owned(), png.clone());
                        Some(png)
                    };
                    let record = |record: &look::Record| {
                        account::Accounts::save_look(
                            &account.uuid,
                            record.skin.clone(),
                            record.cape.clone(),
                        )
                    };
                    let outcome = look::save(
                        &request,
                        &look::Tools {
                            ep: &ep,
                            token,
                            library: skin_library::folder(&account.uuid),
                            fetch: &fetch,
                            record: &record,
                        },
                    );
                    Msg::LookSaved(account.uuid.clone(), id, Box::new(outcome))
                },
                || Msg::LookSaved(uuid, id, Box::new(stopped_outcome(asked))),
            )
        });
        if !started {
            self.skins.saving = None;
            self.skins.said = vec![Err(tr(
                "The launcher couldn't start saving. Nothing was sent: try again.",
            )
            .to_string())];
        }
    }

    /// A Save ended (`Msg::LookSaved`). It is reconciled with its own account's page, shown or
    /// not, and what it did is in that account's record (written by the Save itself, before it
    /// ended, so an account's records are written in the order of its Saves). In the offline
    /// view it starts nothing more.
    pub(crate) fn look_saved(&mut self, uuid: String, id: u64, outcome: look::Outcome) {
        let offline = self.skins_offline();
        if let Some(profile) = &outcome.profile {
            let skin = (outcome.skin == Some(Change::Done)).then(|| profile.skin.clone());
            let cape = (outcome.cape == Some(Change::Done))
                .then(|| profile.worn_cape().map(|c| c.url.clone()));
            if skin.is_some() || cape.is_some() {
                if let Some(a) = self.accounts.accounts.iter_mut().find(|a| a.uuid == uuid) {
                    if let Some(skin) = skin {
                        a.skin = skin;
                    }
                    if let Some(cape) = cape {
                        a.cape = cape;
                    }
                }
                if offline {
                    self.skins.heads_stale = true;
                } else {
                    self.load_profile_heads();
                }
            }
        }
        let (recorded, default_slim) = self.worn_inputs(&uuid);
        let Some(page) = self.skins.page_mut(&uuid) else {
            return;
        };
        if !page.save_answered(id, &outcome) {
            return;
        }
        let worn = page.worn(recorded.as_ref(), default_slim);
        page.settle(&worn);
        self.record_confirmed(&uuid);
        if offline || self.skins.uuid.as_deref() != Some(&uuid) {
            // Shown again later, online, the page reads the profile and the saved skins anew.
            return;
        }
        self.skins_list(&uuid);
        if !matches!(self.skins.profile, Some(Ok(_))) || !self.skins.unconfirmed.is_empty() {
            if let Some(account) = self.look_account() {
                self.profile_read(&account);
            }
        }
        self.after_profile();
    }

    /// Adds skin files (chosen or dropped) to the saved skins of the account shown, on a worker.
    pub(super) fn skins_add(&mut self, paths: Vec<PathBuf>) {
        let Some(uuid) = self.skins.uuid.clone() else {
            return;
        };
        self.skins_add_for(uuid, paths);
    }

    pub(super) fn skins_add_for(&mut self, uuid: String, paths: Vec<PathBuf>) {
        if self.skins_offline() || paths.is_empty() {
            return;
        }
        if let Some(page) = self.skins.page_mut(&uuid) {
            page.added.clear();
        }
        let (tx, account, id) = (self.tx.clone(), uuid.clone(), request_id());
        let job = Box::new(move || {
            answer(
                &tx,
                || {
                    let added = add_files(&account, &paths);
                    Msg::SkinsAdded(account.clone(), added, listing(&account, id))
                },
                || {
                    let listed = (id, Err(stopped()));
                    Msg::SkinsAdded(account.clone(), vec![Err(stopped())], listed)
                },
            )
        });
        match self.skins.writes.push("skins-index", job) {
            Ok(()) => self.skins.disk_writes += 1,
            Err(_) => {
                if let Some(page) = self.skins.page_mut(&uuid) {
                    page.added = vec![tr(
                        "The launcher is busy with your saved skins. Add them again in a moment.",
                    )
                    .to_string()];
                }
            }
        }
    }

    /// Skin files were added (`Msg::SkinsAdded`): those that couldn't be say so beside the
    /// skins, and the last one added is chosen, if the page still shows the account they were
    /// added for.
    pub(crate) fn skins_added(
        &mut self,
        uuid: String,
        added: Vec<Result<Saved, String>>,
        listed: skin_library::Listing,
    ) {
        self.skins.disk_writes = self.skins.disk_writes.saturating_sub(1);
        let shown = self.skins.uuid.as_deref() == Some(&uuid);
        let Some(page) = self.skins.page_mut(&uuid) else {
            return;
        };
        page.listing(listed);
        let last = page.files_added(added);
        if !shown {
            return;
        }
        if let Some(saved) = last {
            self.choose(|edit| {
                edit.skin = Some(SkinPick::Saved(saved.id.clone()));
                edit.slim = Some(saved.slim);
            });
        }
    }

    /// The file picker, for the account shown when it opened: what it picks goes to that
    /// account, even if another plays by the time it closes.
    pub(super) fn skins_pick_file(&mut self, cx: &mut Context<Self>) {
        let Some(uuid) = self.skins.uuid.clone() else {
            return;
        };
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(tr("Add").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| {
                    this.skins_add_for(uuid, paths);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Removes the saved skin `id` of the account shown, on the writes queue.
    pub(super) fn skin_forget(&mut self, id: &str) {
        let Some(uuid) = self.skins.uuid.clone() else {
            return;
        };
        let (tx, account, skin, listed) =
            (self.tx.clone(), uuid.clone(), id.to_owned(), request_id());
        let job = Box::new(move || {
            answer(
                &tx,
                || {
                    let removed = skin_library::folder(&account)
                        .and_then(|dir| skin_library::remove(&dir, &skin));
                    Msg::SkinRemoved(
                        account.clone(),
                        skin.clone(),
                        removed,
                        listing(&account, listed),
                    )
                },
                || {
                    let after = (listed, Err(stopped()));
                    Msg::SkinRemoved(account.clone(), skin.clone(), Err(stopped()), after)
                },
            )
        });
        match self.skins.writes.push("skins-index", job) {
            Ok(()) => self.skins.disk_writes += 1,
            Err(_) => {
                self.skins.said = vec![Err(tr(
                    "The launcher is busy with your saved skins. Try again in a moment.",
                )
                .to_string())]
            }
        }
    }

    /// A saved skin was removed, or couldn't be (`Msg::SkinRemoved`).
    pub(crate) fn skin_removed(
        &mut self,
        uuid: String,
        id: String,
        removed: Result<(), String>,
        listed: skin_library::Listing,
    ) {
        self.skins.disk_writes = self.skins.disk_writes.saturating_sub(1);
        let shown = self.skins.uuid.as_deref() == Some(&uuid);
        let Some(page) = self.skins.page_mut(&uuid) else {
            return;
        };
        page.listing(listed);
        if let Err(why) = removed {
            page.said = vec![Err(why)];
            return;
        }
        page.removed(&id);
        let chosen = page.edit.skin == Some(SkinPick::Saved(id.clone()));
        if chosen && shown {
            self.choose(|edit| {
                edit.skin = None;
                edit.slim = None;
            });
        } else if chosen {
            page.edit.skin = None;
            page.edit.slim = None;
        }
    }
}

/// What a Save says it did: a line for the skin and one for the cape, each done, taken but not
/// confirmed, or not done and why; then anything it couldn't keep track of.
pub(super) fn said(outcome: &look::Outcome) -> Vec<Result<String, String>> {
    let mut lines = Vec::new();
    match &outcome.skin {
        None => {}
        Some(Change::Done) => lines.push(Ok(tr("Skin saved. Everyone sees it.").to_string())),
        Some(Change::Unconfirmed(why)) => lines.push(Err(trf(
            "Your new skin isn't confirmed yet, so it won't be sent again: {why}",
            &[("why", why)],
        ))),
        Some(Change::Failed(why)) => lines.push(Err(match &outcome.kept {
            Kept::Failed(_) => trf(
                "The skin you wear now couldn't be kept, so it wasn't replaced: {why}",
                &[("why", why)],
            ),
            _ => trf("Your skin wasn't changed: {why}", &[("why", why)]),
        })),
    }
    match &outcome.cape {
        None => {}
        Some(Change::Done) => lines.push(Ok(tr("Cape changed. Everyone sees it.").to_string())),
        Some(Change::Unconfirmed(why)) => lines.push(Err(trf(
            "Your cape change isn't confirmed yet, so it won't be sent again: {why}",
            &[("why", why)],
        ))),
        Some(Change::Failed(why)) => {
            lines.push(Err(trf("Your cape wasn't changed: {why}", &[("why", why)])))
        }
    }
    if let Some(why) = &outcome.arms {
        lines.push(Err(trf(
            "The saved skin's arms couldn't be kept: {why}",
            &[("why", why)],
        )));
    }
    if let Some(why) = &outcome.recorded {
        lines.push(Err(trf(
            "The launcher couldn't note down what was saved: {why}",
            &[("why", why)],
        )));
    }
    lines
}

/// What a Save that stopped unexpectedly reports: each change it was asked for may have gone
/// through already, so none is taken as refused (sent again) or done; the profile is read again.
pub(super) fn stopped_outcome((skin, cape): (bool, bool)) -> look::Outcome {
    let change = |asked: bool| asked.then(|| Change::Unconfirmed(stopped()));
    look::Outcome {
        kept: Kept::Nothing,
        skin: change(skin),
        cape: change(cape),
        profile: None,
        arms: None,
        recorded: None,
        uploaded: None,
    }
}
