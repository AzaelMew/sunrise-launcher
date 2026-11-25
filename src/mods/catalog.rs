//! Discover's catalogue: the registry index in memory, the selected build's fit checks,
//! and the cards and collections read from them.

use super::*;

pub(super) struct V2CatalogPayload {
    pub(super) cards: Vec<model::discover_v2::V2Card>,
    pub(super) collections: Vec<model::discover_v2::V2Collection>,
    pub(super) categories: Vec<String>,
    /// The index lists the selected profile's build among its offered builds.
    pub(super) build_listed: bool,
    pub(super) stale_warning: Option<String>,
    pub(super) from_cache: bool,
}

pub(super) enum V2Catalog {
    Idle,
    Loading,
    Ready {
        cards: Rc<[model::discover_v2::V2Card]>,
        collections: Rc<[model::discover_v2::V2Collection]>,
        categories: Rc<[String]>,
        build_listed: bool,
        stale_warning: Option<String>,
        from_cache: bool,
    },
    Failed(String),
}

/// This computer's fit check of the releases the index says nothing of on one build.
#[derive(Default)]
pub(super) struct LocalFitRun {
    /// The build checked last, or being checked.
    pub(super) build: Option<String>,
    pub(super) running: bool,
    /// Why the last check of `build` stopped; it isn't started again until Try again.
    pub(super) failed: Option<String>,
}

impl Launcher {
    /// The exact Sunrise build ID of `instance` for v2 fit checks (unwrapping native-built tags
    /// back to their base release build).
    pub(crate) fn profile_exact_build(&self, instance: &instances::Instance) -> Option<String> {
        let raw = self.profile_target(instance)?;
        if native_mods::is_native(&raw) {
            crate::profile_builds::native_base(&raw).or(Some(raw))
        } else {
            Some(raw)
        }
    }

    pub(crate) fn selected_exact_build(&self) -> Option<String> {
        let instance = self.selected_instance()?;
        self.profile_exact_build(instance)
    }

    /// This computer's fit checks of `build`, as Discover shows them.
    pub(super) fn local_fits(
        &self,
        index: &sunrise_launcher_smod::index_v2::Index,
        build: Option<&str>,
    ) -> model::discover_v2::LocalFits {
        let run = &self.ui.mods.local_fit;
        let running = run.running && run.build.as_deref() == build;
        model::discover_v2::LocalFits::read(index, build, running, &self.mods_home())
    }

