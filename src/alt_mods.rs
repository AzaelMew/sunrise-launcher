//! Mods in the alternative look: Discover, Installed and a mod's page (mocks 03, 04, 05, 05b).
//!
//! **Real:** the registry list, search, categories, fit, kind and sort (`browser::sunrise`'s index,
//! read in a worker); a mod's page from the index (versions and their fit, size, licence,
//! authors, source, mirrored screenshots, what it may do); Add and single Update through the
//! existing install path (native code asks for permission first, `native_consent`); Installed
//! from `mod-browser.json` and `sunrise-mods.txt` with on/off, selection, filters, bulk Turn on,
//! Turn off, Remove and Update N mods (a reviewed plan, applied by a worker); Build… and Allow…
//! hand over to the one native build path.
//!
//! **Not here, said on screen or in the handoff:** Featured (the registry carries no editorial
//! list), a version picker (the registry installs the newest that fits), dependency lists for
//! plain mods (the index lists a mod's needs for packs only), "Add from a file…", resource,
//! shader and data packs, Modrinth, Java and registry modpacks.
//!
//! **Writes** happen only for an action the player took: install, update, on/off, remove. Showing
//! the screens reads the index (its own cache refresh, in a worker), `mod-browser.json`,
//! `sunrise-mods.txt` and file times. Nothing turns off or repairs a mod while drawing: taking
//! down what the registry revoked is left to `before_play` and the row says so.
use super::*;
use crate::alt::Page;
use crate::line_edit::{self, Key, LineEdit};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use sunrise_launcher::browser::{
    installed::{self, Entry},
    java, sunrise, Source, SourceId,
};

mod discover;
mod examples;
mod installed_view;
mod model;
mod project;
mod ui;

use model::{Card, Row};

enum Menu {
    Make,
    Sort,
}

enum Catalog {
    Idle,
    Loading,
    Ready(Vec<Card>, Vec<String>),
    Failed(String),
}

/// A batch the player asked for and has to confirm: named, counted, nothing started yet.
enum Confirm {
    /// (mod id, project id, title)
    Remove(Vec<(String, String, String)>),
    Update(model::UpdatePlan),
}

struct Open {
    id: String,
    /// The screen Back returns to.
    from: Page,
    page: Option<Result<Box<model::Page>, String>>,
}

struct Snapshot {
    key: String,
    home: PathBuf,
    entries: Vec<Entry>,
    enabled: HashSet<String>,
    chosen: std::collections::BTreeMap<String, String>,
}

enum Event {
    Catalog(Result<(Vec<Card>, Vec<String>), String>),
    Page(String, Result<Box<model::Page>, String>),
    Progress(String, f32),
    /// A batch ended: what it said, and which mods failed and why.
    Done(String, Vec<(String, String)>),
}

pub(crate) struct AltModsState {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// `SUNRISE_PREVIEW_EXAMPLES`, read once.
    examples: bool,
    catalog: Catalog,
    find: LineEdit,
    find_focus: Option<FocusHandle>,
    have: LineEdit,
    have_focus: Option<FocusHandle>,
    filters: model::Filters,
    menu: Option<Menu>,
    row_filter: model::RowFilter,
    selection: BTreeSet<String>,
    /// The row last clicked, for Shift-click ranges (an index into the visible rows).
    anchor: Option<usize>,
    confirm: Option<Confirm>,
    snapshot: Option<Snapshot>,
    open: Option<Open>,
    /// My own batch (update or remove): its line and fraction.
    busy: Option<(String, f32)>,
    notice: Option<Result<String, String>>,
    /// Why a mod's last update failed, shown on its row until the next attempt.
    errors: HashMap<String, String>,
    /// How many cards Discover draws ("Show more" adds a page).
    limit: usize,
}

