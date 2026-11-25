//! Each account's page: what is worn, the edits made, and the answers it waits on.
use super::*;

/// What decides whether the offline developer view is on, besides the environment: the build
/// the selected profile plays and every file `developer_offline::enabled` reads for it (its
/// lineage records, then its base build's install and developer records, with their times and
/// sizes). The answer is kept while these stay the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OfflineKey {
    pub(super) played: Option<String>,
    pub(super) own: Vec<(PathBuf, sunrise_launcher::developer_offline::Stamp)>,
    pub(super) base: String,
    pub(super) base_files: Vec<(PathBuf, sunrise_launcher::developer_offline::Stamp)>,
}

/// Which skin is chosen: always a picture, never "the one worn" resolved later, so a choice
/// made while a Save changes what is worn stays the picture clicked.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SkinPick {
    /// A skin on Mojang's texture server, by its URL: one the account wore.
    Texture(String),
    /// The default skin the game gives the account's id.
    AccountDefault,
    Saved(String),
    Default(&'static str),
}

/// Which cape is chosen.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CapePick {
    None,
    Minecraft(String),
}

/// The edits not saved yet, each on its own; none leaves that part as the account wears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Edit {
    pub skin: Option<SkinPick>,
    pub slim: Option<bool>,
    pub cape: Option<CapePick>,
}

impl Edit {
    pub(super) fn is_empty(&self) -> bool {
        *self == Edit::default()
    }

    pub(super) fn touches_skin(&self) -> bool {
        self.skin.is_some() || self.slim.is_some()
    }
}

/// What the account wears, as the page's choices say it. Its cape is unknown (none) until the
/// profile has been read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Worn {
    pub skin: SkinPick,
    pub slim: bool,
    pub cape: Option<CapePick>,
    /// The URL of the skin the profile (else accounts.json) says is worn; none for the
    /// account's default skin. What a Save of new arms alone checks is still worn.
    pub texture: Option<String>,
}

/// A picture from Mojang's texture server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Picture {
    Pending,
    Ready(Vec<u8>),
    /// It couldn't be had: asked for again at the next reading of the profile.
    Failed,
}

/// A picture read from disk (a saved or default skin).
#[derive(Clone, Debug)]
pub(super) enum File {
    /// Being read by the request numbered so: only its answer counts.
    Pending(u64),
    Ready(Rc<Vec<u8>>),
    /// None to be had: read again when the page is opened again.
    Missing,
}

/// A skin picture as the preview needs it.
pub(super) enum Png {
    Ready(Rc<Vec<u8>>),
    /// On its way: the preview says it's loading.
    Pending,
    /// Not to be had: the preview says so, rather than loading forever.
    Missing,
}

/// The skin part of a Save, as the page decides it ([`Skins::skin_sent`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SkinSent {
    /// New arms for the skin worn: its URL, or none for the account's default skin.
    Arms(Option<String>),
    /// The picture chosen.
    Picture(SkinPick),
}

/// Changes the services took that no profile has shown yet, as they were sent, and what was
/// worn before them. Each profile read after them is checked: what it shows is recorded in
/// accounts.json (as the Save itself records a change it saw take). One that still shows what
/// was worn before is a profile behind: the change stays awaited, for [`AWAITED_READINGS`]
/// readings at most. One that shows something else (another skin or cape) drops it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Awaited {
    /// The skin uploaded (its id, [`skin_library::id_of_png`]) and its arms.
    pub(super) skin: Option<(String, bool)>,
    /// The skin worn before it (URL and model; none for a default skin).
    pub(super) skin_before: Option<(String, String)>,
    /// The skin a profile then showed with those arms (its URL and model): whether it is the
    /// one uploaded is known once its picture is here.
    pub(super) skin_at: Option<(String, String)>,
    /// The cape asked for, by id, or none.
    pub(super) cape: Option<Option<String>>,
    /// The cape worn before it.
    pub(super) cape_before: Option<String>,
    /// How many more profiles may still be behind before the changes are dropped.
    pub(super) left: u8,
}

/// How many profiles read after a Save may still show what was worn before it, before the
/// changes it left unconfirmed are taken not to have happened.
pub(super) const AWAITED_READINGS: u8 = 3;