    /// Starts this computer's fit check of the releases the index says nothing of on the
    /// selected build, in the background: one at a time, and not again for a build whose last
    /// check failed until Try again.
    pub(super) fn start_local_fit(&mut self) {
        let run = &self.ui.mods.local_fit;
        let Some(build) = self.selected_exact_build() else {
            return;
        };
        if run.running || (run.build.as_ref() == Some(&build) && run.failed.is_some()) {
            return;
        }
        let Some(catalog) = sunrise::v2::known_index() else {
            return;
        };
        // A build the index doesn't list is checked too, by the build id its own release names
        // (known once the first check has read it).
        let found = local_fit::commit_of(&catalog.index, &build)
            .map(|commit| local_fit::kept(&commit))
            .unwrap_or_default();
        if local_fit::gaps(&catalog.index, &build)
            .iter()
            .all(|r| found.contains_key(&r.file.sha256.to_ascii_lowercase()))
        {
            return;
        }
        self.ui.mods.local_fit = LocalFitRun {
            build: Some(build.clone()),
            running: true,
            failed: None,
        };
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let result = local_fit::check(&catalog.index, &build);
            let _ = tx.send(Event::LocalFit(build, result));
        });
        self.v2_cards_again();
    }

    /// Try again after the selected build's fit check failed.
    pub(crate) fn retry_local_fit(&mut self) {
        self.ui.mods.local_fit.failed = None;
        self.start_local_fit();
    }

    /// Why the selected build's fit check failed, when it did.
    pub(crate) fn local_fit_failure(&self) -> Option<&str> {
        let run = &self.ui.mods.local_fit;
        (run.build == self.selected_exact_build())
            .then_some(run.failed.as_deref())
            .flatten()
    }

    /// Discover's cards and an open mod page drawn again from the index in memory, with what
    /// this computer's checks have found so far.
    pub(super) fn v2_cards_again(&mut self) {
        let Some(catalog) = sunrise::v2::known_index() else {
            return;
        };
        let build = self.selected_exact_build();
        let local = self.local_fits(&catalog.index, build.as_deref());
        let name = &crate::sheets::build_name;
        if self.ui.mods.v2_catalog_build.as_ref() == Some(&build) {
            if let V2Catalog::Ready {
                cards, collections, ..
            } = &mut self.ui.mods.v2_catalog
            {
                let fresh =
                    model::discover_v2::cards(&catalog.index, build.as_deref(), name, &local);
                *collections = model::discover_v2::collections(&catalog.index, &fresh).into();
                *cards = fresh.into();
            }
        }
        if let Some(Ok(page)) = self
            .ui
            .mods
            .open
            .as_mut()
            .and_then(|open| open.v2_page.as_mut())
        {
            let id = page.id.clone();
            if let Some(fresh) =
                model::discover_v2::page(&catalog.index, &id, build.as_deref(), name, &local)
            {
                **page = fresh;
            }
        }
    }

    pub(crate) fn ensure_v2_catalog(&mut self, force: bool) {
        let build = self.selected_exact_build();
        if !force
            && self.ui.mods.v2_catalog_build.as_ref() == Some(&build)
            && !matches!(self.ui.mods.v2_catalog, V2Catalog::Idle)
        {
            return;
        }
        if let Some(catalog) = (!force).then(sunrise::v2::known_index).flatten() {
            let local = self.local_fits(&catalog.index, build.as_deref());
            let cards = model::discover_v2::cards(
                &catalog.index,
                build.as_deref(),
                &crate::sheets::build_name,
                &local,
            );
            let categories = model::discover_v2::categories(&cards);
            let collections = model::discover_v2::collections(&catalog.index, &cards);
            let build_listed = model::discover_v2::build_listed(&catalog.index, build.as_deref());
            self.ui.mods.v2_failure = None;
            self.ui.mods.v2_catalog_build = Some(build);
            self.ui.mods.v2_catalog = V2Catalog::Ready {
                cards: cards.into(),
                collections: collections.into(),
                categories: categories.into(),
                build_listed,
                stale_warning: catalog.stale_warning,
                from_cache: catalog.from_cache,
            };
            self.start_local_fit();
            return;
        }
        self.ui.mods.v2_catalog_build = Some(build.clone());
        self.ui.mods.v2_catalog = V2Catalog::Loading;
        self.ui.mods.v2_failure = None;
        let tx = self.ui.mods.tx.clone();
        let home = self.mods_home();
        std::thread::spawn(move || {
            // Installed starts from the snapshot on disk while the network read is out.
            if !force && sunrise::v2::read_saved_index().is_some() {
                let _ = tx.send(Event::V2Saved);
            }
            let loaded = if force {
                sunrise::v2::refreshed_index()
            } else {
                sunrise::v2::index()
            };
            let result = loaded.map(|catalog| {
                // What a running check finds shows once it answers.
                let local = model::discover_v2::LocalFits::read(
                    &catalog.index,
                    build.as_deref(),
                    false,
                    &home,
                );
                let cards = model::discover_v2::cards(
                    &catalog.index,
                    build.as_deref(),
                    &crate::sheets::build_name,
                    &local,
                );
                let categories = model::discover_v2::categories(&cards);
                let collections = model::discover_v2::collections(&catalog.index, &cards);
                V2CatalogPayload {
                    collections,
                    build_listed: model::discover_v2::build_listed(
                        &catalog.index,
                        build.as_deref(),
                    ),
                    cards,
                    categories,
                    stale_warning: catalog.stale_warning,
                    from_cache: catalog.from_cache,
                }
            });
            let _ = tx.send(Event::V2Catalog(build, result));
        });
    }
}
