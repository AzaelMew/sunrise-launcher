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

mod actions;
mod discover;
mod examples;
mod installed_view;
mod model;
mod project;
mod ui;
pub(crate) use ui::{checkbox, chip, field_theme, switch};

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
    /// Saved worlds hold content of these mods: which worlds and which mods. Nothing has been
    /// changed; continuing backs each world up and checks the backup first.
    Stake(actions::Action, Vec<(String, Vec<String>)>),
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
    /// Native files the browser didn't install: listed, never changed from here.
    local: Vec<model::Local>,
}

enum Event {
    Catalog(Result<(Vec<Card>, Vec<String>), String>),
    Page(String, Result<Box<model::Page>, String>),
    Progress(String, f32),
    /// Worlds would be affected: nothing was changed, the player decides.
    Stake(PathBuf, actions::Action, Vec<(String, Vec<String>)>),
    /// The batch was refused before changing anything.
    Refused(String),
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
    /// The folder the open confirmation was made for.
    reviewed_home: Option<PathBuf>,
    snapshot: Option<Snapshot>,
    open: Option<Open>,
    /// My own batch (update or remove): its line and fraction.
    busy: Option<(String, f32)>,
    notice: Option<Result<String, String>>,
    /// Why a mod's last update failed, shown on its row until the next attempt.
    errors: HashMap<String, String>,
    /// How many cards Discover draws ("Show more" adds a page).
    limit: usize,
    /// The Installed row whose "?" is open (a mod not on this build), and whether its
    /// technical details show.
    unfit_open: Option<String>,
    unfit_details: bool,
    /// The last Mods view the player chose this session.
    last_mods: Option<Page>,
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
            reviewed_home: None,
            snapshot: None,
            open: None,
            busy: None,
            notice: None,
            errors: HashMap::new(),
            limit: 0,
            unfit_open: None,
            unfit_details: false,
            last_mods: None,
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

    /// The installed mods `ids` as people read them: each one's name, and the newer version the
    /// registry has for it when there is one. Reads what is already in memory.
    pub(crate) fn alt_mods_named(&self, ids: &[String]) -> Vec<(String, Option<String>)> {
        let rows = self.alt_rows();
        ids.iter()
            .map(|id| match rows.iter().find(|r| &r.id == id) {
                Some(row) => (row.title.clone(), row.update.clone()),
                None => (id.clone(), None),
            })
            .collect()
    }

    /// Updates the installed mod `id` to the newer version the registry has, as its row does.
    pub(crate) fn alt_update_mod(&mut self, id: &str) {
        if let Some(row) = self.alt_rows().into_iter().find(|r| r.id == id) {
            self.alt_update_one(&row);
        }
    }

    /// Reads the setup's mods for the play bar, when no Mods page has yet.
    pub(crate) fn alt_mods_read_once(&mut self) {
        if self.alt.mods.snapshot.is_none() {
            self.alt_mods_refresh();
        }
    }

    /// How many of the setup's mods the game it plays was made without.
    pub(crate) fn alt_mods_not_on_build(&self) -> usize {
        self.alt_rows().iter().filter(|r| r.unfit.is_some()).count()
    }

    /// The play bar's quiet line about them: Mods › Installed, where each says why.
    pub(crate) fn alt_show_installed_mods(&mut self) {
        self.alt_mods_refresh();
        self.alt_choose_view(Page::Installed);
    }

    /// The rail's Mods: where the player last was this session, else Installed when this setup
    /// has mods and Discover when it has none.
    pub(super) fn alt_open_mods(&mut self) {
        self.alt.page = self.alt.mods.last_mods.unwrap_or_else(|| {
            self.alt_mods_refresh();
            match self.alt_rows().is_empty() {
                true => Page::Discover,
                false => Page::Installed,
            }
        });
    }

    /// Discover or Installed, chosen on the header's switch: remembered for the session.
    fn alt_choose_view(&mut self, page: Page) {
        let m = &mut self.alt.mods;
        m.last_mods = Some(page);
        m.selection.clear();
        m.anchor = None;
        m.confirm = None;
        m.menu = None;
        self.alt.page = page;
    }

    /// The width a page's own content has: the window less the rail and the page's margins.
    pub(super) fn alt_content_width(&self) -> f32 {
        self.window_width - 72. - 2. - 48.
    }

    fn alt_mods_home(&self) -> PathBuf {
        self.selected_instance()
            .map(|i| i.home.clone())
            .unwrap_or_else(backend::game_home)
    }

