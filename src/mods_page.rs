//! The Mods page's state and work: Sunrise's registry mods, modpacks and shader packs, and
//! resource and data packs from Modrinth, installed with their checksum checked, Sunrise mods
//! turned on and off, and what was installed updated or removed (`sunrise_launcher::browser`).

use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use sunrise_launcher::browser::{
    self, installed, mod_list, sunrise, Kind as PackKind, Project, SourceId,
};
use sunrise_launcher::native_status;

mod queue;
pub(crate) mod removal;
mod undo;

/// How a build ended, for the install behind it.
pub(super) enum BuildEnd {
    /// It built (the mods it left out are read from its record).
    Finished,
    /// It waits for the player: a question about worlds, or the permission for native code.
    Pending,
    /// It stopped for a reason outside the mods.
    Stopped(String),
}

impl BuildEnd {
    pub(super) fn of<T>(result: &Result<T, native_mods::Failed>) -> Self {
        use native_mods::Failed;
        match result {
            Ok(_) | Err(Failed::Mods { .. } | Failed::Nothing) => Self::Finished,
            Err(Failed::Worlds(_) | Failed::NoConsent) => Self::Pending,
            Err(e @ Failed::Other(_)) => Self::Stopped(e.to_string()),
        }
    }
}

/// What a build that has ended costs the mods an install brought in.
#[derive(Debug, PartialEq)]
enum BuildCost {
    /// Nothing: every mod built.
    Nothing,
    /// The build stopped for a reason outside the mods: every mod it was started for goes at
    /// once, and Install can be tried again.
    Rollback(Vec<String>),
    /// The build went on without these mods, which don't compile or don't fit: they are listed
    /// for the player, and go when they continue.
    Failures(Vec<native_mods::BuildFailure>),
}

fn build_cost(
    brought: &[String],
    stopped: bool,
    failures: Vec<native_mods::BuildFailure>,
) -> BuildCost {
    match (stopped, failures.is_empty()) {
        (true, _) => BuildCost::Rollback(brought.to_vec()),
        (false, true) => BuildCost::Nothing,
        (false, false) => BuildCost::Failures(failures),
    }
}

/// An install that would break installed mods, waiting for the player's answer.
pub(crate) struct BreakAsk {
    source: SourceId,
    pub(crate) id: String,
    known: Option<Box<Project>>,
    pub(crate) shown: Option<String>,
    /// The mods it would break: id, title and what each needs.
    pub(crate) broken: Vec<(String, String, String)>,
}

/// An install that failed, as its card says it.
pub(crate) struct CardError {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) line: String,
    pub(crate) details: String,
    /// Details is open.
    pub(crate) open: bool,
}

/// What a build that an install started cost it. Nothing stays installed from either.
pub(crate) enum InstallFailure {
    /// Mods that failed to compile: each with its reason and log. They were removed and the
    /// rest of the build went on.
    ModsDidNotBuild(Vec<native_mods::BuildFailure>),
    /// The build stopped for a reason outside the mods (no network, the build tools didn't
    /// download, no room): the mods that went in were removed, the notice says why, and
    /// Install can be tried again.
    Interrupted {
        /// The mods it put back or removed.
        ids: Vec<String>,
        /// Why it stopped, as the build said.
        reason: String,
    },
}

/// How an install ended well: what to tell the player, if anything (a registry mod says
/// nothing: its card shows it installed), and whether native code went in, which a build takes up.
pub(super) struct Finished {
    said: Option<String>,
    /// The native mods that went in, by id: a build takes them up, and they go again if it fails.
    native: Vec<String>,
    /// What came in with the mod (its dependencies), by title, for "Also installed".
    also: Vec<String>,
    /// The dependencies it moved to another version: title, before, after.
    updated: Vec<(String, String, String)>,
    /// What it replaced or added, to put back if the build it asks for stops.
    touched: Vec<undo::Touched>,
}

impl Finished {
    fn said(said: String) -> Self {
        Self {
            said: Some(said),
            native: Vec::new(),
            also: Vec::new(),
            updated: Vec::new(),
            touched: Vec::new(),
        }
    }
}

pub(super) enum Event {
    Progress(String, f32),
    Done(Result<Finished, String>),
    /// The native mods an install put in before it failed: a build takes them up.
    Brought(Vec<String>, Vec<undo::Touched>),
    /// What the automatic updates did (`sunrise::auto`).
    AutoUpdated(AutoOutcome),
    /// The same, for the Play that waited for the mods to be up to date.
    PlayUpdated(AutoOutcome),
    /// A project page's part, an image or a filter's values (`mods_project`).
    Loaded(super::mods_project::Loaded),
}