/// A Save under way: its number and what it sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Submitted {
    pub id: u64,
    pub skin: Option<SkinPick>,
    pub slim: Option<bool>,
    pub cape: Option<CapePick>,
}

/// One account's page.
pub(crate) struct Skins {
    /// The account shown.
    pub(super) uuid: Option<String>,
    /// What the account wears, from its profile: none while the first reading is under way.
    pub(super) profile: Option<Result<Profile, String>>,
    /// Changes the services accepted that no profile has shown yet: the account is taken to
    /// wear them over `profile` until the next reading says what it wears.
    pub(super) unconfirmed: Edit,
    /// The same changes as they were sent, checked against each profile read after them.
    pub(super) awaited: Awaited,
    /// What later profiles showed of them, to record in accounts.json.
    pub(super) confirmed: look::Record,
    /// Those being recorded now: a Save waits for them, so records are written in Save order.
    pub(super) recording: usize,
    /// The reading whose answer counts.
    pub(super) reading: Option<u64>,
    /// The Save under way.
    pub(super) saving: Option<Submitted>,
    pub(super) edit: Edit,
    /// Pictures from Mojang's texture server by URL.
    pub(super) pictures: HashMap<String, Picture>,
    pub(super) saved: Vec<Saved>,
    /// Why the saved skins couldn't be listed, if they couldn't.
    pub(super) library_error: Option<String>,
    /// Why the skin worn couldn't be kept among them, if it couldn't: until one is kept.
    pub(super) keep_error: Option<String>,
    /// The number of the latest listing of the saved skins taken in: an older one changes
    /// nothing.
    pub(super) listed: u64,
    /// The saved skin that keeps each skin worn, by its URL: every skin worn is kept, so one
    /// replaced by a Save stays in the list to wear again.
    pub(super) kept: HashMap<String, String>,
    /// Skins worn (by URL) being kept now.
    pub(super) keeping: HashSet<String>,
    /// What the last Save said, a line for each change; or why it couldn't start.
    pub(super) said: Vec<Result<String, String>>,
    /// The files of the last skins added that couldn't be, each with why: shown beside the
    /// skins, until more are added.
    pub(super) added: Vec<String>,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    /// Where a drag on the preview began, and the turn then.
    pub(super) drag: Option<(gpui::Point<gpui::Pixels>, f32, f32)>,
    /// Pictures read from disk (default and saved skins), by key.
    pub(super) files: RefCell<HashMap<String, File>>,
    /// Files asked for while the page was drawn, by key: read once it has been.
    pub(super) wanted: RefCell<Vec<(String, PathBuf, u64)>>,
    /// Pictures decoded, by key.
    pub(super) textures: RefCell<HashMap<String, Option<Rc<Texture>>>>,
    /// Tile pictures as the window draws them, by key and scale.
    pub(super) baked: RefCell<HashMap<(String, u32), Arc<RenderImage>>>,
    /// The preview drawn last, by what it shows.
    pub(super) shown: Option<(u64, Arc<RenderImage>)>,
}

impl Default for Skins {
    fn default() -> Self {
        Skins {
            uuid: None,
            profile: None,
            unconfirmed: Edit::default(),
            awaited: Awaited::default(),
            confirmed: look::Record::default(),
            recording: 0,
            reading: None,
            saving: None,
            edit: Edit::default(),
            pictures: HashMap::new(),
            saved: Vec::new(),
            library_error: None,
            keep_error: None,
            listed: 0,
            kept: HashMap::new(),
            keeping: HashSet::new(),
            said: Vec::new(),
            added: Vec::new(),
            yaw: 0.55,
            pitch: 0.12,
            drag: None,
            files: RefCell::default(),
            wanted: RefCell::default(),
            textures: RefCell::default(),
            baked: RefCell::default(),
            shown: None,
        }
    }
}