impl Default for AltModsState {
    fn default() -> Self {
        let (tx, rx) = crate::wake::channel();
        Self {
            tx,
            rx,
            examples: model::examples_on(),
            catalog: Catalog::Idle,
            find: LineEdit::default(),
            find_focus: None,
            have: LineEdit::default(),
            have_focus: None,
            filters: model::Filters::default(),
            menu: None,
            row_filter: model::RowFilter::default(),
            selection: BTreeSet::new(),
            anchor: None,
            confirm: None,
            snapshot: None,
            open: None,
            busy: None,
            notice: None,
            errors: HashMap::new(),
            limit: 0,
        }
    }
}

impl Launcher {
    /// The Mods screen for `self.alt.page`: Discover, Installed or a mod's page.
    pub(crate) fn alt_mods(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.alt_mods_drain();
        self.alt_mods_refresh();
        let m = &mut self.alt.mods;
        let find_focus = m
            .find_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let have_focus = m
            .have_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        match self.alt.page {
            Page::Project => self.alt_project(cx),
            Page::Installed => self.alt_installed(window, &have_focus, cx),
            _ => self.alt_discover(window, &find_focus, cx),
        }
    }

    /// Opens a mod's page, from Home or anywhere else. Its details are read in a worker.
    pub(crate) fn alt_mods_show_project(&mut self, id: String) {
        let from = match self.alt.page {
            Page::Project => self
                .alt
                .mods
                .open
                .as_ref()
                .map_or(Page::Discover, |o| o.from),
            Page::Installed => Page::Installed,
            _ => Page::Discover,
        };
        self.alt.page = Page::Project;
        self.alt_mods_load_page(id, from);
    }

    fn alt_mods_home(&self) -> PathBuf {
        self.selected_instance()
            .map(|i| i.home.clone())
            .unwrap_or_else(backend::game_home)
    }

    /// What the workers said. Nothing here touches a file.
    fn alt_mods_drain(&mut self) {
        while let Ok(event) = self.alt.mods.rx.try_recv() {
            let m = &mut self.alt.mods;
            match event {
                Event::Catalog(Ok((cards, categories))) => {
                    m.catalog = Catalog::Ready(cards, categories)
                }
                Event::Catalog(Err(e)) => m.catalog = Catalog::Failed(e),
                Event::Page(id, page) => {
                    if let Some(open) = m.open.as_mut().filter(|o| o.id == id) {
                        open.page = Some(page);
                    }
                }
                Event::Progress(line, f) => m.busy = Some((line, f)),
                Event::Done(said, failed) => {
                    m.busy = None;
                    m.errors.clear();
                    m.errors.extend(failed.iter().cloned());
                    m.notice = Some(match failed.is_empty() {
                        true => Ok(said),
                        false => Err(format!(
                            "{said} {}",
                            trf(
                                "Not done: {list}",
                                &[(
                                    "list",
                                    &failed
                                        .iter()
                                        .map(|(id, why)| format!("{id}: {why}"))
                                        .collect::<Vec<_>>()
                                        .join("; ")
                                )]
                            )
                        )),
                    });
                    // The list is read again by the next refresh (the files' times moved).
                }
            }
        }
    }

    /// Reads what is installed again when the files' fingerprint moved (an install, a toggle or a
    /// build elsewhere), and asks for the registry once. Reads only.
    fn alt_mods_refresh(&mut self) {
        if self.alt.mods.examples {
            return;
        }
        let home = self.alt_mods_home();
        self.mods
            .follow_home(self.selected_instance().map(|i| i.home.clone()));
        let key = native_status::fingerprint(&home, None);
        if self.alt.mods.snapshot.as_ref().is_none_or(|s| s.key != key) {
            let entries = installed::Installed::load(&home).entries;
            let enabled = entries
                .iter()
                .filter_map(|e| sunrise::mod_id(e))
                .filter(|id| java::is_enabled(&home, id))
                .map(str::to_owned)
                .collect();
            let chosen = sunrise::chosen(&home);
            self.alt.mods.snapshot = Some(Snapshot {
                key,
                home,
                entries,
                enabled,
                chosen,
            });
            let rows: HashSet<String> = self.alt_rows().into_iter().map(|r| r.id).collect();
            self.alt.mods.selection.retain(|id| rows.contains(id));
        }
        let wants_index = self
            .alt
            .mods
            .snapshot
            .as_ref()
            .is_some_and(|s| !s.entries.is_empty())
            || self.alt.page != Page::Installed;
        if wants_index && matches!(self.alt.mods.catalog, Catalog::Idle) {
            self.alt_mods_load_catalog();
        }
    }