pub(super) struct Mods {
    focus: FocusHandle,
    pub(super) focused: bool,
    everything: bool,
    /// The Minecraft version Sunrise plays, read again with each search ([`browser::game`]).
    pub(super) game: String,
    results_list: cull::Cull,
    installed_list: cull::Cull,
    pub(super) report_list: cull::Cull,
    pub(super) installed: Vec<installed::Entry>,
    /// The registry native mods' versions the player picked on their Versions tab, by id
    /// (`sunrise::chosen`), read with `installed`.
    pub(super) chosen: std::collections::BTreeMap<String, String>,
    /// The build the page's Minecraft version was last worked out for (the selected instance's).
    game_for: Option<(Option<String>, Option<String>)>,
    /// Whether the start's auto-update ran: it waits for the release list.
    pub(super) auto_updated: bool,
    /// The game folder the browser installs into and lists: the selected instance's.
    home: ModsHome,
    /// Its worlds (folder, name), the last played first, and the one data packs go into.
    worlds: Vec<(PathBuf, String)>,
    world: Option<PathBuf>,
    /// The project being installed, and the line and fraction of its download.
    pub(super) busy: Option<(String, String, f32)>,
    pub(super) message: Option<Result<String, String>>,
    /// What waits for the install and build slot.
    queue: queue::Queue,
    /// The start's automatic update runs: Play and installs wait for it.
    pub(super) updating: bool,
    /// The profile the install that runs was asked for.
    running: Option<queue::Target>,
    /// The question a switch or update asks before it removes mods saved worlds use. Nothing
    /// is removed until it is answered, and the answer defaults to Stay.
    pub(crate) world_ask: Option<WorldAsk>,
    /// What came in with each mod installed this run, by id (the card's "Also installed").
    pub(crate) also: HashMap<String, Vec<String>>,
    /// The dependencies each moved to another version (the card's "Also updated").
    pub(crate) updated: HashMap<String, Vec<(String, String, String)>>,
    /// Why the last install failed, on its card: one plain line, the whole text behind Details.
    pub(crate) card_error: Option<CardError>,
    /// An install that would break installed mods, asked before anything is downloaded.
    pub(crate) break_ask: Option<BreakAsk>,
    /// The mods that go once the player continues (`removal`).
    pub(crate) removal: removal::Pending,
    /// The native mods the running build was started to take up.
    building: Vec<String>,
    /// What the installs behind that build replaced or added.
    undo: Vec<undo::Touched>,
    /// How the last build of an install ended, when it cost the install: read by the card and
    /// by the failure sheet, until the next build starts.
    pub(crate) failed: Option<InstallFailure>,
    pub(super) updates: HashMap<(SourceId, String), String>,
    /// An install awaiting explicit native-code consent, before any download.
    /// The selected instance's native build state, the mods it would build again
    /// (`native_status::unbuilt`), and the `native_status::fingerprint` they were read for: read
    /// again only when that changes.
    pub(super) native_state:
        std::cell::RefCell<Option<(String, native_status::State, Vec<String>)>>,
    /// The instance and the mods whose compile failed in the last stop: their chips say so until
    /// the next build attempt, whatever the player chose.
    pub(super) unbuilt: Option<(String, Vec<String>)>,
    /// The project page, filters and images.
    pub(super) browse: super::mods_project::Browse,
    pub(super) tx: Sender<Event>,
    rx: Receiver<Event>,
}

/// The profile routed to the browser and its pending native-code actions. Kept separate from
/// the page's window state so both use the same binding even before the page has opened.
#[derive(Default)]
struct ModsHome(Option<PathBuf>);

impl ModsHome {
    fn follow_home(&mut self, home: Option<PathBuf>) {
        if home.is_some() && self.0 != home {
            self.0 = home;
        }
    }

    fn current(&self) -> PathBuf {
        self.0.clone().unwrap_or_else(backend::game_home)
    }

    fn begin_build_consent(&mut self, selected: PathBuf) -> Arc<PathBuf> {
        self.follow_home(Some(selected));
        Arc::new(self.current())
    }
}

impl Mods {
    /// The saved settings changed (a save, or its retry): the builds the search follows are worked out again.
    pub(super) fn settings_saved(&mut self) {
        self.game_for = None;
    }

    pub(super) fn reset_lists(&self) {
        self.results_list.reset();
        self.installed_list.reset();
        self.report_list.reset();
    }

    /// The game folder installs go into: the selected instance's, once the page has read it.
    /// Reads what the browser installed in the game folder again, with the versions picked.
    pub(super) fn reload_installed(&mut self) {
        let home = self.home();
        self.installed = installed::Installed::load(&home).entries;
        self.chosen = sunrise::chosen(&home);
    }

    /// Points the folder installs and toggles go into at `home`, the selected instance's folder,
    /// without reading or writing anything: the Mods screens (`mods`) call it so
    /// that a native-code consent answered later continues in the folder they show.
    pub(super) fn follow_home(&mut self, home: Option<PathBuf>) {
        self.home.follow_home(home);
    }

    pub(super) fn home(&self) -> PathBuf {
        self.home.current()
    }

    /// The mods the last install's build removed for not compiling, each with its reason and
    /// log: what the failure sheet lists.
    pub(crate) fn build_failures(&self) -> &[native_mods::BuildFailure] {
        match &self.failed {
            Some(InstallFailure::ModsDidNotBuild(failures)) => failures,
            _ => &[],
        }
    }

    /// Keeps what an install touched, the state before the earliest install first.
    fn remember(&mut self, touched: Vec<undo::Touched>) {
        for t in touched {
            if !self
                .undo
                .iter()
                .any(|have| have.id == t.id && have.home == t.home)
            {
                self.undo.push(t);
            }
        }
    }

