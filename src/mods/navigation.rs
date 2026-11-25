//! Moving between the Mods, Packs and mod pages: which page opens, what Back returns
//! to, selection, scrolling and the search fields.

use super::*;

impl Launcher {
    /// Opens a v2 registry mod's page from Discover.
    pub(crate) fn mods_show_v2_project(&mut self, id: String) {
        self.ui.mods.nightly.open = None;
        self.ui.mods.nightly.collection = None;
        let from = match self.ui.page {
            Page::Project => self
                .ui
                .mods
                .open
                .as_ref()
                .map_or(Page::Discover, |o| o.from),
            Page::Installed => Page::Installed,
            _ => Page::Discover,
        };
        self.ui.page = Page::Project;
        self.mods_load_v2_page(id, from);
    }

    /// Opens a mod's page, from Home or anywhere else. Its details are read in a worker.
    pub(crate) fn mods_show_project(&mut self, id: String) {
        self.mods_show_v2_project(id);
    }

    pub(super) fn mods_show_installed_project(&mut self, row: &Row) {
        // A signed registry mod has the same page as in Discover, with its installed controls.
        if let Some(entry) = row.entry.as_ref().filter(|_| row.v2_installed()) {
            self.mods_show_v2_project(entry.project.clone());
            return;
        }
        self.ui.mods.nightly.open = None;
        self.ui.mods.nightly.collection = None;
        let (id, installed) = match &row.local {
            Some(local) => (row.id.clone(), InstalledProject::Local(local.file.clone())),
            None => (
                row.entry
                    .as_ref()
                    .map_or(&row.id, |entry| &entry.project)
                    .clone(),
                InstalledProject::Registry,
            ),
        };
        self.ui.page = Page::Project;
        self.mods_load_page(id, Page::Installed, Some(installed));
    }

    /// The page of a mod just removed from `home` has nothing left to show: go back to where it
    /// was opened from.
    pub(crate) fn mods_leave_removed(&mut self, home: &Path, ids: &[String]) {
        if self.ui.page != Page::Project || self.mods_home() != home {
            return;
        }
        let gone = self.ui.mods.open.as_ref().is_some_and(|open| {
            ids.contains(&open.id) || open.removing.iter().any(|id| ids.contains(id))
        });
        if let Some(open) = self.ui.mods.open.take_if(|_| gone) {
            self.ui.page = open.from;
        }
    }

    pub(crate) fn show_resource_pack_discover(&mut self) {
        self.leave_section(true);
        // Enter Discover with the requested kind, not the shelf we last left open.
        self.ui.mods.shelf = Shelf::Resource;
        // Force one fresh search for the cleared query even when Resource was already selected.
        self.ui.mods.packs.status = pack_discover::Status::Idle;
        self.choose_view(Page::PackFind);
    }