    fn alt_mods_load_catalog(&mut self) {
        self.alt.mods.catalog = Catalog::Loading;
        let (tx, game) = (self.alt.mods.tx.clone(), self.mods_game());
        std::thread::spawn(move || {
            let result = sunrise::index().map(|index| {
                let cards = model::cards(&index, &game);
                let categories = model::categories(&cards);
                (cards, categories)
            });
            let _ = tx.send(Event::Catalog(result));
        });
    }

    fn alt_mods_load_page(&mut self, id: String, from: Page) {
        if self.alt.mods.examples {
            let page =
                examples::page(&id).ok_or_else(|| tr("This example has no page.").to_string());
            self.alt.mods.open = Some(Open {
                id,
                from,
                page: Some(page.map(Box::new)),
            });
            return;
        }
        self.alt.mods.open = Some(Open {
            id: id.clone(),
            from,
            page: None,
        });
        let (tx, game) = (self.alt.mods.tx.clone(), self.mods_game());
        let instance = instances::selected();
        std::thread::spawn(move || {
            let result = sunrise::index().and_then(|index| {
                let trust = sunrise::trust_and_capabilities(&id, Some(&instance), &game);
                model::page(&index, &id, &game, trust)
                    .map(Box::new)
                    .ok_or_else(|| trf("{id} isn't a mod in Sunrise's registry.", &[("id", &id)]))
            });
            let _ = tx.send(Event::Page(id, result));
        });
    }

    /// The installed mods as rows (or the labelled examples). Reads the index already in memory
    /// and the native build state's memo; never asks the network and never writes.
    fn alt_rows(&self) -> Vec<Row> {
        if self.alt.mods.examples {
            return examples::rows(120);
        }
        let Some(snap) = &self.alt.mods.snapshot else {
            return Vec::new();
        };
        let unbuilt: HashSet<String> = match snap
            .entries
            .iter()
            .any(|e| native_status::is_native_file(&e.file))
        {
            true => {
                self.native_mod_unbuilt("");
                self.mods
                    .native_state
                    .borrow()
                    .as_ref()
                    .map(|(_, _, ids)| ids.iter().cloned().collect())
                    .unwrap_or_default()
            }
            false => HashSet::new(),
        };
        let index = sunrise::known_index();
        let game = self.mods_game();
        model::rows(
            &snap.entries,
            &model::World {
                index: index.as_deref(),
                game: &game,
                enabled: &snap.enabled,
                unbuilt: &unbuilt,
                native_allowed: self.settings.native_mods_allowed_at.is_some(),
                chosen: &snap.chosen,
            },
        )
    }

    /// Whether another install or update holds the folder right now.
    fn alt_mods_busy(&self) -> bool {
        self.alt.mods.busy.is_some() || self.mods.busy.is_some()
    }

    // ------------------------------------------------------------------ actions

    /// Add `id` to the game: the existing install path, which asks first for native code, checks
    /// the file against the index and its signature, and refuses what was taken down.
    fn alt_add(&mut self, id: String) {
        if self.alt.mods.examples || self.alt_mods_busy() {
            return;
        }
        self.alt.mods.notice = None;
        self.mods.message = None;
        self.mods_install(SourceId::Sunrise, id, None);
    }

    /// One mod on or off: the existing switch, with its native-permission and taken-down checks.
    fn alt_toggle(&mut self, row: &Row) {
        let Some(entry) = row.entry.clone().filter(|_| !row.example) else {
            return;
        };
        self.mods.message = None;
        self.mods_switch(&entry);
        if let Some(Err(e)) = self.mods.message.take() {
            self.alt.mods.notice = Some(Err(e));
        }
    }