    /// Whether a build that takes up mod `id` is waiting to start or running.
    pub(crate) fn builds(&self, id: &str) -> bool {
        self.building.iter().any(|b| b == id) || self.queue.will_build(id)
    }

    /// Whether the install of mod `id` waits for its turn.
    pub(crate) fn waiting(&self, id: &str) -> bool {
        self.queue.waits(id)
    }

    pub(super) fn begin_build_consent(&mut self, selected: PathBuf) -> Arc<PathBuf> {
        self.home.begin_build_consent(selected)
    }

    pub(super) fn new(cx: &mut App) -> Self {
        let (tx, rx) = crate::wake::channel();
        Self {
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            focused: false,
            everything: false,
            game: browser::game(),
            results_list: cull::Cull::default(),
            installed_list: cull::Cull::default(),
            report_list: cull::Cull::default(),
            installed: Vec::new(),
            chosen: Default::default(),
            game_for: None,
            auto_updated: false,
            home: ModsHome::default(),
            worlds: Vec::new(),
            world: None,
            busy: None,
            message: None,
            queue: Default::default(),
            running: None,
            updating: false,
            building: Vec::new(),
            undo: Vec::new(),
            world_ask: None,
            removal: Default::default(),
            break_ask: None,
            also: HashMap::new(),
            updated: HashMap::new(),
            card_error: None,
            failed: None,
            native_state: Default::default(),
            unbuilt: None,
            updates: HashMap::new(),
            browse: Default::default(),
            tx,
            rx,
        }
    }

    pub(super) fn note_focus(&mut self, window: &Window) {
        self.focused = self.focus.is_focused(window);
    }
}

impl Launcher {
    /// Called with the launcher's other work: what the background sent.
    pub(super) fn mods_tick(&mut self) -> bool {
        let mut changed = false;
        let mut loaded = Vec::new();
        let mut done = false;
        let mut native = Vec::new();
        let mut native_in = None;
        let mut play_ready = false;
        let mut play_blocked = false;
        while let Ok(event) = self.mods.rx.try_recv() {
            changed = true;
            let is_play = matches!(event, Event::PlayUpdated(..));
            let m = &mut self.mods;
            match event {
                Event::Progress(line, f) => {
                    if let Some(busy) = &mut m.busy {
                        busy.1 = line;
                        busy.2 = f;
                    }
                }
                Event::Done(result) => {
                    done = true;
                    let id = m.busy.take().map(|(id, ..)| id);
                    native_in = m.running.take().map(|t| t.instance);
                    m.card_error = match (&result, &id) {
                        (Err(why), Some(id)) => Some(CardError {
                            title: sunrise::known_index()
                                .and_then(|index| index.project(id).map(|p| p.name.clone()))
                                .unwrap_or_else(|| id.clone()),
                            id: id.clone(),
                            line: crate::mods::install_state::plain_failure(why),
                            details: why.clone(),
                            open: false,
                        }),
                        _ => None,
                    };
                    m.message = match result {
                        Ok(finished) => {
                            if let Some(id) = id {
                                m.also.insert(id.clone(), finished.also);
                                m.updated.insert(id, finished.updated);
                            }
                            if !finished.native.is_empty() {
                                m.remember(finished.touched);
                            }
                            native.extend(finished.native);
                            finished.said.map(|said| {
                                crate::glass_toast::say(said.clone());
                                Ok(said)
                            })
                        }
                        Err(why) => Some(Err(why)),
                    };
                    m.reload_installed();
                    m.installed_list.reset();
                }
                Event::Brought(ids, touched) => {
                    native.extend(ids);
                    m.remember(touched);
                    native_in = m.running.as_ref().map(|t| t.instance.clone());
                }
                Event::Loaded(l) => loaded.push(l),
                Event::AutoUpdated(out) | Event::PlayUpdated(out) => {
                    play_ready |= is_play;
                    // A question or a list of mods to remove is a barrier: Play doesn't start
                    // under it, the player presses Play again once it is answered.
                    play_blocked |= is_play && out.blocks_play();
                    if !is_play {
                        m.updating = false;
                    }
                    let AutoOutcome {
                        lines,
                        asks,
                        removals,
                    } = out;
                    for (instance, items) in removals {
                        let entries = self
                            .instances
                            .list
                            .iter()
                            .find(|i| i.id == instance)
                            .map(|i| installed::Installed::load(&i.home).entries)
                            .unwrap_or_default();
                        m.removal
                            .add(&instance, &entries, removal::Removal::of_updates(items));
                    }
                    // The first question waits for its answer; later ones come with the next run.
                    if m.world_ask.is_none() {
                        m.world_ask = asks.into_iter().next();
                    }
                    if !lines.is_empty() {
                        m.message = Some(Ok(lines.join("\n")));
                        m.reload_installed();
                        m.installed_list.reset();
                    }
                }
            }
        }
        if done {
            // A data pack put into a world shows in its list on Home.
            self.world_packs.reread();
        }
        if play_ready {
            self.play_step = None;
            if let Some(intent) = self.play_when_updated.take().filter(|_| !play_blocked) {
                self.resume_play_with(intent, true);
            }
        }
        if let Some(instance) = native_in.filter(|_| !native.is_empty()) {
            self.mods.queue.build_after_install(&instance, native);
        }
        // Installs that waited start in order. The build that finishes them goes through the
        // Build button's entry, so its own planner decides what to build: native mods share a
        // patch library, so one mod is never built alone. A build already running can't take
        // a new mod in, so what was asked for meanwhile waits until it ends, and one
        // follow-up build takes it all up.
        let locked = matches!(
            self.mods_lock(),
            Some(crate::mods::Lock::GameRunning | crate::mods::Lock::Preparing)
        );
        let selected = self
            .selected_instance()
            .map(|i| i.id.clone())
            .unwrap_or_default();
        let step = self.mods.queue.next(
            &selected,
            self.mods.busy.is_some(),
            self.native_step.is_some(),
            locked,
        );
        changed |= step.is_some();
        match step {
            Some(queue::Step::Install(request)) => self.start_install(request),
            Some(queue::Step::Build(ids)) => {
                self.mods.building = ids;
                self.build_native()
            }
            None => {}
        }
        for l in loaded {
            self.mods_loaded(l);
        }
        changed
    }

