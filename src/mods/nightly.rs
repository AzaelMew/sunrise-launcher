//! Nightly and Dev Discover: source listings and collections, without inspecting them on scroll.
use super::discover::{collection_install_fact, discover_filters, install_fact, PAGE};
use super::ui::*;
use super::*;
use crate::frame::button;
use std::time::{Duration, SystemTime};
pub(super) use sunrise_launcher::browser::sunrise::sources::Source;
use sunrise_launcher::browser::sunrise::sources::{self, Catalogue};

mod collection;
mod jobs;
mod project;

pub(super) use jobs::Event;
pub(super) use project::{short_sha, source_error};

const FRESH: Duration = Duration::from_secs(600);
pub(super) const SOURCE_ID_CONFLICT: &str = "Another installed mod uses this ID.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourcePresence<'a> {
    Missing,
    Installed {
        origin: &'a sunrise_launcher::github_mods::Origin,
        file: &'a str,
    },
    Conflict,
}

impl InstalledRows {
    fn source_project<'a>(
        &self,
        id: &str,
        file: &str,
        catalogue: Option<&'a Catalogue>,
    ) -> Option<&'a Source> {
        let source = find_source(catalogue?, id)?;
        match self.source_presence(source) {
            SourcePresence::Installed { file: owned, .. } if owned == file => Some(source),
            _ => None,
        }
    }

    fn source_presence(&self, source: &Source) -> SourcePresence<'_> {
        let id = source.id();
        if self
            .entries
            .iter()
            .any(|entry| sunrise::mod_id(entry) == Some(id))
        {
            return SourcePresence::Conflict;
        }
        let mut local = self.local.iter().filter(|row| row.id == id);
        let Some(row) = local.next() else {
            return SourcePresence::Missing;
        };
        if local.next().is_some() {
            return SourcePresence::Conflict;
        }
        let Some(origin) = &row.github else {
            return SourcePresence::Conflict;
        };
        if source_matches_origin(source, origin) {
            SourcePresence::Installed {
                origin,
                file: &row.file,
            }
        } else {
            SourcePresence::Conflict
        }
    }
}

fn find_source<'a>(catalogue: &'a Catalogue, id: &str) -> Option<&'a Source> {
    let i = catalogue
        .sources
        .binary_search_by(|source| source.id().cmp(id))
        .ok()?;
    Some(&catalogue.sources[i])
}

fn source_matches_origin(source: &Source, origin: &sunrise_launcher::github_mods::Origin) -> bool {
    source.id() == origin.id
        && origin.owner.eq_ignore_ascii_case(source.owner())
        && origin.source.as_ref().is_some_and(|receipt| {
            receipt.repo_id == source.repo_id() && receipt.manifest == source.manifest()
        })
}

#[derive(Default)]
pub(super) struct State {
    catalogue: Option<Catalogue>,
    reading: bool,
    pub(super) open: Option<jobs::Open>,
    detail_request: u64,
    browse: Browse,
    kind: SourceKind,
    descending: bool,
    pub(super) work: jobs::Work,
    pub(super) collection: Option<collection::CollectionOpen>,
}

impl Launcher {
    pub(super) fn discover_profile_line(&self, show_source_warning: bool) -> Div {
        let target = self.discover_target_name();
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x(px(10.))
            .gap_y(px(4.))
            .when(show_source_warning, |line| {
                line.child(muted(tr(
                    "Add from GitHub. Unreviewed source code builds when you play.",
                )))
            })
            .child(div().flex_1())
            .child(muted(trf("Profile: {build}", &[("build", &target)])))
    }

    pub(super) fn discover_target_name(&self) -> String {
        self.selected_instance()
            .and_then(|instance| self.profile_target(instance))
            .map_or_else(
                || tr("No game build selected").into(),
                |tag| crate::sheets::build_name(&tag),
            )
    }

