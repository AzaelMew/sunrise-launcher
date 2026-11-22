//! Read-only inspection and explicit update offers. Only the shared installer can publish.
use super::*;
use crate::mods_page::install::{SourcePicks, SourceTarget};
use std::collections::BTreeMap;
use sunrise_launcher::browser::sunrise::{PreparedSource, SourceError, SourceRequest};
use sunrise_launcher::github_mods::{Origin, SourceTrack};
use sunrise_launcher::native_mods::Cancellation;

pub(in crate::mods) enum Event {
    Catalogue(Catalogue),
    Detail(u64, Box<Inspection>),
    Image(sources::ReadmeImage, Option<PathBuf>),
    Icon(sources::SourceIcon, Option<PathBuf>),
    Updates(Box<CheckedUpdates>),
}

pub(in crate::mods) struct Open {
    pub(in crate::mods) source: Source,
    pub(super) from: Page,
    pub(super) scope: OpenScope,
    pub(super) request: u64,
    pub(super) inspection: Option<Inspection>,
    /// Add was pressed after a refusal: add the source once it has been read again.
    pub(super) add_when_read: bool,
}

pub(in crate::mods) struct Inspection {
    pub(super) data: sources::Details,
}

pub(in crate::mods) struct CheckedUpdates {
    scope: OpenScope,
    only: Option<String>,
    target: Option<SourceTarget>,
    candidates: BTreeMap<String, (Origin, PreparedSource)>,
    issues: BTreeMap<String, SourceError>,
    notice: Option<String>,
}

#[derive(Default)]
pub(in crate::mods) struct Work {
    pub(super) images: HashMap<sources::ReadmeImage, Option<Option<PathBuf>>>,
    pub(super) icons: HashMap<sources::SourceIcon, Option<PathBuf>>,
    pub(super) card_pins: HashMap<Source, (String, std::time::Instant)>,
    icon_misses: sources::IconMisses,
    pub(super) release_pins: HashMap<String, sources::SourceCommit>,
    checking: bool,
    updates: Option<CheckedUpdates>,
    pub(super) log: Option<(String, std::sync::Arc<str>)>,
}

impl Launcher {
    pub(in crate::mods) fn nightly_event(&mut self, event: Event) {
        let mut add = false;
        let state = &mut self.ui.mods.nightly;
        match event {
            Event::Catalogue(catalogue) => {
                state.reading = false;
                if state
                    .catalogue
                    .as_ref()
                    .is_none_or(|old| catalogue.checked_at >= old.checked_at)
                {
                    if state.catalogue.as_ref().is_none_or(|old| {
                        old.sources != catalogue.sources || old.collections != catalogue.collections
                    }) {
                        state.browse = Browse::new(&catalogue);
                        state
                            .work
                            .card_pins
                            .retain(|source, _| catalogue.sources.contains(source));
                    }
                    state.catalogue = Some(catalogue);
                }
            }
            Event::Detail(request, detail) => {
                let Some(open) = state.open.as_mut().filter(|open| open.request == request) else {
                    return;
                };
                if matches!(open.source.tracker(), sources::Tracker::Release) {
                    let pins = &mut state.work.release_pins;
                    if let Some(pin) = &detail.data.commit {
                        pins.insert(open.source.id().to_owned(), pin.clone());
                    } else {
                        pins.remove(open.source.id());
                    }
                }
                if let Some(pin) = detail
                    .data
                    .build_pin
                    .as_ref()
                    .filter(|_| detail.data.error.is_none())
                {
                    state.work.card_pins.insert(
                        open.source.clone(),
                        (pin.clone(), std::time::Instant::now()),
                    );
                } else {
                    state.work.card_pins.remove(&open.source);
                }
                open.inspection = Some(*detail);
                add = std::mem::take(&mut open.add_when_read);
            }
            Event::Image(image, path) => {
                state.work.images.insert(image, Some(path));
            }
            Event::Icon(icon, path) => {
                state.work.icons.insert(icon, path);
            }
            Event::Updates(checked) => {
                state.work.checking = false;
                state.work.updates = Some(merge_checked(state.work.updates.take(), *checked));
            }
        }
        if add {
            self.build_nightly();
        }
    }