    /// At start: recover interrupted updates, then update every Sunrise instance's registry
    /// content when enabled ([`auto_update`]), reading the selected instance's list again after.
    pub(super) fn mods_auto_update(&mut self) {
        let enabled = self.settings.auto_updates_mods();
        let native_allowed = self.settings.native_mods_allowed_at.is_some();
        let list: Vec<AutoTarget> = self
            .instances
            .list
            .iter()
            .map(|i| AutoTarget {
                instance: i.clone(),
                game: self.game_of(i),
            })
            .collect();
        let tx = self.mods.tx.clone();
        self.mods.updating = true;
        std::thread::spawn(move || {
            let out = auto_update(&list, enabled, native_allowed, &Default::default());
            let _ = tx.send(Event::AutoUpdated(out));
        });
    }

    /// Turns a Sunrise mod the browser installed on or off, as the game's Options > Mods does.
    pub(super) fn mods_switch(&mut self, e: &installed::Entry) {
        let home = self.mods.home();
        // An update holds the folder: the window doesn't wait for it.
        let Some(_lock) = browser::try_lock_home(&home) else {
            self.mods.message = Some(Err(tr("Wait for the current operation to finish.").into()));
            return;
        };
        let Some(id) = sunrise::mod_id(e) else {
            return;
        };
        let on = !mod_list::is_enabled(&home, id);
        if on
            && native_status::is_native_file(&e.file)
            && self.settings.native_mods_allowed_at.is_none()
        {
            self.pending_consent = Some(PendingConsent::Toggle(e.clone()));
            return;
        }
        let index = sunrise::fetched_index();
        let title = index
            .as_ref()
            .and_then(|index| index.project(&e.project))
            .map_or(id, |project| &project.name);
        let revoked = index.as_ref().and_then(|index| {
            let hash = e.checksum.trim_start_matches("SHA-256:");
            index.revoked(&e.project, hash)
        });
        self.mods.message = Some(match revoked.filter(|_| on) {
            Some(revoked) => Err(format!("{title} was taken down: {}", revoked.reason)),
            None => mod_list::set_enabled(&home, id, on).map(|()| {
                format!(
                    "{title} is {}. The game reads this when it starts.",
                    if on { "on" } else { "off" }
                )
            }),
        });
    }

    /// Installs (or updates) a project on a thread: a pack into its folder, a modpack into
    /// Modpacks. `known` is the project when the page has it; else it is asked for first. `shown`
    /// is the name the page or card that asks shows for it, which the native-code question
    /// repeats; the id stays what is installed.
    pub(super) fn mods_install(
        &mut self,
        source: SourceId,
        id: String,
        known: Option<Project>,
        shown: Option<String>,
    ) {
        // The first native code: the trust notice, before anything is installed. The plan
        // decides, so a content mod that needs a native one asks too.
        let first_native = source == SourceId::Sunrise
            && self.settings.native_mods_allowed_at.is_none()
            && self.brings_native(std::slice::from_ref(&id));
        if first_native {
            self.pending_consent = Some(PendingConsent::Install(source, id, known, shown));
            self.content_scroll
                .handle
                .set_offset(gpui::point(px(0.), px(0.)));
            return;
        }
        // An install the plan refuses because of what installed mods need would break them:
        // one sheet asks first.
        if source == SourceId::Sunrise {
            let broken = self.would_break(std::slice::from_ref(&id));
            if !broken.is_empty() {
                self.mods.break_ask = Some(BreakAsk {
                    source,
                    id,
                    known: known.map(Box::new),
                    shown,
                    broken,
                });
                return;
            }
        }
        let request = queue::Request::One {
            target: self.queue_target(),
            source,
            id,
            known: known.map(Box::new),
            replacing: Vec::new(),
        };
        self.submit_install(request);
    }

    /// The profile an install asked for now is bound to.
    fn queue_target(&self) -> queue::Target {
        queue::Target {
            instance: self
                .selected_instance()
                .map(|i| i.id.clone())
                .unwrap_or_default(),
            home: self.mods.home(),
            game: self.mods_game(),
        }
    }

    /// Behind an install or a build, and behind what already waits: one at a time, in order.
    /// What the queue hands out later starts through [`Self::start_install`] and is not asked
    /// again.
    fn submit_install(&mut self, request: queue::Request) {
        let busy = self.mods.busy.is_some() || self.native_step.is_some();
        if let Some(request) = self.mods.queue.submit(request, busy) {
            self.start_install(request);
        }
    }