    pub(super) fn read_nightly_catalogue(&mut self, force: bool) {
        let n = &mut self.ui.mods.nightly;
        if n.reading
            || (!force
                && n.catalogue
                    .as_ref()
                    .is_some_and(|c| c.checked_at.elapsed().is_ok_and(|time| time < FRESH)))
        {
            return;
        }
        n.reading = true;
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(super::Event::Nightly(Event::Catalogue(sources::catalogue(
                force,
            ))));
        });
    }

    pub(super) fn nightly_discover(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.read_nightly_catalogue(false);
        let mut page = self.discover_shell(true, window, focus, cx);
        let state = &mut self.ui.mods.nightly;
        state
            .browse
            .set_query(&self.ui.mods.filters.text, state.kind);
        let state = &self.ui.mods.nightly;
        let Some(catalogue) = &state.catalogue else {
            return page
                .child(muted(tr("Loading source projects…")))
                .into_any_element();
        };
        if catalogue.error.is_some() {
            let retry = button("nightly-catalogue-retry", tr("Try again"));
            let retry = if self.ui.mods.nightly.reading {
                dim(retry)
            } else {
                retry.on_click(cx.listener(|this, _, _, cx| {
                    this.read_nightly_catalogue(true);
                    cx.notify();
                }))
            };
            let (line, cached) = match catalogue.saved_at {
                Some(time) => (
                    trf(
                        "You're offline · showing the saved list from {date}",
                        &[("date", &source_time(time))],
                    ),
                    true,
                ),
                None => (tr("Source list unavailable offline").to_owned(), false),
            };
            page = page.child(
                card()
                    .p(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(line))
                    .child(retry),
            );
            if !cached {
                return page.into_any_element();
            }
        }
        let shown = &state.browse.matches;
        let total = shown.len();
        let visible = (0..self.ui.mods.limit.max(PAGE).min(total))
            .map(|i| shown[if state.descending { total - i - 1 } else { i }]);
        let allow_fetch = catalogue.error.is_none();
        let build = self.nightly_build_revision();
        let installed = self.discover_installed();
        let work = &state.work;
        let mut grid = self.discover_grid();
        for listing in visible {
            let view = match listing {
                Listing::Collection(i) => {
                    let collection = &catalogue.collections[i];
                    let icon = sources::SourceIcon::pages(&collection.icon, allow_fetch);
                    let picture = work.icons.get(&icon).cloned().flatten();
                    let load = (!work.icons.contains_key(&icon))
                        .then(|| when_visible(cx, move |this| this.request_nightly_icon(icon)));
                    let mut facts = vec![
                        badge(tr("Collection")),
                        muted(trf(
                            "{count} mods",
                            &[("count", &collection.members.len().to_string())],
                        )),
                    ];
                    if let Some(rows) = installed {
                        facts.extend(collection_install_fact(
                            collection,
                            |id| {
                                catalogue
                                    .sources
                                    .binary_search_by(|source| source.id().cmp(id))
                                    .is_ok_and(|i| {
                                        matches!(
                                            rows.source_presence(&catalogue.sources[i]),
                                            SourcePresence::Installed { .. }
                                        )
                                    })
                            },
                            |id| rows.unbuilt.contains(id),
                        ));
                    }
                    let collections = catalogue.collections.clone();
                    browse_card(
                        ("nightly-collection", i),
                        picture,
                        &collection.id,
                        &collection.name,
                        &collection.owners.join(", "),
                        &collection.summary,
                        facts,
                        None,
                        None,
                    )
                    .relative()
                    .children(load)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_source_collection(&collections[i]);
                        cx.notify();
                    }))
                }
                Listing::Source { index: i, part_of } => {
                    let source = &catalogue.sources[i];
                    let (picture, load) = self.source_artwork(source, None, allow_fetch);
                    let load = load
                        .map(|icon| when_visible(cx, move |this| this.request_nightly_icon(icon)));
                    let mut facts = vec![tag(model::Make::Native.label())];
                    // A card never makes a GitHub request just for a badge. Only a recently
                    // verified detail supplies the pin, never a floating branch manifest.
                    if allow_fetch
                        && work.card_pins.get(source).is_some_and(|(pin, read)| {
                            read.elapsed() < FRESH && made_for_build(build.as_deref(), Some(pin))
                        })
                    {
                        facts.push(badge(tr("Made for this build")));
                    }
                    if let Some(i) = part_of {
                        facts.push(muted(trf(
                            "Part of {collection}",
                            &[("collection", &catalogue.collections[i].name)],
                        )));
                    }
                    let package = installed.and_then(|rows| match rows.source_presence(source) {
                        SourcePresence::Installed { origin, file } => {
                            facts.push(install_fact(
                                short_sha(&origin.commit),
                                rows.unbuilt.contains(source.id()),
                            ));
                            rows.archives.get(file)?.icon.clone()
                        }
                        SourcePresence::Conflict => {
                            facts.push(status(
                                "information-circle",
                                tr(SOURCE_ID_CONFLICT),
                                palette().warning,
                            ));
                            None
                        }
                        SourcePresence::Missing => None,
                    });
                    let sources = catalogue.sources.clone();
                    browse_card(
                        ("nightly-card", i),
                        mod_artwork(picture, package),
                        source.id(),
                        source.name(),
                        source.owner(),
                        source.summary(),
                        facts,
                        None,
                        None,
                    )
                    .relative()
                    .children(load)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.inspect_nightly(sources[i].clone(), false);
                        cx.notify();
                    }))
                }
            };
            grid = grid.child(view);
        }
        let kind_options = || {
            SourceKind::ALL
                .into_iter()
                .map(|kind| {
                    (
                        kind.label().into(),
                        kind == state.kind,
                        Box::new(move |this: &mut Launcher| {
                            this.ui.mods.nightly.kind = kind;
                            this.reset_source_results();
                        }) as Box<dyn Fn(&mut Launcher)>,
                    )
                })
                .collect()
        };
        let name_order = |reverse| {
            if reverse {
                tr("Name Z–A")
            } else {
                tr("Name A–Z")
            }
        };
        let sort_options = || {
            [false, true]
                .into_iter()
                .map(|reverse| {
                    (
                        name_order(reverse).into(),
                        reverse == state.descending,
                        Box::new(move |this: &mut Launcher| {
                            this.ui.mods.nightly.descending = reverse;
                            this.reset_source_results();
                        }) as Box<dyn Fn(&mut Launcher)>,
                    )
                })
                .collect()
        };
        let kind = self.discover_menu(state.kind.label(), Menu::Make, kind_options, cx);
        let order = self.discover_menu(name_order(state.descending), Menu::Sort, sort_options, cx);
        page = page.child(discover_filters(
            div()
                .flex()
                .items_center()
                .gap(px(18.))
                .flex_wrap()
                .child(kind)
                .child(muted(if total == 1 {
                    tr("1 source project").to_owned()
                } else {
                    trf("{count} source projects", &[("count", &total.to_string())])
                })),
            order,
        ));
        page = page.child(self.discover_results(
            grid,
            total,
            "nightly-discover",
            tr("No source projects match."),
            cx,
        ));
        page.into_any_element()
    }

    pub(super) fn reset_source_results(&mut self) {
        self.ui.mods.limit = 0;
        crate::frame::scrolls("nightly-discover").set_offset(gpui::point(px(0.), px(0.)));
    }

    pub(super) fn installed_source_project(&self, id: &str, file: &str) -> Option<&Source> {
        self.discover_installed()?
            .source_project(id, file, self.ui.mods.nightly.catalogue.as_ref())
    }

    pub(super) fn source_listing(
        &self,
        origin: &sunrise_launcher::github_mods::Origin,
    ) -> Option<&Source> {
        let catalogue = self.ui.mods.nightly.catalogue.as_ref()?;
        let source = find_source(catalogue, &origin.id)?;
        source_matches_origin(source, origin).then_some(source)
    }

    pub(super) fn source_collection_for(
        &self,
        source: &Source,
    ) -> Option<&sunrise_launcher_smod::index::Collection> {
        self.ui
            .mods
            .nightly
            .catalogue
            .as_ref()?
            .collections
            .iter()
            .find(|collection| {
                collection
                    .members
                    .iter()
                    .any(|member| member.id == source.id())
            })
    }

    fn source_artwork(
        &self,
        source: &Source,
        commit: Option<&sources::SourceCommit>,
        allow_fetch: bool,
    ) -> (Option<PathBuf>, Option<sources::SourceIcon>) {
        let work = &self.ui.mods.nightly.work;
        let commit = commit.or_else(|| {
            work.release_pins
                .get(source.id())
                .filter(|pin| pin.source() == source)
        });
        let icon = sources::SourceIcon::new(source, commit, allow_fetch);
        let loaded = icon.as_ref().and_then(|icon| work.icons.get(icon));
        let picture = loaded.cloned().flatten();
        (picture, icon.filter(|_| loaded.is_none()))
    }

    pub(super) fn source_tile(
        &self,
        source: &Source,
        commit: Option<&sources::SourceCommit>,
        allow_fetch: bool,
        package: Option<std::sync::Arc<gpui::Image>>,
        side: f32,
        cx: &Context<Self>,
    ) -> Div {
        let (listed, load) = self.source_artwork(source, commit, allow_fetch);
        let picture = mod_artwork(listed, package);
        tile(picture, source.id(), source.name(), side)
            .relative()
            .children(
                load.map(|icon| when_visible(cx, move |this| this.request_nightly_icon(icon))),
            )
    }

    fn nightly_build_revision(&self) -> Option<String> {
        backend::build_revision(&self.profile_target(self.selected_instance()?)?)
    }
}

