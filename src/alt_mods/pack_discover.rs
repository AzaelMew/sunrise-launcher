//! Discover's resource packs and data packs, searched on Modrinth only and installed through the
//! browser's checked path (`browser::installed`: checksum before anything is put in place).
//!
//! **Scope.** Only vanilla resource packs and data packs: no Java mods, modpacks or shader packs
//! come from here, and a hit of any other kind or source is dropped ([`keep`]).
//!
//! **Where a pack goes.** A resource pack goes into the selected setup's `resourcepacks/`. A data
//! pack goes into one *explicit* world of the selected setup: the one Home's "Data packs" opened
//! Discover for ([`Launcher::alt_discover_for_world`]). There is no default world and the
//! world list is never read to pick one. The target is checked again when the player presses
//! Add and once more in the worker, under the setup's lock ([`check_world`]).
//!
//! **Nothing else changes.** Installing never turns a pack on, starts the game or edits options;
//! it stays blocked while the game or another change runs, and holds Play and setup changes
//! through the shared busy flag (`alt_mutating`).
use super::ui::{c, card, dim, muted, tag, tile};
use super::*;
use crate::alt::{button, LINE, MUTED, TEXT};
use std::path::Path;
use sunrise_launcher::browser::{self, images, installed, Kind, Project, Query};
use sunrise_launcher::worlds;

/// How many hits one search asks for.
const LIMIT: usize = 24;
/// The longest search text sent.
const TEXT_MAX: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Scope {
    /// The Sunrise registry's mods (the existing Discover).
    #[default]
    Mods,
    Resource,
    Data,
    /// Iris-format zips from Modrinth and Sunrise's own `.sshader` packs from its registry.
    Shader,
}

impl Scope {
    pub(super) const ALL: [Scope; 4] = [Scope::Mods, Scope::Resource, Scope::Data, Scope::Shader];

    pub(super) fn label(self) -> &'static str {
        match self {
            Scope::Mods => tr("Mods"),
            Scope::Resource => tr("Resource packs"),
            Scope::Data => tr("Data packs"),
            Scope::Shader => tr("Shader packs"),
        }
    }

    /// The kind searched on Modrinth; none for the registry's mods.
    fn kind(self) -> Option<Kind> {
        match self {
            Scope::Mods => None,
            Scope::Resource => Some(Kind::ResourcePack),
            Scope::Data => Some(Kind::DataPack),
            Scope::Shader => Some(Kind::Shader),
        }
    }
}

/// The world a data pack will go into, chosen explicitly for one setup.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    /// The setup (instance id) and its game folder the world was chosen in.
    pub(super) instance: String,
    pub(super) home: PathBuf,
    pub(super) world: PathBuf,
    pub(super) name: String,
}

pub(super) enum Status {
    Idle,
    Searching,
    Done,
    Failed(String),
}

pub(super) struct PackState {
    pub(super) scope: Scope,
    pub(super) results: Vec<Project>,
    pub(super) status: Status,
    /// The number of the latest search: only its answer is taken.
    pub(super) seq: u64,
    pub(super) target: Option<Target>,
}

impl Default for PackState {
    fn default() -> Self {
        Self {
            scope: Scope::Mods,
            results: Vec::new(),
            status: Status::Idle,
            seq: 0,
            target: None,
        }
    }
}

/// The restricted Modrinth query for `scope`: its one kind, what runs on `game`, a bounded text
/// and page. None for the registry's mods, which don't use Modrinth.
pub(super) fn query(scope: Scope, text: &str, game: &str) -> Option<Query> {
    let mut q = Query::new(text, scope.kind()?);
    q.text = q.text.chars().take(TEXT_MAX).collect();
    q.game = game.to_owned();
    q.everything = false;
    q.limit = LIMIT;
    Some(q)
}

/// The hits a scope shows: Modrinth's, of the scope's kind only (a mod, modpack or other kind
/// that a source answered anyway is dropped), each project once. Shader packs also come from
/// Sunrise's registry (its own `.sshader` packs), and from there only.
pub(super) fn keep(scope: Scope, hits: Vec<Project>) -> Vec<Project> {
    let Some(kind) = scope.kind() else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    hits.into_iter()
        .filter(|p| p.kind == kind)
        .filter(|p| {
            p.source == SourceId::Modrinth
                || (p.source == SourceId::Sunrise && kind == Kind::Shader)
        })
        .filter(|p| seen.insert((p.source, p.id.clone())))
        .collect()
}