    /// Add after a refusal: the author may have fixed the source since, so it is read again
    /// first, and added at the commit that read finds.
    pub(in crate::mods) fn reread_and_add(&mut self, source: Source) {
        self.inspect_nightly(source, true);
        if let Some(open) = &mut self.ui.mods.nightly.open {
            open.add_when_read = true;
        }
    }

    pub(in crate::mods) fn refresh_nightly_scope(&mut self) {
        let state = &self.ui.mods.nightly;
        if state.open.is_none() && state.work.updates.is_none() {
            return;
        }
        let scope = self.mods_page_scope();
        let state = &mut self.ui.mods.nightly;
        if state.open.as_ref().is_some_and(|open| open.scope != scope) {
            let from = state.open.take().map_or(Page::Discover, |open| open.from);
            if self.ui.page == Page::Project {
                self.ui.page = from;
            }
        }
        if state
            .work
            .updates
            .as_ref()
            .is_some_and(|updates| updates.scope != scope)
        {
            state.work.updates = None;
        }
    }

    pub(in crate::mods) fn inspect_nightly(&mut self, source: Source, refresh: bool) {
        let from = self
            .ui
            .mods
            .nightly
            .open
            .as_ref()
            .filter(|_| self.ui.page == Page::Project)
            .map_or(Page::Discover, |open| open.from);
        self.inspect_nightly_from(source, refresh, from);
    }