    fn start_install(&mut self, request: queue::Request) {
        match request {
            queue::Request::One {
                target,
                source,
                id,
                known,
                replacing,
            } => self.start_install_one(target, source, id, known.map(|p| *p), replacing),
            queue::Request::Many(target, ids, shown) => self.start_install_many(target, ids, shown),
        }
    }

    fn start_install_one(
        &mut self,
        target: queue::Target,
        source: SourceId,
        id: String,
        known: Option<Project>,
        replacing: Vec<String>,
    ) {
        self.mods.message = None;
        self.mods.card_error = None;
        self.mods.busy = Some((id.clone(), format!("Asking {}…", source.label()), 0.));
        self.mods.updates.remove(&(source, id.clone()));
        let everything =
            self.mods.everything || self.mods.browse.filters.other_version(&self.mods.game);
        let tx = self.mods.tx.clone();
        let queue::Target { home, game, .. } = target.clone();
        self.mods.running = Some(target);
        let world = self.mods.world.clone().and_then(|path| {
            let (_, name) = self.mods.worlds.iter().find(|(p, _)| *p == path)?;
            Some((path, name.clone()))
        });
        std::thread::spawn(move || {
            let last = std::cell::Cell::new(-1);
            let progress = |line: &str, f: f32| {
                let pct = (f * 100.) as i32;
                if last.replace(pct) != pct {
                    let _ = tx.send(Event::Progress(line.to_string(), f));
                }
            };
            let result = (|| -> Result<Finished, String> {
                let source = browser::source(source);
                let p = match known {
                    Some(p) => p,
                    None => source.project(&id)?,
                };
                if p.kind == PackKind::DataPack {
                    let (world, name) = world.ok_or_else(|| {
                        tr("Pick the world the data pack goes into first.").to_string()
                    })?;
                    let done = installed::install_datapack(
                        &*source,
                        &p,
                        everything,
                        &world,
                        &game,
                        &|f| progress(&p.title, f),
                    )?;
                    return Ok(Finished::said(trf(
                        "Put {pack} {version} in {world}'s data packs, checksum checked: the \
                             game turns it on when the world opens. {coverage}",
                        &[
                            ("pack", &p.title),
                            ("version", &done.number),
                            ("world", &name),
                            ("coverage", &done.coverage.summary()),
                        ],
                    )));
                }
                if p.kind == PackKind::Modpack && p.source == SourceId::Sunrise {
                    let pack = sunrise::pack::install(&p.id, &progress)?;
                    if let Some(url) = &p.icon {
                        sunrise_launcher::instances::adopt_icon(
                            &sunrise::pack::home(&pack.work),
                            url,
                        );
                    }
                    return Ok(Finished::said(trf(
                        "Installed {pack} {version} as its own profile with {mods}, each \
                             checked. Play it from the list above.",
                        &[
                            ("pack", &pack.name),
                            ("version", &pack.version),
                            ("mods", &pack.mods.join(", ")),
                        ],
                    )));
                }
                let before = installed::Installed::load(&home).entries;
                let priors = undo::capture(&home, &before, &p.id, &game);
                let done = installed::install(
                    &*source, &p, everything, &home, &game, &replacing, &progress,
                )?;
                if p.source == SourceId::Sunrise && p.kind == PackKind::Mod {
                    return Ok(Finished {
                        said: None,
                        native: done
                            .iter()
                            .filter(|e| native_status::is_native_file(&e.file))
                            .map(|e| e.project.clone())
                            .collect(),
                        also: done
                            .iter()
                            .filter(|e| e.project != p.id)
                            .filter(|e| !before.iter().any(|b| b.project == e.project))
                            .map(|e| e.title.clone())
                            .collect(),
                        updated: done
                            .iter()
                            .filter_map(|e| {
                                let was = before.iter().find(|b| b.project == e.project)?;
                                (e.project != p.id && was.number != e.number).then(|| {
                                    (e.title.clone(), was.number.clone(), e.number.clone())
                                })
                            })
                            .collect(),
                        touched: undo::touched(&home, &done, &priors),
                    });
                }
                let names: Vec<String> = done
                    .iter()
                    .map(|e| format!("{} {}", e.title, e.number))
                    .collect();
                Ok(Finished::said(trf(
                    "Installed {mods}, checksums checked. Turn it on in the game: Options \
                         > Resource Packs.",
                    &[("mods", &names.join(", "))],
                )))
            })();
            let _ = tx.send(Event::Done(result));
        });
    }
}

impl Launcher {
    /// The player's answer to the world question. Stay leaves everything as it is; Switch runs
    /// the update again with the mods the question named allowed to go.
    pub(crate) fn answer_world_question(&mut self, answer: sunrise_launcher::worlds::Answer) {
        let Some(ask) = self.mods.world_ask.take() else {
            return;
        };
        if answer == sunrise_launcher::worlds::Answer::Stay {
            return;
        }
        let allow = ask.question.mods.into_iter().collect();
        match ask.then {
            Then::Build(base) => self.build_native_on(Some(base), true),
            Then::Switch(version, channel) => {
                self.commit_switch(&ask.instance, version, channel, allow)
            }
            Then::Install(ask) => self.install_replacing(*ask),
            Then::Update => self.migrate_mods(&ask.instance, allow),
        }
    }