/// Whether `world` is, right now, an existing world of the setup whose game folder is `home`:
/// a real folder directly in `saves/`, with neither it, `saves/` nor `datapacks/` a link to
/// somewhere else. Gives the world's name.
pub(super) fn check_world(home: &Path, world: &Path) -> Result<String, String> {
    let stale = || tr("That world is no longer in this setup. Open its Data packs again.");
    let plain_dir = |p: &Path| {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    };
    let saves = home.join("saves");
    if world.parent() != Some(saves.as_path()) || !plain_dir(&saves) || !plain_dir(world) {
        return Err(stale().to_string());
    }
    let datapacks = world.join("datapacks");
    match std::fs::symlink_metadata(&datapacks) {
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
            return Err(tr("This world's datapacks folder isn't a plain folder.").to_string());
        }
        _ => {}
    }
    worlds::list(home)
        .into_iter()
        .find(|w| w.path == world)
        .map(|w| w.name)
        .ok_or_else(|| stale().to_string())
}

/// Whether `target` still belongs to the setup `instance` with game folder `home`.
pub(super) fn target_belongs(target: &Target, instance: &str, home: &Path) -> bool {
    target.instance == instance && target.home == home
}

impl Launcher {
    /// The selected setup that packs may go into: a Sunrise one, with its folder readable.
    fn pack_setup(&self) -> Option<(String, PathBuf)> {
        self.selected_instance()
            .filter(|i| i.java.is_none() && self.location_error.is_none())
            .map(|i| (i.id.clone(), i.home.clone()))
    }

    /// Records `world` of the selected setup as the data pack target, after checking it.
    fn pack_set_target(&mut self, world: PathBuf) -> Result<(), String> {
        let (instance, home) = self
            .pack_setup()
            .ok_or_else(|| tr("Select a Sunrise setup first.").to_string())?;
        let name = check_world(&home, &world)?;
        self.alt.mods.packs.target = Some(Target {
            instance,
            home,
            world,
            name,
        });
        Ok(())
    }

    /// Switches Discover to `scope`. A new scope never keeps a world target: only
    /// [`Self::alt_discover_for_world`] sets one.
    pub(super) fn alt_pack_scope(&mut self, scope: Scope) {
        let p = &mut self.alt.mods.packs;
        if p.scope == scope {
            return;
        }
        p.scope = scope;
        p.target = None;
        p.results.clear();
        p.status = Status::Idle;
        p.seq += 1;
        self.alt.mods.menu = None;
        if scope != Scope::Mods {
            self.alt_pack_search();
        }
    }

    /// The Home child's entry: Discover for data packs, aimed at `world` of the selected setup.
    /// Validates the world, then searches. Changes no file.
    pub(crate) fn alt_discover_for_world(&mut self, world: PathBuf) {
        self.alt.page = Page::Discover;
        // Discard any prior setup's snapshot before recording this explicit world.
        self.alt_mods_refresh();
        self.alt.mods.menu = None;
        let p = &mut self.alt.mods.packs;
        p.scope = Scope::Data;
        p.target = None;
        p.results.clear();
        p.status = Status::Idle;
        p.seq += 1;
        match self.pack_set_target(world) {
            Ok(()) => self.alt_dismiss_notice(),
            Err(why) => self.alt.mods.notice = Some(Err(why)),
        }
        self.alt_pack_search();
    }

    /// The setup changed: nothing chosen for the old one carries over.
    pub(super) fn alt_pack_setup_moved(&mut self) {
        let p = &mut self.alt.mods.packs;
        p.target = None;
        p.results.clear();
        p.status = Status::Idle;
        p.seq += 1;
    }

    /// Searches Modrinth for the scope's kind with the header's text, in a worker.
    pub(super) fn alt_pack_search(&mut self) {
        let text = self.alt.mods.find.text().to_owned();
        let game = self.mods_game();
        let p = &mut self.alt.mods.packs;
        let Some(q) = query(p.scope, &text, &game) else {
            return;
        };
        p.seq += 1;
        p.status = Status::Searching;
        let (seq, scope, tx) = (p.seq, p.scope, self.alt.mods.tx.clone());
        std::thread::spawn(move || {
            let ask = |id| {
                browser::source(id)
                    .ok_or_else(|| tr("Modrinth isn't set up.").to_string())
                    .and_then(|s| s.search(&q))
            };
            let modrinth = ask(SourceId::Modrinth);
            // Sunrise's own shader packs lead; the registry failing leaves Modrinth's.
            let mut hits = match scope {
                Scope::Shader => ask(SourceId::Sunrise).unwrap_or_default(),
                _ => Vec::new(),
            };
            let result = match modrinth {
                Ok(found) => {
                    hits.extend(found);
                    Ok(keep(scope, hits))
                }
                Err(_) if !hits.is_empty() => Ok(keep(scope, hits)),
                Err(e) => Err(e),
            };
            let _ = tx.send(Event::Packs(seq, result));
        });
    }

    pub(super) fn alt_pack_answer(&mut self, seq: u64, result: Result<Vec<Project>, String>) {
        let p = &mut self.alt.mods.packs;
        if seq != p.seq {
            return;
        }
        match result {
            Ok(found) => {
                p.results = found;
                p.status = Status::Done;
            }
            Err(e) => {
                p.results.clear();
                p.status = Status::Failed(e);
            }
        }
    }