    pub(in crate::mods) fn inspect_nightly_from(
        &mut self,
        source: Source,
        refresh: bool,
        from: Page,
    ) {
        let scope = self.mods_page_scope();
        self.ui.page = Page::Project;
        if from != Page::Installed {
            self.ui.mods.open = None;
        }
        let state = &mut self.ui.mods.nightly;
        if state.open.as_ref().is_none_or(|open| open.source != source) {
            crate::frame::scrolls("project").set_offset(gpui::point(px(0.), px(0.)));
        }
        state.detail_request += 1;
        let request = state.detail_request;
        state.collection = None;
        state.open = Some(Open {
            source: source.clone(),
            from,
            scope,
            request,
            inspection: None,
            add_when_read: false,
        });
        if refresh {
            self.read_nightly_catalogue(true);
        }
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let data = sources::details(&source, refresh);
            let _ = tx.send(super::super::Event::Nightly(Event::Detail(
                request,
                Box::new(Inspection { data }),
            )));
        });
    }

    /// A README image's file once it has loaded, asking for it the first time.
    pub(in crate::mods) fn readme_picture(
        &mut self,
        image: &sources::ReadmeImage,
    ) -> Option<PathBuf> {
        match self.ui.mods.nightly.work.images.get(image) {
            Some(Some(path)) => path.clone(),
            Some(None) => None,
            None => {
                self.request_nightly_image(image.clone());
                None
            }
        }
    }

    pub(super) fn request_nightly_image(&mut self, image: sources::ReadmeImage) {
        use std::collections::hash_map::Entry;
        let Entry::Vacant(entry) = self.ui.mods.nightly.work.images.entry(image.clone()) else {
            return;
        };
        entry.insert(None);
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let path = sources::readme_image(&image).ok();
            let _ = tx.send(super::super::Event::Nightly(Event::Image(image, path)));
        });
    }

    pub(super) fn request_nightly_icon(&mut self, icon: sources::SourceIcon) {
        use std::collections::hash_map::Entry;
        let Entry::Vacant(entry) = self.ui.mods.nightly.work.icons.entry(icon.clone()) else {
            return;
        };
        entry.insert(None);
        let tx = self.ui.mods.tx.clone();
        let misses = self.ui.mods.nightly.work.icon_misses.clone();
        std::thread::spawn(move || {
            let path = icon.load(&misses);
            let _ = tx.send(super::super::Event::Nightly(Event::Icon(icon, path)));
        });
    }

    pub(super) fn nightly_busy(&self) -> bool {
        self.mods_locked().is_some()
            || (!self.mutating()
                && self
                    .ui
                    .mods
                    .snapshot
                    .as_ref()
                    .is_none_or(|snap| !snap.ready()))
    }

    pub(crate) fn source_picks_listed(&self, picks: &[Source]) -> bool {
        self.ui
            .mods
            .nightly
            .catalogue
            .as_ref()
            .is_some_and(|list| picks.iter().all(|source| list.sources.contains(source)))
    }

    pub(super) fn build_nightly(&mut self) {
        if !self.nightly_catalogue_current() || self.nightly_busy() {
            return;
        }
        let scope = self.mods_page_scope();
        let Some(open) = self
            .ui
            .mods
            .nightly
            .open
            .as_ref()
            .filter(|open| open.scope == scope)
        else {
            return;
        };
        if !self
            .discover_installed()
            .is_some_and(|rows| rows.source_presence(&open.source) == SourcePresence::Missing)
        {
            return;
        }
        if !self.source_picks_listed(std::slice::from_ref(&open.source)) {
            self.ui.mods.notice = Some(Err(tr(
                "This source listing changed. Open it again from Discover.",
            )
            .into()));
            return;
        }
        if let Some(pin) = open
            .inspection
            .as_ref()
            .and_then(|inspection| inspection.data.checked_commit())
            .cloned()
        {
            self.add_sources(SourcePicks::One(Box::new(pin)));
        }
    }

    pub(in crate::mods) fn nightly_catalogue_current(&self) -> bool {
        self.ui.mods.nightly.catalogue.as_ref().is_some_and(|list| {
            list.saved_at.is_some()
                && list.error.is_none()
                && list.checked_at.elapsed().is_ok_and(|age| age < FRESH)
        })
    }

    pub(in crate::mods) fn check_source_updates(&mut self, only: Option<&Origin>) {
        if self.ui.mods.nightly.work.checking || !self.guard() {
            return;
        }
        let scope = self.mods_page_scope();
        let target = match self.source_target() {
            Ok(target) => target,
            Err(error) => {
                self.ui.mods.notice = Some(Err(error));
                return;
            }
        };
        let Some(rows) = self.discover_installed() else {
            return;
        };
        let origins = tracked_origins(&rows.local, only);
        if origins.is_empty() {
            return;
        }
        let only = only.map(|origin| origin.id.clone());
        self.ui.mods.nightly.work.checking = true;
        // One project's check keeps what the last profile-wide check found for the others.
        if only.is_none() {
            self.ui.mods.nightly.work.updates = None;
        }
        let refresh_catalogue = !self.nightly_catalogue_current();
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let mut checked = CheckedUpdates {
                scope,
                only,
                target: None,
                candidates: BTreeMap::new(),
                issues: BTreeMap::new(),
                notice: None,
            };
            let catalogue = sources::catalogue(refresh_catalogue);
            if catalogue.error.is_some() {
                checked.notice = Some(tr("No source updates were checked while offline.").into());
            } else {
                let cancellation = Cancellation::default();
                let mut requested = Vec::new();
                let mut predecessors = Vec::new();
                for origin in origins {
                    match source_for_update(&origin) {
                        Ok(Some(source)) => {
                            requested.push(source);
                            predecessors.push(origin);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            checked.issues.insert(origin.id, SourceError::Github(error));
                        }
                    }
                }
                for (origin, pin) in predecessors
                    .into_iter()
                    .zip(sources::resolve_batch(&requested))
                {
                    let pin = match pin {
                        Ok(pin) if pin.sha() == origin.commit => continue,
                        Ok(pin) => pin,
                        Err(error) => {
                            checked.issues.insert(origin.id, SourceError::Github(error));
                            continue;
                        }
                    };
                    match sunrise::prepare_source(
                        target.instance(),
                        target.identity(),
                        target.base(),
                        SourceRequest::Update {
                            pin,
                            predecessor: Box::new(origin.clone()),
                        },
                        &cancellation,
                    ) {
                        Ok(prepared) => {
                            checked
                                .candidates
                                .insert(origin.id.clone(), (origin, prepared));
                        }
                        Err(error) => {
                            checked.issues.insert(origin.id, error);
                        }
                    }
                }
                checked.notice = Some(
                    if checked.candidates.is_empty() && checked.issues.is_empty() {
                        tr("No source updates available.").into()
                    } else {
                        tr("Finished checking for source updates.").into()
                    },
                );
                checked.target = Some(target);
            }
            for (id, error) in &checked.issues {
                eprintln!("Nightly source update preflight ({id}): {error}");
            }
            let _ = tx.send(super::super::Event::Nightly(Event::Catalogue(catalogue)));
            let _ = tx.send(super::super::Event::Nightly(Event::Updates(Box::new(
                checked,
            ))));
        });
    }

    pub(in crate::mods) fn source_updates_checking(&self) -> bool {
        self.ui.mods.nightly.work.checking
    }

    pub(in crate::mods) fn source_update_version(&self, origin: &Origin) -> Option<&str> {
        if !self.nightly_catalogue_current() {
            return None;
        }
        let (predecessor, prepared) = self.source_updates_here()?.candidates.get(&origin.id)?;
        (predecessor == origin).then_some(prepared.origin().version.as_str())
    }

    pub(in crate::mods) fn source_update_issue(&self, id: &str) -> Option<&SourceError> {
        self.source_updates_here()?.issues.get(id)
    }

    pub(in crate::mods) fn source_update_notice(&self, only: Option<&str>) -> Option<&str> {
        let checked = self.source_updates_here()?;
        (checked.only.as_deref() == only)
            .then_some(checked.notice.as_deref())
            .flatten()
    }

    fn source_updates_here(&self) -> Option<&CheckedUpdates> {
        self.ui.mods.nightly.work.updates.as_ref()
    }

    pub(in crate::mods) fn submit_source_update(&mut self, id: &str) {
        if !self.guard() {
            return;
        }
        if let Some((target, sources)) = self.take_source_updates(Some(id)) {
            if let Err(error) = self.queue_sources(target, sources) {
                self.ui.mods.notice = Some(Err(error));
            }
        }
    }

    pub(in crate::mods) fn submit_mod_update_all(
        &mut self,
        displayed_names: BTreeMap<String, String>,
    ) {
        if !self.guard() {
            return;
        }
        let registry: Vec<_> = self
            .rows()
            .into_iter()
            .filter(model::bulk_updatable)
            .map(|row| row.id)
            .collect();
        let Some((target, sources)) = self.take_source_updates(None) else {
            if !registry.is_empty() {
                self.ask_update(false);
            }
            return;
        };
        self.ui.mods.notice = None;
        self.mods.message = None;
        let result = if registry.is_empty() {
            self.queue_sources(target, sources)
        } else {
            self.queue_all_mod_updates(target, registry, displayed_names, sources)
        };
        if let Err(error) = result {
            self.ui.mods.notice = Some(Err(error));
        }
    }

    fn take_source_updates(
        &mut self,
        id: Option<&str>,
    ) -> Option<(SourceTarget, Vec<PreparedSource>)> {
        if !self.nightly_catalogue_current() {
            return None;
        }
        let scope = self.mods_page_scope();
        let installed = self
            .ui
            .mods
            .snapshot
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| &snapshot.rows.local)
            .filter_map(|local| local.github.as_ref());
        let checked = self
            .ui
            .mods
            .nightly
            .work
            .updates
            .as_mut()
            .filter(|updates| updates.scope == scope)?;
        let target = checked.target.clone()?;
        let wanted: Vec<_> = installed
            .filter(|origin| id.is_none_or(|id| id == origin.id))
            .filter(|origin| {
                checked
                    .candidates
                    .get(&origin.id)
                    .is_some_and(|(predecessor, _)| predecessor == *origin)
            })
            .map(|origin| origin.id.clone())
            .collect();
        let inputs: Vec<_> = wanted
            .into_iter()
            .filter_map(|id| checked.candidates.remove(&id))
            .map(|(_, prepared)| prepared)
            .collect();
        (!inputs.is_empty()).then_some((target, inputs))
    }
}