    /// Brings every mod of profile `instance` to the game it plays now, on a worker: the update
    /// of a switch or of a question answered. `allow` are the mods a world may lose.
    pub(super) fn migrate_mods(
        &mut self,
        instance: &str,
        allow: std::collections::BTreeSet<String>,
    ) {
        let Some(instance) = self.instances.list.iter().find(|i| i.id == instance) else {
            return;
        };
        let target = AutoTarget {
            instance: instance.clone(),
            game: self.game_of(instance),
        };
        let native_allowed = self.settings.native_mods_allowed_at.is_some();
        let tx = self.mods.tx.clone();
        self.mods.updating = true;
        std::thread::spawn(move || {
            let out = auto_update(&[target], true, native_allowed, &allow);
            let _ = tx.send(Event::AutoUpdated(out));
        });
    }

    /// A build ended. The mods it left out because they didn't compile or don't fit are
    /// listed to go (`continue_removal`); the build already went on without them. If it stopped
    /// for another reason, every mod it touched is put back as it was, and the ones it added go
    /// at once. A build waiting for the player's answer (worlds, permission) ends nothing yet.
    pub(super) fn install_build_ended(&mut self, end: BuildEnd) {
        if matches!(end, BuildEnd::Pending) {
            return;
        }
        let brought = std::mem::take(&mut self.mods.building);
        let touched = std::mem::take(&mut self.mods.undo);
        if self.selected_instance().is_none_or(|i| i.is_pack_profile()) {
            return;
        }
        let stopped = match end {
            BuildEnd::Stopped(why) => Some(why),
            _ => None,
        };
        let failures = match &stopped {
            Some(_) => Vec::new(),
            None => native_mods::build_failures(&self.native_left_out()),
        };
        match build_cost(&brought, stopped.is_some(), failures) {
            BuildCost::Nothing => {}
            BuildCost::Rollback(ids) => {
                let home = self.mods.home();
                for t in &touched {
                    let _ = t.undo();
                }
                for id in ids
                    .iter()
                    .filter(|id| !touched.iter().any(|t| &t.id == *id))
                {
                    let _ = installed::remove(&home, id);
                }
                self.mods.failed = Some(InstallFailure::Interrupted {
                    ids: touched.iter().map(|t| t.id.clone()).chain(ids).collect(),
                    reason: stopped.clone().unwrap_or_default(),
                });
                self.mods.message = stopped.map(Err);
                self.mods.reload_installed();
            }
            BuildCost::Failures(failures) => {
                let instance = self
                    .selected_instance()
                    .map(|i| i.id.clone())
                    .unwrap_or_default();
                let entries = self
                    .selected_instance()
                    .map(|i| installed::Installed::load(&i.home).entries)
                    .unwrap_or_default();
                let titles: std::collections::HashMap<String, String> = entries
                    .iter()
                    .map(|e| (e.project.clone(), e.title.clone()))
                    .collect();
                let items = failures.into_iter().map(|f| removal::Item {
                    title: titles.get(&f.id).cloned().unwrap_or_else(|| f.id.clone()),
                    id: f.id,
                    reason: f.reason,
                    log: Some(f.log),
                    checksum: String::new(),
                });
                self.mods.removal.add(&instance, &entries, items);
            }
        }
    }

    /// Continue on the removal sheet: the listed mods go. Those whose build failed are
    /// remembered for the cards.
    pub(crate) fn continue_removal(&mut self) {
        let Some(removal) = self.mods.removal.take_first() else {
            return;
        };
        let Some(home) = self
            .instances
            .list
            .iter()
            .find(|i| i.id == removal.instance)
            .map(|i| i.home.clone())
        else {
            return;
        };
        // Under the profile's lock, and only the files that are still the ones listed.
        let _lock = match browser::lock_home(&home) {
            Ok(lock) => lock,
            Err(e) => {
                self.mods.message = Some(Err(e.to_string()));
                return;
            }
        };
        let entries = installed::Installed::load(&home).entries;
        let mut failed = Vec::new();
        for item in &removal.items {
            let same = entries
                .iter()
                .any(|e| e.project == item.id && e.checksum == item.checksum);
            if !same {
                continue;
            }
            if let Err(e) = installed::remove(&home, &item.id) {
                self.mods.message = Some(Err(e));
            } else if let Some(log) = &item.log {
                failed.push(native_mods::BuildFailure {
                    id: item.id.clone(),
                    reason: item.reason.clone(),
                    log: log.clone(),
                });
            }
        }
        if !failed.is_empty() {
            self.mods.failed = Some(InstallFailure::ModsDidNotBuild(failed));
        }
        self.mods.reload_installed();
        self.mods.installed_list.reset();
    }