    /// Turn the chosen mods on or off in one rewrite of the list. A mod that can't be turned on
    /// (taken down, or native code with no permission) stays as it is and its row says why.
    fn alt_bulk_toggle(&mut self, on: bool) {
        let rows = self.alt_rows();
        let chosen: Vec<&Row> = rows
            .iter()
            .filter(|r| !r.example && self.alt.mods.selection.contains(&r.id))
            .collect();
        let (go, held) = match on {
            true => model::turn_on_plan(&chosen, self.settings.native_mods_allowed_at.is_some()),
            false => (chosen.clone(), Vec::new()),
        };
        let m = &mut self.alt.mods;
        m.errors = held
            .iter()
            .map(|(r, why)| (r.id.clone(), why.clone()))
            .collect();
        let Some(home) = m.snapshot.as_ref().map(|s| s.home.clone()) else {
            return;
        };
        if go.is_empty() {
            m.notice = Some(Err(tr("Nothing selected can be turned on yet.").to_string()));
            return;
        }
        let Some(_lock) = sunrise_launcher::browser::try_lock_home(&home) else {
            m.notice = Some(Err(
                tr("Wait for the current operation to finish.").to_string()
            ));
            return;
        };
        let ids: Vec<&str> = go.iter().map(|r| r.id.as_str()).collect();
        m.notice = Some(match java::set_all_enabled(&home, &ids, on) {
            Ok(()) if held.is_empty() => Ok(match on {
                true => trf(
                    "Turned on {count} mods. The game reads this when it starts.",
                    &[("count", &ids.len().to_string())],
                ),
                false => trf(
                    "Turned off {count} mods. The game reads this when it starts.",
                    &[("count", &ids.len().to_string())],
                ),
            }),
            Ok(()) => Err(trf(
                "Turned on {done}. {held} stayed off: see their rows.",
                &[
                    ("done", &ids.len().to_string()),
                    ("held", &held.len().to_string()),
                ],
            )),
            Err(e) => Err(e),
        });
    }

    /// Asks for confirmation before removing the chosen mods.
    fn alt_ask_remove(&mut self) {
        let rows = self.alt_rows();
        let chosen: Vec<(String, String, String)> = rows
            .iter()
            .filter(|r| !r.example && self.alt.mods.selection.contains(&r.id))
            .filter_map(|r| {
                Some((
                    r.id.clone(),
                    r.entry.as_ref()?.project.clone(),
                    r.title.clone(),
                ))
            })
            .collect();
        if !chosen.is_empty() {
            self.alt.mods.confirm = Some(Confirm::Remove(chosen));
        }
    }

    fn alt_remove(&mut self, chosen: Vec<(String, String, String)>) {
        if self.alt_mods_busy() {
            return;
        }
        let m = &mut self.alt.mods;
        m.confirm = None;
        m.notice = None;
        m.busy = Some((tr("Removing…").to_string(), 0.));
        m.selection.clear();
        let (tx, home) = (
            m.tx.clone(),
            m.snapshot
                .as_ref()
                .map(|s| s.home.clone())
                .unwrap_or_else(backend::game_home),
        );
        std::thread::spawn(move || {
            let (mut done, mut failed) = (0, Vec::new());
            for (i, (id, project, title)) in chosen.iter().enumerate() {
                let _ = tx.send(Event::Progress(
                    trf("Removing {name}…", &[("name", title)]),
                    i as f32 / chosen.len() as f32,
                ));
                match installed::remove(&home, project) {
                    Ok(_) => done += 1,
                    Err(e) => failed.push((id.clone(), e)),
                }
            }
            let said = trf("Removed {count} mods.", &[("count", &done.to_string())]);
            let _ = tx.send(Event::Done(said, failed));
        });
    }