    /// A pack install ended: its words; what lists packs is read again.
    pub(super) fn alt_pack_done(&mut self, result: Result<String, String>) {
        self.alt.mods.busy = None;
        self.alt.mods.notice = Some(result);
        self.game_packs.stale();
        self.world_packs.reread();
    }

    /// Add on a result: checked against the scope, the selected setup and (for a data pack) the
    /// chosen world, then installed in a worker.
    fn alt_pack_install(&mut self, source: SourceId, id: &str) {
        let scope = self.alt.mods.packs.scope;
        let Some(project) = self
            .alt
            .mods
            .packs
            .results
            .iter()
            .find(|p| p.source == source && p.id == id && Some(p.kind) == scope.kind())
            .cloned()
        else {
            return;
        };
        if !self.alt_guard() {
            return;
        }
        let Some((instance, home)) = self.pack_setup() else {
            self.alt.mods.notice = Some(Err(tr("Select a Sunrise setup first.").to_string()));
            return;
        };
        // A resource pack never uses a world; a data pack needs the explicit, still valid one.
        let world = match project.kind {
            Kind::DataPack => {
                let target = self.alt.mods.packs.target.clone();
                let ok = target
                    .filter(|t| target_belongs(t, &instance, &home))
                    .ok_or_else(|| {
                        tr("Open a world's Data packs, then Add, to choose where this goes.")
                            .to_string()
                    })
                    .and_then(|t| check_world(&home, &t.world).map(|_| t));
                match ok {
                    Ok(t) => Some(t),
                    Err(why) => {
                        self.alt.mods.packs.target = None;
                        self.alt.mods.notice = Some(Err(why));
                        return;
                    }
                }
            }
            _ => None,
        };
        self.alt_dismiss_notice();
        self.alt.mods.busy = Some((tr("Checking the pack…").into(), 0.));
        let (tx, game) = (self.alt.mods.tx.clone(), self.mods_game());
        std::thread::spawn(move || {
            let result = install(&project, &home, world.as_ref(), &game, &|line, f| {
                let _ = tx.send(Event::Progress(line.to_owned(), f));
            });
            let _ = tx.send(Event::PackDone(result));
        });
    }