/// What a check found, laid over the last one: a profile-wide check replaces it, and one
/// project's check replaces only that project's offer or issue, on the same profile and build.
/// Offline, nothing was checked, so every earlier finding stays.
fn merge_checked(previous: Option<CheckedUpdates>, checked: CheckedUpdates) -> CheckedUpdates {
    let Some(id) = checked.only.clone() else {
        return checked;
    };
    let same_target = |earlier: &Option<SourceTarget>| match (earlier, &checked.target) {
        (Some(earlier), Some(now)) => {
            earlier.identity() == now.identity() && earlier.base() == now.base()
        }
        (_, None) => true,
        (None, Some(_)) => false,
    };
    let Some(mut merged) =
        previous.filter(|earlier| earlier.scope == checked.scope && same_target(&earlier.target))
    else {
        return checked;
    };
    if checked.target.is_some() {
        merged.candidates.remove(&id);
        merged.issues.remove(&id);
        merged.target = checked.target;
    }
    merged.candidates.extend(checked.candidates);
    merged.issues.extend(checked.issues);
    merged.only = checked.only;
    merged.notice = checked.notice;
    merged
}

fn tracked_origins(local: &[model::Local], only: Option<&Origin>) -> Vec<Origin> {
    local
        .iter()
        .filter_map(|local| local.github.as_ref())
        .filter(|origin| only.is_none_or(|only| only == *origin))
        .filter(|origin| {
            origin
                .source
                .as_ref()
                .is_some_and(|source| !matches!(source.track, SourceTrack::Pinned))
        })
        .cloned()
        .collect()
}

