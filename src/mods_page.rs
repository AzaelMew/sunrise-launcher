//! The Mods page: its tabs are the kinds (mods, modpacks, resource packs, shader packs, data
//! packs). Search Sunrise's own registry for what was made for Sunrise, or Modrinth (CurseForge:
//! coming soon) for Java mods, modpacks, resource packs and data packs (a Java instance's files
//! always come from Modrinth, for its version), see where each comes
//! from and how well it runs (a Java mod's jar is checked first; a Sunrise mod says whether it is
//! signed and what it may do), install it with its checksum checked, turn Sunrise mods on and
//! off, and update or remove what was installed (`sunrise_launcher::browser`). Imported modpacks
//! are listed on its Modpacks tab.
use super::line_edit::LineEdit;
use super::*;
use java_instance::Target;
use std::collections::HashMap;
use std::path::PathBuf;
use sunrise_launcher::browser::{
    self, installed, java, java_instance, sunrise, Kind as PackKind, Project, SourceId,
};
use sunrise_launcher::modpacks;
use sunrise_launcher::native_status;
use sunrise_launcher::porter::Check;

/// Java mods checked at once.
const CHECKING: usize = 2;

#[path = "glass_mods.rs"]
mod glass_mods;

pub(super) enum Event {
    Progress(String, f32),
    Done(Result<String, String>, bool),
    /// A Java mod's check (its jar read, or why it couldn't be), for the Minecraft version it picked the jar for.
    Checked((SourceId, String), String, Result<Box<Check>, String>),
    /// What the automatic updates did (`sunrise::auto`).
    AutoUpdated(Vec<String>),
    /// A project page's part, an image or a filter's values (`mods_project`).
    Loaded(super::mods_project::Loaded),
}

/// Where a Java mod's check is.
pub(super) enum Checking {
    Running,
    Done(Box<Check>),
    Failed(String),
}

pub(super) struct Mods {
    pub(super) kind: PackKind,
    pub(super) source: SourceId,
    text: LineEdit,
    focus: FocusHandle,
    pub(super) focused: bool,
    everything: bool,
    /// The Minecraft version Sunrise plays, read again with each search ([`browser::game`]).
    pub(super) game: String,
    /// Which search is the current one; answers for older ones are dropped.
    search: u64,
    searching: bool,
    /// A new list is on its way to replace the rows shown, which dim meanwhile.
    refreshing: bool,
    searched: bool,
    results: Vec<Project>,
    results_list: cull::Cull,
    installed_list: cull::Cull,
    pub(super) report_list: cull::Cull,
    more: bool,
    error: Option<String>,
    /// The failed search was Sunrise's own registry (its card offers Modrinth instead).
    error_sunrise: bool,
    /// The failed search's own words are shown under its plain message.
    pub(super) error_details: bool,
    pub(super) installed: Vec<installed::Entry>,
    /// The registry native mods' versions the player picked on their Versions tab, by id
    /// (`sunrise::chosen`), read with `installed`.
    pub(super) chosen: std::collections::BTreeMap<String, String>,
    /// The Sunrise the selected instance played when its updates were last looked for.
    pub(super) checked_for: Option<String>,
    /// The number of the latest look for updates: only its answer is taken ([`offer_updates`]).
    updates_asked: u64,
    /// The build the page's Minecraft version was last worked out for (the selected instance's).
    game_for: Option<(Option<String>, Option<String>)>,
    /// Whether the start's auto-update ran: it waits for the release list.
    pub(super) auto_updated: bool,
    loaded: bool,
    /// Whether the page searched for the instance as it is (its version and loader).
    asked: bool,
    /// The game folder the browser installs into and lists: the selected instance's.
    home: Option<PathBuf>,
    /// The selected Java instance's version and loader: its mods and resource packs are the
    /// source's own files for them, put in as they are.
    pub(super) java: Option<Target>,
    /// Its worlds (folder, name), the last played first, and the one data packs go into.
    worlds: Vec<(PathBuf, String)>,
    world: Option<PathBuf>,
    /// The project being installed, and the line and fraction of its download.
    pub(super) busy: Option<(String, String, f32)>,
    pub(super) message: Option<Result<String, String>>,
    pub(super) updates: HashMap<(SourceId, String), String>,
    confirm_remove: Option<String>,
    /// The project the glass look's detail card shows; the first result when none is.
    pub(super) picked: Option<String>,
    /// A project was just picked where its card sits under the list (a narrow window): the page
    /// scrolls to the card once it is laid out (`glass_mods::reveal`).
    pub(super) reveal: std::rc::Rc<glass_mods::Reveal>,
    /// The project the glass detail card showed last.
    pub(super) shown: Option<Project>,
    /// An install awaiting explicit native-code consent, before any download.
    /// The selected instance's native build state, the mods it would build again
    /// (`native_status::unbuilt`), and the `native_status::fingerprint` they were read for: read
    /// again only when that changes.
    pub(super) native_state:
        std::cell::RefCell<Option<(String, native_status::State, Vec<String>)>>,
    /// Native routes read by a worker for this instance's files, consent and game version.
    pub(super) native_routes: std::cell::RefCell<Option<crate::build_mode::RouteMemo>>,
    /// What updating the native mods to a newer installed game would do, read in the background.
    pub(super) native_update: std::cell::RefCell<Option<crate::build_mode::UpdateMemo>>,
    /// The update the player chose to stay off (its memo key): its summary isn't shown again.
    pub(super) update_stayed: std::cell::RefCell<Option<String>>,
    /// Installed wasm declarations, reread only when the instance's files or enable list change.
    /// The mod whose route menu is open.
    pub(super) route_open: Option<String>,
    /// The instance and the mods whose compile failed in the last stop: their chips say so until
    /// the next build attempt, whatever the player chose.
    pub(super) unbuilt: Option<(String, Vec<String>)>,
    native_files: std::cell::RefCell<Option<(String, Vec<native_status::Installed>)>>,
    /// Java mods' checks, by source and project.
    pub(super) checks: HashMap<(SourceId, String), Checking>,
    /// Sunrise registry projects' trust and capabilities, by id ([`sunrise::trust_and_capabilities`]).
    pub(super) about: HashMap<String, Vec<String>>,
    /// The project page, filters and images.
    pub(super) browse: super::mods_project::Browse,
    pub(super) tx: Sender<Event>,
    rx: Receiver<Event>,
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
    /// without reading or writing anything: the alternative Mods screens (`alt_mods`) call it so
    /// that a native-code consent answered later continues in the folder they show.
    pub(super) fn follow_home(&mut self, home: Option<PathBuf>) {
        if home.is_some() && self.home != home {
            self.home = home;
        }
    }