    /// Scope tabs above Discover's list; the packs' own list when a pack scope is open.
    /// `page` is Discover's header and strip so far.
    pub(super) fn alt_pack_tabs(&mut self, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().items_center().gap(px(8.));
        for (i, scope) in Scope::ALL.into_iter().enumerate() {
            row = row.child(
                chip(
                    ElementId::named_usize("alt-discover-scope", i),
                    scope.label(),
                    self.alt.mods.packs.scope == scope,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt_pack_scope(scope);
                    cx.notify();
                })),
            );
        }
        row
    }

    pub(super) fn alt_pack_results(&mut self, page: Div, cx: &mut Context<Self>) -> AnyElement {
        let scope = self.alt.mods.packs.scope;
        let setup = self.pack_setup();
        let target = self
            .alt
            .mods
            .packs
            .target
            .clone()
            .filter(|t| setup.as_ref().is_some_and(|(i, h)| target_belongs(t, i, h)));
        let mut page = page.child(muted(match scope {
            Scope::Data => tr("From Modrinth. A data pack goes into one world, chosen under that world's Data packs on Home."),
            Scope::Shader => tr("From Sunrise's registry and Modrinth, into the selected setup's shader packs."),
            _ => tr("From Modrinth, into the selected setup's resource packs."),
        }));
        if scope == Scope::Data {
            page = page.child(muted(match &target {
                Some(t) => trf("Adding to the world {world}.", &[("world", &t.name)]),
                None => tr("No world chosen: open a world's Data packs, then Add.").to_string(),
            }));
        }
        let added: HashSet<(SourceId, String)> = self
            .alt
            .mods
            .snapshot
            .as_ref()
            .map(|s| {
                s.entries
                    .iter()
                    .map(|e| (e.source, e.project.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let working = self.alt.mods.busy.is_some();
        let results = self.alt.mods.packs.results.clone();
        let body: AnyElement = match &self.alt.mods.packs.status {
            Status::Searching => muted(tr("Searching Modrinth…")).into_any_element(),
            Status::Idle => muted(tr("Type a search and press Enter.")).into_any_element(),
            Status::Failed(e) => card()
                .p(px(20.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr("Modrinth can't be reached")),
                )
                .child(muted(e.clone()))
                .child(
                    button("alt-pack-retry", tr("Try again")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.alt_pack_search();
                            cx.notify();
                        },
                    )),
                )
                .into_any_element(),
            Status::Done if results.is_empty() => {
                muted(tr("Nothing found. Try another search.")).into_any_element()
            }
            Status::Done => {
                let game = self.mods_game();
                let mut list = div().flex().flex_col().gap(px(10.));
                for (n, p) in results.iter().enumerate() {
                    let picture = p
                        .icon
                        .clone()
                        .and_then(|url| self.mods_image(&url, images::ICON));
                    let support = p.support(&game);
                    let action: AnyElement = if added.contains(&(p.source, p.id.clone()))
                        && p.kind != Kind::DataPack
                    {
                        muted(tr("Added")).into_any_element()
                    } else if let Some(why) = support.reason().filter(|_| !support.installable()) {
                        muted(why.to_owned()).into_any_element()
                    } else if p.kind == Kind::DataPack && target.is_none() {
                        muted(tr("Choose a world first")).into_any_element()
                    } else {
                        let b = button(ElementId::named_usize("alt-pack-add", n), tr("Add"));
                        if working {
                            dim(b).into_any_element()
                        } else {
                            let id = p.id.clone();
                            let source = p.source;
                            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.alt_pack_install(source, &id);
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .into_any_element()
                        }
                    };
                    let kind_label = match p.kind {
                        Kind::DataPack => tr("Data pack"),
                        Kind::Shader => tr("Shader pack"),
                        _ => tr("Resource pack"),
                    };
                    let source_label = match p.source {
                        SourceId::Sunrise => tr("Sunrise registry"),
                        other => other.label(),
                    };
                    // An Iris-format pack says how well it runs, in words.
                    let iris = (p.source == SourceId::Modrinth && p.kind == Kind::Shader)
                        .then(|| support.reason().map(str::to_owned))
                        .flatten();
                    let page_url = p.page.clone();
                    list = list.child(
                        div()
                            .id(ElementId::named_usize("alt-pack-card", n))
                            .flex()
                            .gap(px(12.))
                            .p(px(12.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(c(LINE))
                            .bg(c(crate::alt::CARD))
                            .child(tile(picture, &p.title, 48.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_baseline()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .truncate()
                                                    .child(p.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(c(MUTED))
                                                    .truncate()
                                                    .child(p.author.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(c(MUTED))
                                            .truncate()
                                            .child(p.summary.clone()),
                                    )
                                    .children(iris.map(muted))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(tag(source_label))
                                                    .child(tag(kind_label))
                                                    .child(tag(support.label()))
                                                    .child(muted(model::downloads(p.downloads))),
                                            )
                                            .child(action),
                                    ),
                            )
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                Launcher::alt_open_url(&page_url, cx);
                            })),
                    );
                }
                list.into_any_element()
            }
        };
        page.child(
            div()
                .id("alt-pack-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .text_color(c(TEXT))
                .child(body),
        )
        .bg(c(crate::alt::BG))
        .into_any_element()
    }
}

/// Puts `project` in place with the browser's checked install: a resource pack in the setup's
/// `resourcepacks/`, a data pack in `target`'s world. Under the setup's lock, with the world
/// checked again. Nothing is turned on.
fn install(
    project: &Project,
    home: &Path,
    target: Option<&Target>,
    game: &str,
    progress: &dyn Fn(&str, f32),
) -> Result<String, String> {
    let source = browser::source(project.source)
        .ok_or_else(|| tr("This source isn't set up.").to_string())?;
    let _lock = browser::try_lock_home(home).ok_or_else(|| {
        tr("This setup is busy. Try again when the operation finishes.").to_string()
    })?;
    match (project.kind, target) {
        (Kind::DataPack, Some(t)) => {
            let name = check_world(home, &t.world)?;
            let done =
                installed::install_datapack(&*source, project, false, &t.world, game, &|f| {
                    progress(&project.title, f)
                })?;
            Ok(trf(
                "Put {pack} {version} in {world}'s data packs, checksum checked: the game turns \
                 it on when the world opens. {coverage}",
                &[
                    ("pack", &project.title),
                    ("version", &done.number),
                    ("world", &name),
                    ("coverage", &done.coverage.summary()),
                ],
            ))
        }
        (Kind::DataPack, None) => Err(tr("Choose a world first.").to_string()),
        (Kind::ResourcePack | Kind::Shader, _) => {
            let done = installed::install(&*source, project, false, home, game, progress)?;
            let names: Vec<String> = done
                .iter()
                .map(|e| format!("{} {}", e.title, e.number))
                .collect();
            let on = match project.kind {
                Kind::Shader => tr("Packs → Shader packs"),
                _ => tr("Packs → Resource packs"),
            };
            Ok(trf(
                "Installed {mods}, checksums checked. Turn it on under {where}.",
                &[("mods", &names.join(", ")), ("where", on)],
            ))
        }
        _ => Err(tr("Only resource, data and shader packs install from here.").to_string()),
    }
}

#[cfg(test)]
mod tests;