    /// The message pump's look at the mods folder while a Mods page is open: once the kept
    /// fingerprint is due its full read, read it, so a mod rewritten where it lies shows without
    /// waiting for input. True when the page has something new to read; an unchanged folder asks
    /// for no frame.
    pub(crate) fn mods_check_due(&mut self, now: std::time::Instant) -> bool {
        if !matches!(
            self.ui.page,
            Page::Installed | Page::Discover | Page::Project
        ) || !self.ui.mods.fingerprints.due(now)
        {
            return false;
        }
        let home = self.mods_home();
        let key = self.ui.mods.fingerprints.read(&home, None, now);
        self.ui
            .mods
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.home == home && snapshot.key != key)
    }

    /// The play bar's quiet line about them: Mods › Installed, where each says why.
    pub(crate) fn show_installed_mods(&mut self) {
        self.ui.mods.drawn = None;
        self.leave_section(false);
        self.mods_refresh();
        self.choose_view(Page::Installed);
    }

    /// Mods › Discover itself, whatever Mods last showed.
    pub(crate) fn show_discover(&mut self) {
        self.leave_section(false);
        self.choose_view(Page::Discover);
    }

    /// The Mods tab: where the player last was this session, else Installed.
    pub(crate) fn open_mods(&mut self) {
        self.leave_section(false);
        self.ui.page = self.ui.mods.last_mods.unwrap_or_else(|| {
            self.mods_refresh();
            Page::Installed
        });
    }

    /// The Packs tab: where the player last was this session, else Installed.
    pub(crate) fn open_packs(&mut self) {
        self.leave_section(true);
        self.ui.page = self.ui.mods.last_packs.unwrap_or_else(|| {
            self.load_packs_list(self.mods_home());
            Page::Packs
        });
    }

    /// Mods and Packs share their search fields: going from one to the other starts them empty.
    pub(super) fn leave_section(&mut self, to_packs: bool) {
        let in_packs = matches!(
            self.ui.page,
            Page::Packs | Page::PackFind | Page::PackProject
        );
        let in_mods = matches!(
            self.ui.page,
            Page::Discover | Page::Installed | Page::Project
        );
        if (to_packs && in_packs) || (!to_packs && in_mods) {
            return;
        }
        self.reset_pack_view();
        let m = &mut self.ui.mods;
        m.find.set(String::new());
        m.filters.text.clear();
        m.menu = None;
        m.packs.hint = None;
        m.data = Default::default();
    }

    /// Discover or Installed, chosen on the header's switch: remembered for the session.
    pub(super) fn choose_view(&mut self, page: Page) {
        let m = &mut self.ui.mods;
        m.nightly.open = None;
        m.nightly.collection = None;
        match page {
            Page::Discover | Page::Installed => m.last_mods = Some(page),
            _ => m.last_packs = Some(page),
        }
        m.selection.clear();
        m.anchor = None;
        m.menu = None;
        m.reveal = None;
        self.game_packs.confirm_remove = None;
        // Packs keeps the kind across the switch.
        match page {
            Page::PackFind => self.pack_scope(match self.ui.mods.shelf {
                Shelf::Resource => pack_discover::Scope::Resource,
                Shelf::Shader => pack_discover::Scope::Shader,
                Shelf::Data => pack_discover::Scope::Data,
            }),
            Page::Packs => {
                self.ui.mods.shelf = match self.ui.mods.packs.scope {
                    pack_discover::Scope::Resource => Shelf::Resource,
                    pack_discover::Scope::Shader => Shelf::Shader,
                    pack_discover::Scope::Data => Shelf::Data,
                };
                self.ui.mods.data.reread();
            }
            _ => {}
        }
        self.ui.page = page;
    }

    /// An installed mod's page that the signed registry doesn't list (a local archive, or one
    /// from the old registry): drawn from its installed row, with nothing to fetch.
    pub(super) fn mods_load_page(
        &mut self,
        id: String,
        from: Page,
        installed: Option<InstalledProject>,
    ) {
        crate::frame::scrolls("project").set_offset(gpui::point(px(0.), px(0.)));
        let scope = self.mods_page_scope();
        self.ui.mods.page_request += 1;
        let request = self.ui.mods.page_request;
        self.ui.mods.open = Some(Open {
            id,
            installed,
            collection: false,
            v2: false,
            request,
            scope,
            from,
            v2_page: None,
            parent: None,
            removing: Vec::new(),
        });
    }

    pub(super) fn mods_load_v2_page(&mut self, id: String, from: Page) {
        crate::frame::scrolls("project").set_offset(gpui::point(px(0.), px(0.)));
        let scope = self.mods_page_scope();
        let exact_build = self.selected_exact_build();
        self.ui.mods.page_request += 1;
        let request = self.ui.mods.page_request;
        let immediate = sunrise::v2::known_index().map(|catalog| {
            let local = self.local_fits(&catalog.index, exact_build.as_deref());
            model::discover_v2::page(
                &catalog.index,
                &id,
                exact_build.as_deref(),
                &crate::sheets::build_name,
                &local,
            )
            .map(Box::new)
            .ok_or_else(|| trf("{id} isn't a mod in Sunrise's registry.", &[("id", &id)]))
        });
        let needs_fetch = immediate.is_none();
        self.ui.mods.open = Some(Open {
            id: id.clone(),
            installed: None,
            collection: false,
            v2: true,
            request,
            scope,
            from,
            v2_page: immediate,
            parent: None,
            removing: Vec::new(),
        });
        self.ratings_for_page(&id);
        if !needs_fetch {
            return;
        }
        let tx = self.ui.mods.tx.clone();
        let home = self.mods_home();
        std::thread::spawn(move || {
            let result = sunrise::v2::index()
                .map_err(|e| e.to_string())
                .and_then(|catalog| {
                    let local = model::discover_v2::LocalFits::read(
                        &catalog.index,
                        exact_build.as_deref(),
                        false,
                        &home,
                    );
                    model::discover_v2::page(
                        &catalog.index,
                        &id,
                        exact_build.as_deref(),
                        &crate::sheets::build_name,
                        &local,
                    )
                    .map(Box::new)
                    .ok_or_else(|| trf("{id} isn't a mod in Sunrise's registry.", &[("id", &id)]))
                });
            let _ = tx.send(Event::V2Page(request, result));
        });
    }
}