/// Every account's page: the one shown, and those of the accounts shown before, kept as they
/// were left (a Save under way, edits, what the profile said). The shown page is reached
/// through `Deref`.
#[derive(Default)]
pub(crate) struct SkinsPages {
    pub(super) current: Skins,
    pub(super) parked: HashMap<String, Skins>,
    /// Pictures fetched from Mojang's texture server.
    pub(super) net: Queue,
    /// Skin files read for the pictures drawn.
    pub(super) reads: Queue,
    /// The saved skins' index listed and changed (skins added, kept, removed): apart from the
    /// reads, so a page of pictures never crowds out a change.
    pub(super) writes: Queue,
    /// Saved skins being added, kept or removed now: a data move waits for them.
    pub(super) disk_writes: usize,
    /// Whether the page was last opened in the offline developer view: drawn in the other
    /// mode, it is opened anew.
    pub(super) opened_offline: Option<bool>,
    /// A Save that ended in the offline view changed a profile head not fetched since.
    pub(super) heads_stale: bool,
    /// Whether the offline developer view is on, and what that was worked out from.
    pub(super) offline: RefCell<Option<(OfflineKey, bool)>>,
    /// The latest answer to the same, for the texture jobs queued: one that starts once it is
    /// on fetches nothing.
    pub(super) offline_flag: Arc<AtomicBool>,
    /// The prepared game files the default skins come from and the default skins they have,
    /// for the profile build and the builds installed they were looked for with.
    pub(super) resources: RefCell<Option<ResourcesFound>>,
    /// The offline developer view, as a test sets it.
    #[cfg(test)]
    pub(super) offline_for_test: Option<bool>,
}