fn source_for_update(predecessor: &Origin) -> Result<Option<Source>, sources::Failure> {
    let Some(source) = predecessor.source.as_ref() else {
        return Ok(None);
    };
    let tracker = match &source.track {
        SourceTrack::Branch(branch) => sources::Tracker::Branch(branch.clone()),
        SourceTrack::Release => sources::Tracker::Release,
        SourceTrack::Pinned => return Ok(None),
    };
    Source::for_origin(
        &predecessor.id,
        &format!("{}/{}", predecessor.owner, predecessor.repo),
        source.repo_id,
        &source.manifest,
        tracker,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_projects_check_keeps_the_other_mods_findings() {
        let scope = || OpenScope {
            profile: Some("p".into()),
            home: PathBuf::from("home"),
            target: None,
            game: "nightly".into(),
        };
        let issue = |why: &str| SourceError::Unproved(why.into());
        let earlier = CheckedUpdates {
            scope: scope(),
            only: None,
            target: None,
            candidates: BTreeMap::new(),
            issues: BTreeMap::from([("a".into(), issue("old a")), ("b".into(), issue("b"))]),
            notice: Some("Finished checking for source updates.".into()),
        };
        let one = CheckedUpdates {
            scope: scope(),
            only: Some("a".into()),
            target: None,
            candidates: BTreeMap::new(),
            issues: BTreeMap::from([("a".into(), issue("new a"))]),
            notice: Some("Finished checking for source updates.".into()),
        };
        let merged = merge_checked(Some(earlier), one);
        assert_eq!(merged.issues.get("b"), Some(&issue("b")));
        assert_eq!(merged.issues.get("a"), Some(&issue("new a")));
        assert_eq!(merged.only.as_deref(), Some("a"));

        // A profile-wide check replaces everything.
        let all = CheckedUpdates {
            scope: scope(),
            only: None,
            target: None,
            candidates: BTreeMap::new(),
            issues: BTreeMap::new(),
            notice: None,
        };
        assert!(merge_checked(Some(merged), all).issues.is_empty());
    }

    #[test]
    fn project_check_uses_only_its_exact_current_receipt() {
        let (_, first) = super::super::tests::receipt_fixture();
        let mut second = first.clone();
        second.id = "other".into();
        second.github.as_mut().unwrap().id = second.id.clone();
        let mut pinned = first.clone();
        pinned
            .github
            .as_mut()
            .unwrap()
            .source
            .as_mut()
            .unwrap()
            .track = SourceTrack::Pinned;
        let only = first.github.clone().unwrap();
        let other = second.github.clone().unwrap();
        let locals = [first, second, pinned];
        assert_eq!(tracked_origins(&locals, None), [only.clone(), other]);
        assert_eq!(
            tracked_origins(&locals, Some(&only)),
            std::slice::from_ref(&only)
        );
        let stale = Origin {
            commit: "9".repeat(40),
            ..only
        };
        assert!(tracked_origins(&locals, Some(&stale)).is_empty());
    }
}