fn when_visible(
    cx: &Context<Launcher>,
    load: impl FnOnce(&mut Launcher) + 'static,
) -> impl IntoElement + 'static {
    let entity = cx.entity().downgrade();
    gpui::canvas(
        move |bounds, window, cx| {
            if window.content_mask().bounds.intersects(&bounds) {
                cx.defer(move |cx| {
                    entity.update(cx, |this, _| load(this)).ok();
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

fn made_for_build(profile: Option<&str>, pin: Option<&str>) -> bool {
    // The manifest validated a full pin; the profile returned its full commit or a valid prefix.
    profile
        .zip(pin)
        .is_some_and(|(profile, pin)| pin.starts_with(profile))
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SourceKind {
    #[default]
    Any,
    Mods,
    Collections,
}

impl SourceKind {
    const ALL: [Self; 3] = [Self::Any, Self::Mods, Self::Collections];

    fn label(self) -> &'static str {
        match self {
            Self::Any => tr("Any kind"),
            Self::Mods => tr("Source mods"),
            Self::Collections => tr("Collections"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Listing {
    Source {
        index: usize,
        part_of: Option<usize>,
    },
    Collection(usize),
}

#[derive(Default)]
struct Browse {
    /// Name-ordered display indexes and pre-folded search text for this catalogue.
    listings: Vec<(Listing, String)>,
    query: Option<(String, SourceKind)>,
    matches: Vec<Listing>,
}

impl Browse {
    fn new(catalogue: &Catalogue) -> Self {
        let mut membership = HashMap::new();
        let collections = catalogue
            .collections
            .iter()
            .enumerate()
            .map(|(index, collection)| {
                for member in &collection.members {
                    membership.entry(member.id.as_str()).or_insert(index);
                }
                (
                    Listing::Collection(index),
                    collection.name.as_str(),
                    collection.id.as_str(),
                    format!(
                        "{} {} {}",
                        collection.name, collection.id, collection.summary
                    ),
                )
            });
        let mut listings: Vec<_> = collections.collect();
        listings.extend(catalogue.sources.iter().enumerate().map(|(index, source)| {
            (
                Listing::Source {
                    index,
                    part_of: membership.get(source.id()).copied(),
                },
                source.name(),
                source.id(),
                format!(
                    "{} {} {} {}",
                    source.name(),
                    source.id(),
                    source.repo(),
                    source.summary()
                ),
            )
        }));
        listings.sort_by_cached_key(|(_, name, id, _)| (name.to_lowercase(), *id));
        Self {
            listings: listings
                .into_iter()
                .map(|(listing, _, _, text)| (listing, text.to_lowercase()))
                .collect(),
            ..Self::default()
        }
    }

    fn set_query(&mut self, search: &str, kind: SourceKind) {
        if self
            .query
            .as_ref()
            .is_some_and(|(last, old_kind)| last == search && *old_kind == kind)
        {
            return;
        }
        let words: Vec<_> = search.split_whitespace().map(str::to_lowercase).collect();
        self.matches.clear();
        self.matches
            .extend(self.listings.iter().filter_map(|(listing, text)| {
                let kind_matches = match (kind, listing) {
                    // An unfiltered browse folds members into their collection. A search, or a
                    // deliberate choice of Source mods, finds the individual members too.
                    (SourceKind::Any, Listing::Source { part_of, .. }) => {
                        part_of.is_none() || !words.is_empty()
                    }
                    (SourceKind::Any, Listing::Collection(_)) => true,
                    (SourceKind::Mods, Listing::Source { .. })
                    | (SourceKind::Collections, Listing::Collection(_)) => true,
                    _ => false,
                };
                (kind_matches && words.iter().all(|word| text.contains(word))).then_some(*listing)
            }));
        self.query = Some((search.to_owned(), kind));
    }
}

pub(super) fn installed_source(row: &Row) -> Option<&sunrise_launcher::github_mods::Origin> {
    row.local
        .as_ref()?
        .github
        .as_ref()
        .filter(|origin| origin.source.is_some())
}

pub(super) fn source_provenance(origin: &sunrise_launcher::github_mods::Origin) -> String {
    let repo = format!("{}/{}", origin.owner, origin.repo);
    trf(
        "Source · {repo} @ {sha}",
        &[("repo", &repo), ("sha", short_sha(&origin.commit))],
    )
}

fn source_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn receipt_fixture() -> (Source, model::Local) {
        let source = Source::for_origin(
            "lantern",
            "Fixture/Lantern",
            42,
            "sunrise-mod.toml",
            sources::Tracker::Branch("main".into()),
        )
        .unwrap();
        let origin = sunrise_launcher::github_mods::Origin {
            id: "lantern".into(),
            version: "1.0.0".into(),
            owner: "fixture".into(),
            repo: "Lantern".into(),
            commit: "1".repeat(40),
            sha256: "0".repeat(64),
            tag: None,
            asset: None,
            added_from: None,
            source: Some(sunrise_launcher::github_mods::SourceOrigin {
                repo_id: 42,
                manifest: "sunrise-mod.toml".into(),
                track: sunrise_launcher::github_mods::SourceTrack::Branch("main".into()),
                published_at: None,
            }),
        };
        let local = model::Local {
            id: origin.id.clone(),
            version: origin.version.clone(),
            name: "Lantern".into(),
            file: "sunrise-native-mods/lantern.smod".into(),
            github: Some(origin),
            native: true,
            unreadable: false,
        };
        (source, local)
    }

    #[test]
    fn an_older_commit_of_the_same_listing_is_installed() {
        let (listing, local) = receipt_fixture();
        let rows = InstalledRows {
            local: vec![local],
            ..Default::default()
        };
        let origin = rows.local[0].github.as_ref().unwrap();
        // The installed commit is the receipt's commit, independent of where the branch is now.
        assert_eq!(origin.commit, "1".repeat(40));
        assert_eq!(
            rows.source_presence(&listing),
            SourcePresence::Installed {
                origin,
                file: &rows.local[0].file
            }
        );
    }

    #[test]
    fn installed_project_only_promotes_the_selected_archive_to_a_listed_source() {
        let (listing, local) = receipt_fixture();
        let file = local.file.clone();
        let mut rows = InstalledRows {
            local: vec![local.clone()],
            ..Default::default()
        };
        let mut catalogue = Catalogue {
            sources: Vec::new().into(),
            collections: Vec::new().into(),
            saved_at: None,
            checked_at: SystemTime::UNIX_EPOCH,
            error: Some("Offline".into()),
        };
        // Neither a pending reply nor a failed reply without a listing replaces local details.
        assert_eq!(rows.source_project(&local.id, &file, None), None);
        assert_eq!(
            rows.source_project(&local.id, &file, Some(&catalogue)),
            None
        );
        catalogue.sources = vec![listing.clone()].into();
        catalogue.error = None;
        assert_eq!(
            rows.source_project(&local.id, &file, Some(&catalogue)),
            Some(&listing)
        );
        assert_eq!(
            rows.source_project(&local.id, "different.smod", Some(&catalogue)),
            None
        );
        // A duplicate forces Installed back to the selected archive's own details, while
        // Discover still sees a conflict and cannot install over either package.
        rows.local.push(model::Local {
            file: "copy.smod".into(),
            github: None,
            ..local.clone()
        });
        assert_eq!(
            rows.source_project(&local.id, &file, Some(&catalogue)),
            None
        );
        assert_eq!(rows.source_presence(&listing), SourcePresence::Conflict);
    }

    #[test]
    fn an_occupied_id_without_the_listings_receipt_is_a_conflict() {
        let (listing, local) = receipt_fixture();
        let mut rows = InstalledRows {
            local: vec![local],
            ..Default::default()
        };
        let matches = rows.local[0].github.clone().unwrap();
        let mut other = matches.clone();
        other.source.as_mut().unwrap().repo_id += 1;
        rows.local[0].github = Some(other);
        assert_eq!(rows.source_presence(&listing), SourcePresence::Conflict);
        let mut other = matches.clone();
        other.owner = "DifferentOwner".into();
        rows.local[0].github = Some(other);
        assert_eq!(rows.source_presence(&listing), SourcePresence::Conflict);
        let mut other = matches.clone();
        other.source.as_mut().unwrap().manifest = "other/sunrise-mod.toml".into();
        rows.local[0].github = Some(other);
        assert_eq!(rows.source_presence(&listing), SourcePresence::Conflict);
        rows.local[0].github = Some(sunrise_launcher::github_mods::Origin {
            source: None,
            ..matches
        });
        assert_eq!(rows.source_presence(&listing), SourcePresence::Conflict);
        rows.local.clear();
        assert_eq!(rows.source_presence(&listing), SourcePresence::Missing);
    }

    #[test]
    fn source_kind_and_search_apply_to_the_combined_name_order() {
        let source = |id, repo| {
            Source::for_origin(
                id,
                repo,
                42,
                "sunrise-mod.toml",
                sources::Tracker::Branch("main".into()),
            )
            .unwrap()
        };
        let catalogue = Catalogue {
            sources: vec![
                source("zebra", "Zoo/zebra"),
                source("aardvark", "Zoo/aardvark"),
            ]
            .into(),
            collections: vec![sunrise_launcher_smod::index::Collection {
                id: "animals".into(),
                name: "Mammals".into(),
                summary: "Animals of the plains".into(),
                description: String::new(),
                icon: "icons/mammals.png".into(),
                owners: vec!["Zoo".into()],
                members: vec![sunrise_launcher_smod::index::Member {
                    id: "zebra".into(),
                    default: true,
                }],
                downloads: None,
            }]
            .into(),
            saved_at: None,
            checked_at: SystemTime::UNIX_EPOCH,
            error: None,
        };
        let collection = Listing::Collection(0);
        let zebra = Listing::Source {
            index: 0,
            part_of: Some(0),
        };
        let aardvark = Listing::Source {
            index: 1,
            part_of: None,
        };
        let mut browse = Browse::new(&catalogue);
        browse.set_query("", SourceKind::Any);
        assert_eq!(browse.matches, [aardvark, collection]);
        browse.set_query("", SourceKind::Mods);
        assert_eq!(browse.matches, [aardvark, zebra]);
        browse.set_query("", SourceKind::Collections);
        assert_eq!(browse.matches, [collection]);
        browse.set_query("ANIMALS plains", SourceKind::Collections);
        assert_eq!(browse.matches, [collection]);
        browse.set_query("zOo ZEBRA", SourceKind::Any);
        assert_eq!(browse.matches, [zebra]);
        browse.set_query("zOo ZEBRA", SourceKind::Collections);
        assert!(browse.matches.is_empty());
    }

    #[test]
    fn only_a_matching_full_build_or_valid_profile_prefix_gets_the_badge() {
        let pin = "9425651aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other = "9425651bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(made_for_build(Some(pin), Some(pin)));
        assert!(made_for_build(Some("9425651"), Some(pin)));
        assert!(!made_for_build(Some(other), Some(pin)));
        assert!(!made_for_build(Some("1234567"), Some(pin)));
        assert!(!made_for_build(Some(pin), None));
        assert!(!made_for_build(None, Some(pin)));
    }
}