    pub(super) fn home(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(backend::game_home)
    }

    pub(super) fn new(cx: &mut App) -> Self {
        let (tx, rx) = crate::wake::channel();
        let page = std::env::var("SUNRISE_LAUNCHER_PAGE");
        let kind = match page.as_deref() {
            Ok("modpacks") => PackKind::Modpack,
            Ok("resourcepacks") => PackKind::ResourcePack,
            Ok("shaders") => PackKind::Shader,
            Ok("datapacks") => PackKind::DataPack,
            _ => PackKind::Mod,
        };
        Self {
            kind,
            source: source_for(kind),
            text: LineEdit::new(String::new(), 100, 40),
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            focused: false,
            everything: false,
            game: browser::game(),
            search: 0,
            searching: false,
            refreshing: false,
            searched: false,
            results: Vec::new(),
            results_list: cull::Cull::default(),
            installed_list: cull::Cull::default(),
            report_list: cull::Cull::default(),
            more: false,
            error: None,
            error_sunrise: false,
            error_details: false,
            installed: Vec::new(),
            chosen: Default::default(),
            checked_for: None,
            updates_asked: 0,
            game_for: None,
            auto_updated: false,
            loaded: false,
            asked: false,
            home: None,
            java: None,
            worlds: Vec::new(),
            world: None,
            busy: None,
            message: None,
            native_state: Default::default(),
            native_routes: Default::default(),
            native_update: Default::default(),
            update_stayed: Default::default(),
            route_open: None,
            unbuilt: None,
            native_files: Default::default(),
            updates: HashMap::new(),
            confirm_remove: None,
            picked: None,
            reveal: Default::default(),
            shown: None,
            checks: HashMap::new(),
            about: HashMap::new(),
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
        let mut to_check = Vec::new();
        let mut loaded = Vec::new();
        let mut done = false;
        while let Ok(event) = self.mods.rx.try_recv() {
            changed = true;
            let m = &mut self.mods;
            match event {
                Event::Progress(line, f) => {
                    if let Some(busy) = &mut m.busy {
                        busy.1 = line;
                        busy.2 = f;
                    }
                }
                Event::Done(result, modpack) => {
                    done = true;
                    m.busy = None;
                    if let Ok(said) = &result {
                        crate::glass_toast::say(said.clone());
                    }
                    m.message = Some(result);
                    m.reload_installed();
                    m.installed_list.reset();
                    if modpack {
                        // The Modpacks page reads its list again.
                        self.packs_loaded = None;
                    }
                }
                // A check for another Minecraft version than the page's (the instance moved) is dropped.
                Event::Checked(key, game, result) if game == m.game => {
                    let state = match result {
                        Ok(c) => Checking::Done(c),
                        Err(e) => Checking::Failed(e),
                    };
                    m.checks.insert(key, state);
                }
                // Its worker is done: the project is checked again, for the page's version, without two workers on one jar.
                Event::Checked(key, ..) => {
                    m.checks.remove(&key);
                    if let Some(p) = m
                        .results
                        .iter()
                        .find(|p| (p.source, &p.id) == (key.0, &key.1))
                    {
                        to_check.push(p.clone());
                    }
                }
                Event::Loaded(l) => loaded.push(l),
                Event::AutoUpdated(lines) => {
                    if !lines.is_empty() {
                        m.message = Some(Ok(lines.join("\n")));
                        m.reload_installed();
                        m.installed_list.reset();
                    }
                }
            }
        }
        // `SUNRISE_LAUNCHER_PROJECT=<slug>` (with `SUNRISE_LAUNCHER_PAGE=mods`): opens that
        // project's page once a search finds it, for screenshots. Once per run, kept in a flag
        // rather than by removing the variable: `remove_var` races `getenv` on the download, check
        // and keyring threads running by now (unsound, and `unsafe` from edition 2024).
        static PROJECT_OPENED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if self.mods.browse.open.is_none()
            && !PROJECT_OPENED.load(std::sync::atomic::Ordering::Relaxed)
        {
            if let Ok(want) = std::env::var("SUNRISE_LAUNCHER_PROJECT") {
                if let Some(p) = self
                    .mods
                    .results
                    .iter()
                    .find(|p| p.id == want || p.slug == want)
                    .cloned()
                {
                    PROJECT_OPENED.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.mods_open(p);
                }
            }
        }
        self.mods_check(to_check);
        if done {
            // A data pack put into a world shows in its list on Home.
            self.world_packs.reread();
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
        std::thread::spawn(move || {
            let lines = auto_update(&list, enabled);
            if !lines.is_empty() {
                let _ = tx.send(Event::AutoUpdated(lines));
            }
        });
    }

    /// Turns a Sunrise mod the browser installed on or off, as the game's Options > Mods does;
    /// in a Java instance, renames its file.
    pub(super) fn mods_switch(&mut self, e: &installed::Entry) {
        let home = self.mods.home();
        // An update holds the folder: the window doesn't wait for it.
        let Some(_lock) = browser::try_lock_home(&home) else {
            self.mods.message = Some(Err(tr("Wait for the current operation to finish.").into()));
            return;
        };
        if self.mods.java.is_some() {
            let on = !java_instance::is_on(&home, e);
            self.mods.message = Some(java_instance::set_on(&home, e, on).map(|()| {
                format!(
                    "{} is {}. The game reads this when it starts.",
                    e.title,
                    if on { "on" } else { "off" }
                )
            }));
            return;
        }
        let Some(id) = sunrise::mod_id(e) else {
            return;
        };
        let on = !java::is_enabled(&home, id);
        if on
            && native_status::is_native_file(&e.file)
            && self.settings.native_mods_allowed_at.is_none()
        {
            self.pending_consent = Some(PendingConsent::Toggle(e.clone()));
            return;
        }
        let revoked = sunrise::known_index().and_then(|index| {
            let hash = e.checksum.trim_start_matches("SHA-256:");
            index.revoked(&e.project, hash).map(|r| r.reason.clone())
        });
        self.mods.message = Some(match revoked.filter(|_| on) {
            Some(reason) => Err(format!("{} was taken down: {reason}", e.title)),
            None => java::set_enabled(&home, id, on).map(|()| {
                format!(
                    "{} is {}. The game reads this when it starts.",
                    e.title,
                    if on { "on" } else { "off" }
                )
            }),
        });
    }

    /// Checks the Java mods among `found` that aren't checked yet, a few at a time, on threads:
    /// a cached check answers at once, else the jar is downloaded and read.
    fn mods_check(&mut self, found: Vec<Project>) {
        let m = &mut self.mods;
        if m.java.is_some() {
            return;
        }
        // Each project is claimed as it is taken, so one listed twice (a result and a re-queued check) gets one worker.
        let mut todo = Vec::new();
        for p in found {
            let key = (p.source, p.id.clone());
            if p.kind == PackKind::Mod
                && p.source != SourceId::Sunrise
                && p.support(&m.game).installable()
                && !m.checks.contains_key(&key)
            {
                m.checks.insert(key, Checking::Running);
                todo.push(p);
            }
        }
        if todo.is_empty() {
            return;
        }
        // A version other than 26.3 asked for: check the newest file for 1.21 and later.
        let everything = m.everything || m.browse.filters.other_version(&m.game);
        let game = m.game.clone();
        let tx = m.tx.clone();
        let queue = std::sync::Mutex::new(todo.into_iter());
        std::thread::spawn(move || {
            // Checks are kept in the default game folder, shared by every instance.
            let home = backend::game_home();
            std::thread::scope(|s| {
                for _ in 0..CHECKING {
                    s.spawn(|| {
                        while let Some(p) = queue.lock().ok().and_then(|mut q| q.next()) {
                            let result = browser::source(p.source)
                                .ok_or_else(|| {
                                    trf("{source} isn't set up.", &[("source", p.source.label())])
                                })
                                .and_then(|source| {
                                    java::check_project(
                                        &home,
                                        &*source,
                                        &p,
                                        everything,
                                        &game,
                                        &|_| {},
                                    )
                                });
                            // The project's page, if open, shows the verdict beside its version.
                            if let Ok((version, check)) = &result {
                                let key = (p.source, p.id.clone());
                                let _ =
                                    tx.send(Event::Loaded(super::mods_project::Loaded::Checked(
                                        key,
                                        version.id.clone(),
                                        check.verdict,
                                    )));
                            }
                            let result = result.map(|(_, check)| Box::new(check));
                            let _ = tx.send(Event::Checked((p.source, p.id), game.clone(), result));
                        }
                    });
                }
            });
        });
    }

    /// Installs (or updates) a project on a thread: a pack into its folder, a modpack into
    /// Modpacks. `known` is the project when the page has it; else it is asked for first.
    pub(super) fn mods_install(&mut self, source: SourceId, id: String, known: Option<Project>) {
        if self.mods.busy.is_some() {
            return;
        }
        // The first native mod: the trust notice, before anything is installed.
        let first_native = source == SourceId::Sunrise
            && self.settings.native_mods_allowed_at.is_none()
            && sunrise::is_native_mod(&id);
        if first_native {
            self.pending_consent = Some(PendingConsent::Install(source, id, known));
            self.content_scroll
                .handle
                .set_offset(gpui::point(px(0.), px(0.)));
            return;
        }
        self.mods.message = None;
        self.mods.busy = Some((id.clone(), format!("Asking {}…", source.label()), 0.));
        self.mods.updates.remove(&(source, id.clone()));
        let everything =
            self.mods.everything || self.mods.browse.filters.other_version(&self.mods.game);
        // A modpack is ported for the selected version as soon as it is imported.
        let tag = self.selected().map(|tag| tag.to_string());
        let tx = self.mods.tx.clone();
        let home = self.mods.home();
        let game = self.mods_game();
        let java = self.mods.java.clone();
        let instance = self
            .selected_instance()
            .map_or_else(|| "Sunrise".to_string(), |i| i.name.clone());
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
            let mut modpack = false;
            let result = browser::source(source)
                .ok_or_else(|| trf("{source} isn't set up.", &[("source", source.label())]))
                .and_then(|source| {
                    let p = match known {
                        Some(p) => p,
                        None => source.project(&id)?,
                    };
                    if let Some(target) = java.as_ref().filter(|t| t.takes(p.kind)) {
                        let done = java_instance::install(&*source, target, &p, &home, &progress)?;
                        let names: Vec<String> = done
                            .iter()
                            .map(|e| format!("{} {}", e.title, e.number))
                            .collect();
                        return Ok(trf(
                            "Installed {mods}, checksums checked.",
                            &[("mods", &names.join(", "))],
                        ));
                    }
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
                        return Ok(trf(
                            "Put {pack} {version} in {world}'s data packs, checksum checked: the \
                             game turns it on when the world opens. {coverage}",
                            &[
                                ("pack", &p.title),
                                ("version", &done.number),
                                ("world", &name),
                                ("coverage", &done.coverage.summary()),
                            ],
                        ));
                    }
                    modpack = p.kind == PackKind::Modpack;
                    if modpack && p.source == SourceId::Sunrise {
                        let pack = sunrise::pack::install(&p.id, &progress)?;
                        if let Some(url) = &p.icon {
                            sunrise_launcher::instances::adopt_icon(
                                &sunrise::pack::home(&pack.work),
                                url,
                            );
                        }
                        return Ok(trf(
                            "Installed {pack} {version} as its own profile with {mods}, each \
                             checked. Play it from the list above.",
                            &[
                                ("pack", &pack.name),
                                ("version", &pack.version),
                                ("mods", &pack.mods.join(", ")),
                            ],
                        ));
                    }
                    if modpack {
                        let folder = modpacks::ports_dir().join("downloads");
                        let file = installed::fetch_modpack(&*source, &p, &folder, &game, &|f| {
                            progress(&p.title, f)
                        })?;
                        let log = |line: String| progress(&line, 1.);
                        let porter = modpacks::Porter::for_version(tag.as_deref())?;
                        let pack = modpacks::import_and_port(&porter, &file, &log)?;
                        if let Some(url) = &p.icon {
                            sunrise_launcher::instances::adopt_icon(
                                &modpacks::profile_home(&pack.work),
                                url,
                            );
                        }
                        return Ok(format!(
                            "{} {}",
                            modpacks::outcome(&pack),
                            tr("Play it from the list above.")
                        ));
                    }
                    let done =
                        installed::install(&*source, &p, everything, &home, &game, &progress)?;
                    let names: Vec<String> = done
                        .iter()
                        .map(|e| format!("{} {}", e.title, e.number))
                        .collect();
                    let mods = names.join(", ");
                    if done.iter().any(|e| native_status::is_native_file(&e.file)) {
                        return Ok(trf(
                            "Installed {mods} into {instance}. Build to play with it.",
                            &[("mods", &mods), ("instance", &instance)],
                        ));
                    }
                    if p.source == SourceId::Sunrise && p.kind == PackKind::Mod {
                        return Ok(trf(
                            "Installed {mods} into sunrise-mods and turned it on, checksum \
                             checked (Options > Mods in the game).",
                            &[("mods", &mods)],
                        ));
                    }
                    if p.kind == PackKind::Mod {
                        let skipped = done.last().map_or(0, |e| e.skipped.len());
                        let ported = trf(
                            "Ported {mods} into sunrise-mods and turned on (Options > Mods in \
                             the game).",
                            &[("mods", &mods)],
                        );
                        return Ok(if skipped > 0 {
                            format!(
                                "{ported} {}",
                                tr("What didn't port is listed under Installed.")
                            )
                        } else {
                            ported
                        });
                    }
                    Ok(trf(
                        "Installed {mods}, checksums checked. Turn it on in the game: Options \
                         > Resource Packs.",
                        &[("mods", &mods)],
                    ))
                });
            let _ = tx.send(Event::Done(result, modpack));
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
pub(super) fn auto_update(targets: &[AutoTarget], enabled: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for t in targets {
        let i = &t.instance;
        let pack = i.id.starts_with("pack:") || i.id.starts_with("port:");
        let (false, None) = (pack, &i.java) else {
            continue;
        };
        let ctx = sunrise::auto::Context {
            in_pack: sunrise::auto::in_pack(&i.home),
            game: &t.game,
        };
        if let Some(report) = sunrise::auto::run(&i.home, &ctx, enabled, &|_, _| {}) {
            lines.extend(report.lines());
        }
    }
    lines
}

/// Where a kind's tab searches first: Sunrise's registry for mods and shader packs, Modrinth for
/// the rest.
fn source_for(kind: PackKind) -> SourceId {
    match kind {
        PackKind::Mod | PackKind::Shader => SourceId::Sunrise,
        PackKind::ResourcePack | PackKind::Modpack | PackKind::DataPack => SourceId::Modrinth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