    /// One mod's update. Native code without permission asks first, as installing it does.
    fn alt_update_one(&mut self, row: &Row) {
        let Some(to) = row.update.clone().filter(|_| !row.example) else {
            return;
        };
        if row.native() && self.settings.native_mods_allowed_at.is_none() {
            self.pending_consent = Some(PendingConsent::Install(
                SourceId::Sunrise,
                row.id.clone(),
                None,
            ));
            return;
        }
        self.alt_update(vec![model::Included {
            id: row.id.clone(),
            title: row.title.clone(),
            from: row.version.clone(),
            to,
            native: row.native(),
            was_on: row.on,
        }]);
    }

    /// Installs each update through the registry's checked install (size, SHA-256, signature,
    /// taken-down list), one after the other. An update leaves the mod as the player had it:
    /// one that was off is turned off again. A failure is tied to its mod and stops nothing else.
    fn alt_update(&mut self, items: Vec<model::Included>) {
        if self.alt_mods_busy() || items.is_empty() {
            return;
        }
        let game = self.mods_game();
        let m = &mut self.alt.mods;
        m.confirm = None;
        m.notice = None;
        m.errors.clear();
        m.busy = Some((tr("Updating…").to_string(), 0.));
        let (tx, home) = (
            m.tx.clone(),
            m.snapshot
                .as_ref()
                .map(|s| s.home.clone())
                .unwrap_or_else(backend::game_home),
        );
        std::thread::spawn(move || {
            let (mut done, mut failed) = (0, Vec::new());
            let count = items.len();
            for (i, item) in items.iter().enumerate() {
                let step = |line: &str, f: f32| {
                    let _ = tx.send(Event::Progress(
                        trf(
                            "{n} of {count}: {line}",
                            &[
                                ("n", &(i + 1).to_string()),
                                ("count", &count.to_string()),
                                ("line", line),
                            ],
                        ),
                        (i as f32 + f) / count as f32,
                    ));
                };
                let result = sunrise::Registry
                    .project(&item.id)
                    .and_then(|p| sunrise::install(&p, &home, &game, &step))
                    .and_then(|_| match item.was_on {
                        true => Ok(()),
                        false => java::set_enabled(&home, &item.id, false),
                    });
                match result {
                    Ok(()) => done += 1,
                    Err(e) => failed.push((item.id.clone(), e)),
                }
            }
            let said = trf(
                "Updated {count} mods, each checked against the registry.",
                &[("count", &done.to_string())],
            );
            let _ = tx.send(Event::Done(said, failed));
        });
    }

    /// Build… on a row: the one native build path, which asks for permission first.
    fn alt_build(&mut self) {
        self.build_native();
        if self.native_step.is_some() {
            self.alt.page = Page::Home;
        }
    }

    /// Allow… on a row: the native-code question, with what it builds.
    fn alt_allow(&mut self) {
        self.request_native_build_consent();
    }

    fn alt_open_url(url: &str, cx: &mut Context<Self>) {
        if url.starts_with("https://") {
            cx.open_url(url);
        }
    }

    /// What the notice strip says: the batch's own, else what the shared install path said.
    fn alt_notice(&self) -> Option<Result<String, String>> {
        self.alt
            .mods
            .notice
            .clone()
            .or_else(|| self.mods.message.clone())
    }

    fn alt_dismiss_notice(&mut self) {
        self.alt.mods.notice = None;
        self.mods.message = None;
    }