    /// The installed mods that installing the registry mods `ids` would break, by the index
    /// already read.
    fn would_break(&self, ids: &[String]) -> Vec<(String, String, String)> {
        let entries = installed::Installed::load(&self.mods.home()).entries;
        let held = sunrise::held_mods(&entries);
        let Some(index) = sunrise::known_index() else {
            return Vec::new();
        };
        sunrise::would_break(&index, ids, &held, &self.mods_game())
            .into_iter()
            .map(|b| {
                let title = entries
                    .iter()
                    .find(|e| e.project == b.id)
                    .map_or_else(|| b.id.clone(), |e| e.title.clone());
                (b.id, title, b.needs)
            })
            .collect()
    }

    /// The mods switching the selected profile to the game build `tag` would remove, by the
    /// index already read, the profile's own list and the SDK check of the native mods
    /// (`native_off`). None when no index has been read: that is not "none".
    pub(crate) fn mods_removed_by(
        &self,
        tag: &str,
        native_off: &[(String, String)],
    ) -> Option<Vec<sunrise::auto::Removal>> {
        let (index, instance) = (sunrise::known_index()?, self.selected_instance()?);
        let ctx = sunrise::auto::Context {
            in_pack: sunrise::auto::in_pack(&instance.home),
            game: &backend::played_version_id_by(Some(tag)),
            native_allowed: self.settings.native_mods_allowed_at.is_some(),
        };
        Some(sunrise::auto::removals_at(
            &index,
            &instance.home,
            &ctx,
            native_off,
        ))
    }

    /// The answer to the break sheet: Install goes ahead and removes the mods it would break as
    /// part of the install (all or nothing); Don't install leaves everything as it is. A mod a
    /// saved world uses asks the world question first.
    pub(crate) fn answer_break(&mut self, install: bool) {
        let Some(ask) = self.mods.break_ask.take() else {
            return;
        };
        if !install {
            return;
        }
        let ids: Vec<String> = ask.broken.iter().map(|(id, ..)| id.clone()).collect();
        let instance = self
            .selected_instance()
            .map(|i| i.id.clone())
            .unwrap_or_default();
        if let Ok(Some(question)) = sunrise_launcher::worlds::question(&self.mods.home(), &ids) {
            self.mods.world_ask = Some(WorldAsk {
                instance,
                question,
                then: Then::Install(Box::new(ask)),
            });
            return;
        }
        self.install_replacing(ask);
    }

    /// Queues the install a break sheet asked for, with the mods it would break going in the
    /// same transaction.
    fn install_replacing(&mut self, ask: BreakAsk) {
        let request = queue::Request::One {
            target: self.queue_target(),
            source: ask.source,
            id: ask.id,
            known: ask.known,
            replacing: ask.broken.into_iter().map(|(id, ..)| id).collect(),
        };
        self.submit_install(request);
    }

    /// Whether installing the registry mods `ids` would put native code in, by the index
    /// already read: false when none was.
    fn brings_native(&self, ids: &[String]) -> bool {
        let held = sunrise::held_mods(&installed::Installed::load(&self.mods.home()).entries);
        sunrise::known_index()
            .is_some_and(|index| sunrise::brings_native(&index, ids, &held, &self.mods_game()))
    }

    /// A collection's one Install: the registry mods `ids` planned together, then installed one
    /// after another, each with what it needs, as the mod page's Install does one. A failure
    /// stops the rest and names the mod; the ones before it stay installed. `shown` is the
    /// collection's name.
    pub(super) fn mods_install_many(&mut self, ids: Vec<String>, shown: String) {
        if ids.is_empty() {
            return;
        }
        if self.settings.native_mods_allowed_at.is_none() && self.brings_native(&ids) {
            self.pending_consent = Some(PendingConsent::Collection(ids, shown));
            self.content_scroll
                .handle
                .set_offset(gpui::point(px(0.), px(0.)));
            return;
        }
        let request = queue::Request::Many(self.queue_target(), ids, shown);
        self.submit_install(request);
    }

    fn start_install_many(&mut self, target: queue::Target, ids: Vec<String>, shown: String) {
        self.mods.message = None;
        self.mods.busy = Some((
            shown.clone(),
            tr("Asking the Sunrise registry…").to_string(),
            0.,
        ));
        let tx = self.mods.tx.clone();
        let queue::Target { home, game, .. } = target.clone();
        self.mods.running = Some(target);
        std::thread::spawn(move || {
            let native = std::cell::RefCell::new(Vec::new());
            let touched = std::cell::RefCell::new(Vec::new());
            let result = (|| -> Result<Finished, String> {
                // The whole set is planned once first: a conflict between picks refuses all of
                // them before anything downloads. Each is then installed with what it needs.
                let index = sunrise::fetched_index().ok_or_else(|| {
                    tr("Connect to the Sunrise registry before installing a mod and its dependencies.")
                        .to_string()
                })?;
                let held = sunrise::held_mods(&installed::Installed::load(&home).entries);
                sunrise::check_set(&index, &ids, &held, &game)?;
                let source = browser::source(SourceId::Sunrise);
                let total = ids.len() as f32;
                crate::mods::run_in_order(&ids, |i, id| {
                    let p = source.project(id)?;
                    let progress = |line: &str, f: f32| {
                        let _ = tx.send(Event::Progress(line.to_string(), (i as f32 + f) / total));
                    };
                    let before = installed::Installed::load(&home).entries;
                    let priors = undo::capture(&home, &before, &p.id, &game);
                    installed::install(&*source, &p, false, &home, &game, &[], &progress)
                        .map(|done| {
                            touched
                                .borrow_mut()
                                .extend(undo::touched(&home, &done, &priors));
                            native.borrow_mut().extend(
                                done.iter()
                                    .filter(|e| native_status::is_native_file(&e.file))
                                    .map(|e| e.project.clone()),
                            );
                            done.into_iter().map(|e| e.title).collect()
                        })
                        .map_err(|why| {
                            trf(
                                "{mod} didn't install: {why}",
                                &[("mod", &p.title), ("why", &why)],
                            )
                        })
                })?;
                Ok(Finished {
                    said: None,
                    native: native.take(),
                    also: Vec::new(),
                    updated: Vec::new(),
                    touched: touched.take(),
                })
            })();
            // The members that went in before one failed stay installed: the native ones among
            // them are still built.
            if result.is_err() {
                let _ = tx.send(Event::Brought(native.take(), touched.take()));
            }
            let _ = tx.send(Event::Done(result));
        });
    }
}