/// [`SkinsPages::resources`]: the build played, the builds installed, and what they have.
pub(super) type ResourcesFound = (
    Option<String>,
    Vec<String>,
    Option<(PathBuf, Vec<&'static str>)>,
);

impl std::ops::Deref for SkinsPages {
    type Target = Skins;
    fn deref(&self) -> &Skins {
        &self.current
    }
}

impl std::ops::DerefMut for SkinsPages {
    fn deref_mut(&mut self) -> &mut Skins {
        &mut self.current
    }
}

impl SkinsPages {
    /// Shows the page of account `uuid`: the one it had, else a new one. Pages of accounts no
    /// longer `signed_in` are forgotten, unless a Save of theirs is still under way.
    pub(super) fn show(&mut self, uuid: &str, signed_in: impl Fn(&str) -> bool) {
        if self.current.uuid.as_deref() != Some(uuid) {
            let page = self
                .parked
                .remove(uuid)
                .unwrap_or_else(|| Skins::new_for(uuid));
            let before = std::mem::replace(&mut self.current, page);
            if let Some(was) = before.uuid.clone() {
                self.parked.insert(was, before);
            }
        }
        self.parked
            .retain(|uuid, page| page.saving.is_some() || signed_in(uuid));
    }

    /// The page of account `uuid`, shown or not.
    pub(super) fn page_mut(&mut self, uuid: &str) -> Option<&mut Skins> {
        if self.current.uuid.as_deref() == Some(uuid) {
            Some(&mut self.current)
        } else {
            self.parked.get_mut(uuid)
        }
    }

    pub(super) fn pages_mut(&mut self) -> impl Iterator<Item = &mut Skins> {
        std::iter::once(&mut self.current).chain(self.parked.values_mut())
    }

    /// A picture arrived: every page that asked for it gets it.
    pub(super) fn picture(&mut self, url: &str, png: Option<Vec<u8>>) {
        for page in self.pages_mut() {
            if page.pictures.get(url) == Some(&Picture::Pending) {
                page.picture(url.to_owned(), png.clone());
            }
        }
    }

    /// A file was read: every page that asked for it gets it.
    pub(super) fn file_read(&mut self, key: &str, id: u64, png: Option<Vec<u8>>) {
        let png = png.map(Rc::new);
        for page in self.pages_mut() {
            let files = page.files.get_mut();
            if matches!(files.get(key), Some(File::Pending(asked)) if *asked == id) {
                files.insert(
                    key.to_owned(),
                    png.clone().map_or(File::Missing, File::Ready),
                );
                page.forget(key);
            }
        }
    }

    /// A Save or a skin being written to disk is under way, for any account, shown or not: a
    /// data move or a launcher restart would cut it off (a Mojang change left without its
    /// record in accounts.json, a skin half added).
    pub(crate) fn writing(&self) -> bool {
        self.disk_writes > 0
            || self.current.saving.is_some()
            || self.parked.values().any(|page| page.saving.is_some())
    }
}

impl Skins {
    pub(super) fn new_for(uuid: &str) -> Skins {
        Skins {
            uuid: Some(uuid.to_owned()),
            ..Default::default()
        }
    }

    /// What the account wears. `recorded` is the skin accounts.json has for it, used until the
    /// profile has been read; `default_slim` the arms of its default skin, when it wears none.
    /// Changes accepted but not yet shown by a profile count as worn.
    pub(super) fn worn(&self, recorded: Option<&(String, String)>, default_slim: bool) -> Worn {
        let (skin, cape) = match &self.profile {
            Some(Ok(profile)) => (
                profile.skin.as_ref(),
                Some(match profile.worn_cape() {
                    Some(c) => CapePick::Minecraft(c.id.clone()),
                    None => CapePick::None,
                }),
            ),
            _ => (recorded, None),
        };
        let slim = match skin {
            Some((_, model)) => model == "slim",
            None => default_slim,
        };
        let texture = skin.map(|(url, _)| url.clone());
        let pick = match &texture {
            Some(url) => self.picked(SkinPick::Texture(url.clone())),
            None => SkinPick::AccountDefault,
        };
        Worn {
            skin: self.unconfirmed.skin.clone().unwrap_or(pick),
            slim: self.unconfirmed.slim.unwrap_or(slim),
            cape: self.unconfirmed.cape.clone().or(cape),
            texture,
        }
    }

    /// `skin` as the page lists it: a skin worn at a URL is the saved skin that keeps it, once
    /// one does.
    pub(super) fn picked(&self, skin: SkinPick) -> SkinPick {
        match &skin {
            SkinPick::Texture(url) => self
                .kept
                .get(url)
                .map_or(skin.clone(), |id| SkinPick::Saved(id.clone())),
            _ => skin,
        }
    }

    /// The look the preview shows: each edit over what is worn.
    pub(super) fn chosen(&self, worn: &Worn) -> (SkinPick, bool, Option<CapePick>) {
        (
            self.edit.skin.clone().unwrap_or_else(|| worn.skin.clone()),
            self.edit.slim.unwrap_or(worn.slim),
            self.edit.cape.clone().or_else(|| worn.cape.clone()),
        )
    }

    /// Forgets the edits that ask for what is worn already. A cape edit stays while the worn
    /// cape is unknown: it is an explicit choice. While a Save runs nothing is forgotten: what
    /// is worn is about to change, and a choice that matches it now may not match it after.
    pub(super) fn settle(&mut self, worn: &Worn) {
        if self.saving.is_some() {
            return;
        }
        self.edit.skin = self.edit.skin.take().map(|skin| self.picked(skin));
        if self.edit.skin.as_ref() == Some(&worn.skin) {
            self.edit.skin = None;
        }
        if self.edit.skin.is_none() && self.edit.slim == Some(worn.slim) {
            self.edit.slim = None;
        }
        if worn.cape.is_some() && self.edit.cape == worn.cape {
            self.edit.cape = None;
        }
    }

    /// Chooses `change` of the look, the account wearing `worn`: what the last Save said gives
    /// way to the choice, but not the files the last import couldn't add.
    pub(super) fn choose(&mut self, change: impl FnOnce(&mut Edit), worn: &Worn) {
        change(&mut self.edit);
        self.settle(worn);
        self.said.clear();
    }

    /// A reading asked for.
    pub(super) fn read_asked(&mut self) -> u64 {
        let id = request_id();
        self.reading = Some(id);
        id
    }

    /// A reading answered: kept when it is for this page's account and the last one asked, the
    /// page isn't `offline` now, and no Save is under way (its answer is newer than any
    /// reading made before it ends).
    pub(super) fn read_answered(
        &mut self,
        uuid: &str,
        id: u64,
        result: Result<Profile, String>,
        offline: bool,
    ) -> bool {
        if self.uuid.as_deref() != Some(uuid) || self.reading != Some(id) {
            return false;
        }
        self.reading = None;
        // Answered: in the offline view it is taken in no more than during a Save, and a later
        // opening reads again.
        if offline || self.saving.is_some() {
            return false;
        }
        if let Ok(profile) = &result {
            // The reading says what is worn: accepted changes are in it, or didn't take.
            self.profile_seen(profile);
            self.unconfirmed = Edit::default();
        }
        // A reading that failed after one that worked keeps what was known.
        if result.is_ok() || !matches!(self.profile, Some(Ok(_))) {
            self.profile = Some(result);
        }
        true
    }

    /// Checks the changes still awaited against `profile`. A cape worn as asked is confirmed (to
    /// record); a skin worn with the arms sent is compared with the one uploaded once its
    /// picture is here ([`Self::skin_checked`]). A profile still showing what was worn before is
    /// behind: the change stays awaited, [`AWAITED_READINGS`] profiles at most. One showing
    /// anything else drops it, the profile being what the account wears.
    pub(super) fn profile_seen(&mut self, profile: &Profile) {
        let awaited = &mut self.awaited;
        if awaited.skin.is_none() && awaited.cape.is_none() {
            return;
        }
        if let Some(sent) = awaited.cape.clone() {
            let worn = profile.worn_cape();
            let worn_id = worn.map(|c| c.id.clone());
            if worn_id == sent {
                self.confirmed.cape = Some(worn.map(|c| c.url.clone()));
                awaited.cape = None;
            } else if worn_id != awaited.cape_before {
                awaited.cape = None;
            }
        }
        if let Some((_, slim)) = &awaited.skin {
            match &profile.skin {
                // Behind: what was worn before.
                shown if *shown == awaited.skin_before => {}
                Some((url, model)) if (model == "slim") == *slim => {
                    awaited.skin_at = Some((url.clone(), model.clone()));
                }
                _ => {
                    awaited.skin = None;
                    awaited.skin_at = None;
                }
            }
        }
        if awaited.skin.is_some() || awaited.cape.is_some() {
            awaited.left = awaited.left.saturating_sub(1);
            if awaited.left == 0 && awaited.skin_at.is_none() {
                *awaited = Awaited::default();
            }
        }
        self.skin_checked();
    }

    /// Compares the skin awaited with the one a profile showed, once that one's picture is
    /// here: the same skin is confirmed (to record), another is dropped. A picture that
    /// couldn't be had leaves it awaited: it is asked for again at the next reading.
    pub(super) fn skin_checked(&mut self) {
        self.skin_compared();
        // Nothing left awaited: nothing of it is kept either.
        if self.awaited.skin.is_none() && self.awaited.cape.is_none() {
            self.awaited = Awaited::default();
        }
    }

    pub(super) fn skin_compared(&mut self) {
        let (Some((id, _)), Some((url, model))) = (&self.awaited.skin, &self.awaited.skin_at)
        else {
            return;
        };
        let same = match self.pictures.get(url) {
            Some(Picture::Ready(png)) => skin_library::id_of_png(png).as_ref() == Some(id),
            Some(Picture::Failed | Picture::Pending) | None => return,
        };
        if same {
            self.confirmed.skin = Some(Some((url.clone(), model.clone())));
        }
        self.awaited.skin = None;
        self.awaited.skin_at = None;
        self.awaited.skin_before = None;
    }

    /// Asks for a picture from Mojang's texture server: true when it is to be fetched (not
    /// here or on its way already, and the page isn't `offline`). One that couldn't be had
    /// before is asked for again.
    pub(super) fn ask_picture(&mut self, url: &str, offline: bool) -> bool {
        if offline
            || matches!(
                self.pictures.get(url),
                Some(Picture::Pending | Picture::Ready(_))
            )
        {
            return false;
        }
        self.pictures.insert(url.to_owned(), Picture::Pending);
        true
    }

    /// A Save of the edits asked for.
    pub(super) fn submit(&mut self, offline: bool) -> Option<Submitted> {
        if offline || self.edit.is_empty() || self.saving.is_some() || self.recording > 0 {
            return None;
        }
        // A change this Save makes again is no longer one to record from an earlier Save.
        if self.edit.touches_skin() {
            self.awaited.skin = None;
            self.awaited.skin_at = None;
        }
        if self.edit.cape.is_some() {
            self.awaited.cape = None;
        }
        let submitted = Submitted {
            id: request_id(),
            skin: self.edit.skin.clone(),
            slim: self.edit.slim,
            cape: self.edit.cape.clone(),
        };
        self.saving = Some(submitted.clone());
        self.said.clear();
        Some(submitted)
    }

    /// The Save `id` answered, if it is this page's. Each change the services accepted is done:
    /// its edit is forgotten unless the player changed it again meanwhile, and it isn't sent
    /// again. What a profile showed is what is worn; what was accepted but not shown is taken
    /// as worn until the next reading. What failed stays to do. Choices made meanwhile are the
    /// pictures clicked, and stay as they are. The caller then settles the edits against what
    /// is worn now.
    pub(super) fn save_answered(&mut self, id: u64, outcome: &look::Outcome) -> bool {
        let Some(submitted) = self.saving.take_if(|s| s.id == id) else {
            return false;
        };
        // A reading asked before this Save ended may say what was worn before it.
        self.reading = None;
        // What was worn before this Save: read before `outcome.profile` replaces `self.profile`.
        let before = match &self.profile {
            Some(Ok(profile)) => Some(profile.clone()),
            _ => None,
        };
        if let Some(profile) = &outcome.profile {
            // A change an earlier Save left unconfirmed may show in it.
            self.profile_seen(profile);
            self.profile = Some(Ok(profile.clone()));
            self.unconfirmed = Edit::default();
        }
        if let Kept::Kept { url, saved } = &outcome.kept {
            self.kept.insert(url.clone(), saved.id.clone());
            self.forget_file(&saved_key(self.uuid.as_deref(), &saved.id));
        }
        let accepted = |change: &Option<Change>| change.as_ref().is_some_and(Change::accepted);
        if matches!(outcome.skin, Some(Change::Unconfirmed(_))) {
            self.unconfirmed.skin = submitted.skin.clone();
            self.unconfirmed.slim = submitted.slim;
            self.awaited.skin = outcome.uploaded.clone();
            self.awaited.skin_before = before.as_ref().and_then(|p| p.skin.clone());
            self.awaited.skin_at = None;
            self.awaited.left = AWAITED_READINGS;
        }
        if matches!(outcome.cape, Some(Change::Unconfirmed(_))) {
            self.unconfirmed.cape = submitted.cape.clone();
            self.awaited.cape = submitted.cape.clone().map(|cape| match cape {
                CapePick::Minecraft(id) => Some(id),
                CapePick::None => None,
            });
            self.awaited.cape_before = before
                .as_ref()
                .and_then(|p| p.worn_cape().map(|c| c.id.clone()));
            self.awaited.left = AWAITED_READINGS;
        }
        if accepted(&outcome.skin) {
            // A newer accepted write supersedes any earlier confirmed record still waiting on the
            // write queue for the same field.
            self.confirmed.skin = None;
            if self.edit.skin == submitted.skin {
                self.edit.skin = None;
            }
            if self.edit.slim == submitted.slim {
                self.edit.slim = None;
            }
        }
        if accepted(&outcome.cape) {
            self.confirmed.cape = None;
            if self.edit.cape == submitted.cape {
                self.edit.cape = None;
            }
        }
        self.said = said(outcome);
        true
    }

    /// The pictures not here or on their way: those never asked for, and those that couldn't
    /// be had.
    pub(super) fn unasked<'a>(&self, urls: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        urls.into_iter()
            .filter(|url| {
                !matches!(
                    self.pictures.get(*url),
                    Some(Picture::Pending | Picture::Ready(_))
                )
            })
            .cloned()
            .collect()
    }

    pub(super) fn picture(&mut self, url: String, png: Option<Vec<u8>>) {
        self.forget(&url);
        self.pictures
            .insert(url, png.map_or(Picture::Failed, Picture::Ready));
        self.skin_checked();
    }

    pub(super) fn picture_png(&self, url: &str) -> Png {
        match self.pictures.get(url) {
            Some(Picture::Ready(png)) => Png::Ready(Rc::new(png.clone())),
            Some(Picture::Failed) => Png::Missing,
            Some(Picture::Pending) | None => Png::Pending,
        }
    }

    /// Drops what was drawn from the picture `key`.
    pub(super) fn forget(&mut self, key: &str) {
        self.textures.get_mut().remove(key);
        self.baked.get_mut().retain(|(k, _), _| !k.starts_with(key));
    }

    /// Drops the file `key` and what was drawn from it: it is read again when next drawn.
    pub(super) fn forget_file(&mut self, key: &str) {
        self.files.get_mut().remove(key);
        self.forget(key);
    }

    /// The picture at `path` (none: there is none to look for), read once, on a worker: asked
    /// for here and read once the page has been drawn.
    pub(super) fn file(&self, key: &str, path: impl FnOnce() -> Option<PathBuf>) -> Png {
        if let Some(file) = self.files.borrow().get(key) {
            return match file {
                File::Pending(_) => Png::Pending,
                File::Ready(png) => Png::Ready(png.clone()),
                File::Missing => Png::Missing,
            };
        }
        let Some(path) = path() else {
            self.files
                .borrow_mut()
                .insert(key.to_owned(), File::Missing);
            return Png::Missing;
        };
        let id = request_id();
        self.files
            .borrow_mut()
            .insert(key.to_owned(), File::Pending(id));
        self.wanted.borrow_mut().push((key.to_owned(), path, id));
        Png::Pending
    }

    /// The picture with `key`, decoded once by `decode`.
    pub(super) fn texture(
        &self,
        key: &str,
        png: &[u8],
        decode: fn(&[u8]) -> Option<Texture>,
    ) -> Option<Rc<Texture>> {
        if let Some(known) = self.textures.borrow().get(key) {
            return known.clone();
        }
        let texture = decode(png).map(Rc::new);
        self.textures
            .borrow_mut()
            .insert(key.to_owned(), texture.clone());
        texture
    }

    /// A listing of the saved skins arrived (numbered `id`): taken in unless a newer one was.
    /// It says only whether the list could be read; why the skin worn couldn't be kept stays.
    pub(super) fn listing(&mut self, (id, listed): skin_library::Listing) {
        if id <= self.listed {
            return;
        }
        self.listed = id;
        match listed {
            Ok(saved) => {
                self.saved = saved;
                self.library_error = None;
            }
            Err(why) => self.library_error = Some(why),
        }
    }

    /// Skin files were added: those that couldn't be are kept to show (a choice made after
    /// doesn't hide them); the last one that was is returned, to be chosen.
    pub(super) fn files_added(&mut self, added: Vec<Result<Saved, String>>) -> Option<Saved> {
        let mut last = None;
        self.added.clear();
        for result in added {
            match result {
                Ok(saved) => {
                    // Its picture may have been written anew.
                    self.forget_file(&saved_key(self.uuid.as_deref(), &saved.id));
                    last = Some(saved);
                }
                Err(why) => self.added.push(why),
            }
        }
        last
    }

    /// What a Save sends as the skin, when the skin or its arms were edited: new arms alone
    /// (the picture chosen is the one worn, and no accepted change still stands in for it) are
    /// sent as such, with the URL of the skin the page shows as worn (none: the account's
    /// default skin), so the Save keeps what the account wears when it starts; anything else
    /// is the picture chosen.
    pub(super) fn skin_sent(&self, worn: &Worn) -> Option<SkinSent> {
        if !self.edit.touches_skin() {
            return None;
        }
        let (skin, _, _) = self.chosen(worn);
        Some(if skin == worn.skin && self.unconfirmed.skin.is_none() {
            SkinSent::Arms(worn.texture.clone())
        } else {
            SkinSent::Picture(skin)
        })
    }

    /// A saved skin was removed: no skin worn is taken for it any more, and its picture is
    /// forgotten.
    pub(super) fn removed(&mut self, id: &str) {
        self.kept.retain(|_, kept| kept != id);
        self.forget_file(&saved_key(self.uuid.as_deref(), id));
    }
}