    /// What the workers said. Nothing here touches a file.
    pub(super) fn alt_mods_drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.alt.mods.rx.try_recv() {
            changed = true;
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
                Event::Stake(home, action, worlds) => {
                    m.busy = None;
                    // Only for the folder still shown: a worker's answer for another setup is dropped.
                    if m.snapshot.as_ref().is_some_and(|s| s.home == home) {
                        m.reviewed_home = Some(home);
                        m.confirm = Some(Confirm::Stake(action, worlds));
                    }
                }
                Event::Refused(why) => {
                    m.busy = None;
                    m.notice = Some(Err(why));
                }
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
        changed
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
        let moved = self
            .alt
            .mods
            .snapshot
            .as_ref()
            .is_some_and(|s| s.home != home);
        if moved {
            // Another setup: nothing reviewed or selected for the old one carries over.
            let m = &mut self.alt.mods;
            m.confirm = None;
            m.reviewed_home = None;
            m.selection.clear();
            m.anchor = None;
            m.errors.clear();
            m.notice = None;
        }
        if self.alt.mods.snapshot.as_ref().is_none_or(|s| s.key != key) {
            let entries = installed::Installed::load(&home).entries;
            let local: Vec<model::Local> = native_status::untracked(&home)
                .into_iter()
                .map(|f| model::Local {
                    id: f.id,
                    version: f.version,
                    github: f.origin.is_some(),
                })
                .collect();
            let mut enabled: HashSet<String> = entries
                .iter()
                .filter_map(|e| sunrise::mod_id(e))
                .filter(|id| java::is_enabled(&home, id))
                .map(str::to_owned)
                .collect();
            enabled.extend(
                local
                    .iter()
                    .filter(|l| java::is_enabled(&home, &l.id))
                    .map(|l| l.id.clone()),
            );
            let chosen = sunrise::chosen(&home);
            self.alt.mods.snapshot = Some(Snapshot {
                key,
                home,
                entries,
                enabled,
                chosen,
                local,
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
        let unbuilt: HashSet<String> = match !snap.local.is_empty()
            || snap
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
        let mut rows = model::rows(
            &snap.entries,
            &model::World {
                index: index.as_deref(),
                game: &game,
                enabled: &snap.enabled,
                unbuilt: &unbuilt,
                native_allowed: self.settings.native_mods_allowed_at.is_some(),
                chosen: &snap.chosen,
                local: &snap.local,
            },
        );
        // A mod the game was made without isn't waiting for a build: it waits for its author.
        let left = self.native_left_out();
        if !left.is_empty() {
            let on = self
                .selected_instance()
                .and_then(|i| self.version_of(i))
                .map(|tag| crate::alt_sheets::build_name(tag))
                .unwrap_or_default();
            for row in rows.iter_mut().filter(|r| r.on) {
                let Some(l) = left
                    .iter()
                    .find(|l| l.id == row.id && l.version == row.version)
                    .filter(|l| {
                        matches!(
                            l.why,
                            native_mods::Why::Unfit(_) | native_mods::Why::OtherVersion
                        )
                    })
                else {
                    continue;
                };
                let detail = l.detail.clone().unwrap_or_default();
                row.need = None;
                row.unfit = Some(model::Unfit {
                    on: on.clone(),
                    made_for: detail
                        .made_for
                        .map(|tag| (crate::alt_sheets::build_name(&tag), tag))
                        .map(|(name, tag)| (tag, name)),
                    changes: detail.changes,
                });
            }
            // A row that no longer needs the player moves out of that group.
            model::sort(&mut rows);
        }
        rows
    }

    /// Whether another install or update holds the folder right now.
    fn alt_mods_busy(&self) -> bool {
        self.alt_mutating()
    }

    /// Whether a mod change is in progress (mine or the shared install's), for the shared Play
    /// and setup gates. Reads only the Mods busy flags, never the launch state.
    pub(super) fn alt_mutating(&self) -> bool {
        self.alt.mods.busy.is_some() || self.mods.busy.is_some()
    }

    /// Build… on a row: the one native build path, which asks for permission first. The player
    /// stays where they are: the strip above the list and the play bar show how far it is.
    fn alt_build(&mut self) {
        self.build_native();
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
        if let Some(build) = self.alt_build_strip(cx) {
            return Some(build);
        }
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
                                    .bg(c(crate::alt::TEXT))
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
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .text_color(c(color))
                        .child(text),
                )
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
        let placeholder: SharedString = if installed {
            tr("Search your mods")
        } else {
            tr("Search mods")
        }
        .into();
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
            .min_w(px(120.))
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
                    if this.alt.page != page {
                        this.alt_choose_view(page);
                    }
                    cx.notify();
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(
                div()
                    .flex_none()
                    .text_size(px(28.))
                    .font_weight(FontWeight::BOLD)
                    .child(tr("Mods")),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
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