/// An instance whose registry projects may update.
pub(super) struct AutoTarget {
    pub(super) instance: instances::Instance,
    /// The Minecraft version its own build plays ([`Launcher::game_of`]).
    pub(super) game: String,
}

/// Updates the registry projects of `targets`: Sunrise's own instances only, never a modpack's
/// profile, whose pack pins its mods. Returns what happened for the page's notice.
pub(super) fn auto_update(
    targets: &[AutoTarget],
    enabled: bool,
    native_allowed: bool,
    allow: &std::collections::BTreeSet<String>,
) -> AutoOutcome {
    let mut out = AutoOutcome::default();
    for t in targets {
        let i = &t.instance;
        if i.is_pack_profile() {
            continue;
        }
        let ctx = sunrise::auto::Context {
            in_pack: sunrise::auto::in_pack(&i.home),
            game: &t.game,
            native_allowed,
        };
        if let Some(report) = sunrise::auto::run(&i.home, &ctx, enabled, allow, &|_, _| {}) {
            out.lines.extend(report.lines());
            if !report.remove.is_empty() {
                out.removals.push((i.id.clone(), report.remove));
            }
            if let Some(question) = report.ask {
                out.asks.push(WorldAsk {
                    instance: i.id.clone(),
                    question,
                    then: Then::Update,
                });
            }
        }
    }
    out
}

/// What the automatic updates did: the notice's lines, and the questions to put to the player.
#[derive(Default)]
pub(crate) struct AutoOutcome {
    pub(crate) lines: Vec<String>,
    pub(crate) asks: Vec<WorldAsk>,
    /// The mods that can't stay on the game a profile plays, by instance: listed for the
    /// player to continue.
    pub(crate) removals: Vec<(String, Vec<sunrise::auto::Removal>)>,
}

impl AutoOutcome {
    /// A question to answer or mods to confirm for removal: Play doesn't start under either.
    fn blocks_play(&self) -> bool {
        !self.asks.is_empty() || !self.removals.is_empty()
    }
}

/// What went wrong for mod `id` in the last install's build, for its card: the reason in one
/// plain line and the whole text for Details (a compile failure's log).
pub(crate) fn build_failure_of(
    failed: Option<&InstallFailure>,
    id: &str,
) -> Option<(String, String)> {
    match failed? {
        InstallFailure::ModsDidNotBuild(failures) => failures
            .iter()
            .find(|f| f.id == id)
            .map(|f| (f.reason.clone(), f.log.clone())),
        InstallFailure::Interrupted { ids, reason } if ids.iter().any(|i| i == id) => Some((
            crate::mods::install_state::plain_failure(reason),
            reason.clone(),
        )),
        InstallFailure::Interrupted { .. } => None,
    }
}

/// The display names of the mods `ids`, from the installed `entries`; the id when the list
/// doesn't have it.
pub(crate) fn titles(entries: &[installed::Entry], ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|id| {
            entries
                .iter()
                .find(|e| &e.project == id)
                .map_or_else(|| id.clone(), |e| e.title.clone())
        })
        .collect()
}

/// What goes on once the player answers Switch anyway to a world question.
pub(crate) enum Then {
    /// The mods are updated again with the ones named allowed to go.
    Update,
    /// A build that stopped for the question is made again, leaving those mods out: the build
    /// it was on.
    Build(String),
    /// A switch of build: the pin and channel it would save, saved only now.
    Switch(Option<String>, instances::Channel),
    /// An install that removes mods a world uses.
    Install(Box<BreakAsk>),
}

/// A switch, update or install that waits for the player's answer: it would remove mods that
/// saved worlds use. Staying is the default; Switch anyway goes on with those mods allowed.
pub(crate) struct WorldAsk {
    /// The instance whose profile it is.
    pub(crate) instance: String,
    pub(crate) question: sunrise_launcher::worlds::Question,
    pub(crate) then: Then,
}

impl WorldAsk {
    /// The question a build's stop for worlds puts: the mods it would have left out and the
    /// worlds saved with them.
    pub(crate) fn from_stop(instance: String, stake: &native_mods::AtStake) -> Self {
        Self {
            instance,
            question: sunrise_launcher::worlds::Question {
                mods: stake.mods.clone(),
                worlds: stake.worlds.clone(),
            },
            then: Then::Build(stake.base.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