    /// The strip shown above either list: example banner, progress, or the last result.
    fn alt_strip(&self, cx: &mut Context<Self>) -> Option<Div> {
        use ui::{c, muted};
        let busy = self.alt.mods.busy.clone().or_else(|| {
            self.mods
                .busy
                .as_ref()
                .map(|(_, line, f)| (line.clone(), *f))
        });
        if let Some((line, f)) = busy {
            return Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(13.)).child(line))
                    .child(
                        div()
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(c(crate::alt::LINE))
                            .child(
                                div()
                                    .h(px(4.))
                                    .rounded(px(2.))
                                    .bg(c(crate::alt::GOLD))
                                    .w(relative(f.clamp(0.02, 1.))),
                            ),
                    ),
            );
        }
        let notice = self.alt_notice()?;
        let (text, color) = match notice {
            Ok(t) => (t, crate::alt::TEXT),
            Err(t) => (t, crate::alt::GOLD),
        };
        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(div().text_size(px(13.)).text_color(c(color)).child(text))
                .child(
                    muted(tr("Dismiss"))
                        .id("alt-mods-dismiss")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_dismiss_notice();
                            cx.notify();
                        })),
                ),
        )
    }

    /// Title, Discover/Installed switch and the search field, shared by both lists.
    fn alt_mods_header(
        &mut self,
        installed: bool,
        focus: &FocusHandle,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        use ui::{c, field_theme};
        let t = field_theme(self.theme);
        let focused = focus.is_focused(window);
        let count = match &self.alt.mods.catalog {
            Catalog::Ready(cards, _) => cards.len(),
            _ => 0,
        };
        let placeholder: SharedString = match (installed, count) {
            (true, _) => tr("Search your mods by name, author or id").into(),
            (false, 0) => tr("Search the Sunrise registry").into(),
            (false, n) => trf(
                "Search {count} mods in the Sunrise registry",
                &[("count", &n.to_string())],
            )
            .into(),
        };
        let edit = if installed {
            &self.alt.mods.have
        } else {
            &self.alt.mods.find
        };
        let field = div()
            .id(if installed {
                "alt-mods-have"
            } else {
                "alt-mods-find"
            })
            .track_focus(focus)
            .flex_1()
            .h(px(44.))
            .px(px(16.))
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(if focused {
                c(crate::alt::TEXT)
            } else {
                c(crate::alt::LINE)
            })
            .bg(c(crate::alt::CARD))
            .text_size(px(14.))
            .cursor_text()
            .child(line_edit::content_with(
                edit,
                &t,
                focused,
                false,
                false,
                placeholder,
            ))
            .on_click({
                let focus = focus.clone();
                cx.listener(move |_, _: &ClickEvent, window, cx| {
                    window.focus(&focus, cx);
                    cx.notify();
                })
            })
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                let m = &mut this.alt.mods;
                let edit = if installed { &mut m.have } else { &mut m.find };
                match line_edit::handle(edit, ev, cx) {
                    Key::Escape => window.blur(cx),
                    Key::Pass => return,
                    Key::Edited => {
                        let text = edit.text().to_owned();
                        if installed {
                            m.row_filter.text = text;
                            // A new filter is a new scope: nothing stays selected out of sight.
                            m.selection.clear();
                            m.anchor = None;
                        } else {
                            m.filters.text = text;
                        }
                    }
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let field = if installed {
            line_edit::mouse(field, cx, |this: &mut Launcher| &mut this.alt.mods.have)
        } else {
            line_edit::mouse(field, cx, |this: &mut Launcher| &mut this.alt.mods.find)
        };
        let seg = |id: &'static str,
                   label: &'static str,
                   on: bool,
                   page: Page,
                   cx: &mut Context<Self>| {
            div()
                .id(id)
                .px(px(18.))
                .h(px(36.))
                .flex()
                .items_center()
                .rounded(px(9.))
                .text_size(px(14.))
                .cursor_pointer()
                .when(on, |d| {
                    d.bg(c(crate::alt::LINE)).font_weight(FontWeight::SEMIBOLD)
                })
                .child(tr(label))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt.page = page;
                    this.alt.mods.selection.clear();
                    this.alt.mods.confirm = None;
                    this.alt.mods.menu = None;
                    cx.notify();
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(
                div()
                    .text_size(px(28.))
                    .font_weight(FontWeight::BOLD)
                    .child(tr("Mods")),
            )
            .child(
                div()
                    .flex()
                    .p(px(3.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(c(crate::alt::LINE))
                    .bg(c(crate::alt::CARD))
                    .child(seg(
                        "alt-mods-tab-discover",
                        "Discover",
                        !installed,
                        Page::Discover,
                        cx,
                    ))
                    .child(seg(
                        "alt-mods-tab-installed",
                        "Installed",
                        installed,
                        Page::Installed,
                        cx,
                    )),
            )
            .child(field)
    }
}
