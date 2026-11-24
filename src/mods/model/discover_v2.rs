//! Discover and Mod Page view models built strictly from the signed v2 registry index.
//!
//! Pure data in and out: no files, no network, no GPUI. Compatibility comes solely from each
//! release's generated `fits` and `does_not_fit` lists for the selected profile's exact build,
//! never inferred from `sunrise` or semver. Images are kept only when their path is
//! content-addressed (`pages/<project id>/<sha256>.<png|jpg>`).
use super::{Make, SourceLink};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use sunrise_launcher::i18n::{tr, trf};
use sunrise_launcher::native_mods::local_fit;
use sunrise_launcher_smod::index::Kind;
use sunrise_launcher_smod::index_v2::{self, FitVerdict, Index, Project, Release};

/// How Discover orders matching mods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::mods) enum V2Sort {
    #[default]
    Relevance,
    Updated,
    Name,
}

impl V2Sort {
    pub(in crate::mods) const ALL: [V2Sort; 3] = [V2Sort::Relevance, V2Sort::Updated, V2Sort::Name];

    pub(in crate::mods) fn label(self) -> &'static str {
        match self {
            V2Sort::Relevance => tr("Relevance"),
            V2Sort::Updated => tr("Recently updated"),
            V2Sort::Name => tr("Name"),
        }
    }

    pub(in crate::mods) fn summary_label(self) -> &'static str {
        match self {
            V2Sort::Relevance => tr("Ranked by relevance"),
            V2Sort::Updated => tr("Sorted by recently updated"),
            V2Sort::Name => tr("Sorted by name"),
        }
    }
}

/// Active Discover filters. `show_all` defaults to `false`: only mods with a fitting release for
/// the selected profile's exact build are shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::mods) struct V2Filters {
    pub(in crate::mods) text: String,
    pub(in crate::mods) category: Option<String>,
    pub(in crate::mods) show_all: bool,
    pub(in crate::mods) sort: V2Sort,
}

/// Compatibility state of a project for the selected profile's exact build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) enum CardFit {
    /// At least one non-revoked release lists the exact build in `fits`.
    /// Carries the newest fitting release's version.
    Fits { version: String },
    /// No non-revoked release fits, and the newest non-revoked release lists the exact build in
    /// `does_not_fit`.
    DoesNotFit { build_label: String },
    /// No non-revoked release fits, and the exact build is in neither `fits` nor `does_not_fit`.
    NotCheckedYet { build_label: String },
    /// The index says nothing of the build, and this computer's check found that a release
    /// fits it. Carries the newest such release's version.
    FitsHere { version: String },
    /// The index says nothing of the build, and this computer's check found that the newest
    /// release does not fit it.
    DoesNotFitHere { build_label: String },
    /// The index says nothing of the build, and this computer is checking the newest release.
    Checking { build_label: String },
    /// The project or every one of its releases is taken down in `index.revoked`.
    Revoked(String),
    /// The selected profile has no game build yet.
    NoBuild,
}

impl CardFit {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, CardFit::Fits { .. } | CardFit::FitsHere { .. })
    }

    pub(in crate::mods) fn fitting_version(&self) -> Option<&str> {
        match self {
            CardFit::Fits { version, .. } | CardFit::FitsHere { version } => Some(version),
            _ => None,
        }
    }

    /// The fit pill a fitting card shows.
    pub(in crate::mods) fn fit_pill(&self) -> Option<String> {
        match self {
            CardFit::Fits { version } => {
                Some(trf("{version} · Fits build", &[("version", version)]))
            }
            CardFit::FitsHere { version } => Some(trf(
                "{version} · Fits build (checked on this computer)",
                &[("version", version)],
            )),
            _ => None,
        }
    }

    /// Whether a card that can't be installed is drawn faded: not while its fit is being
    /// checked.
    pub(in crate::mods) fn faded(&self) -> bool {
        !self.fits() && !matches!(self, CardFit::Checking { .. })
    }

    /// The reason a card is greyed out and cannot be installed on the selected profile.
    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            CardFit::Fits { .. } | CardFit::FitsHere { .. } => None,
            CardFit::DoesNotFit { build_label } => {
                Some(trf("Doesn't fit {build}", &[("build", build_label)]))
            }
            CardFit::NotCheckedYet { build_label } => Some(trf(
                "Not checked for {build} yet",
                &[("build", build_label)],
            )),
            CardFit::DoesNotFitHere { build_label } => Some(trf(
                "Doesn't fit {build} (checked on this computer)",
                &[("build", build_label)],
            )),
            CardFit::Checking { build_label } => {
                Some(trf("Checking fit for {build}…", &[("build", build_label)]))
            }
            CardFit::Revoked(why) => Some(trf("Taken down: {reason}", &[("reason", why)])),
            CardFit::NoBuild => Some(tr("No game build selected").to_owned()),
        }
    }
}

/// One mod card on Discover built from the v2 index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2Card {
    pub(in crate::mods) id: String,
    pub(in crate::mods) name: String,
    pub(in crate::mods) authors: String,
    pub(in crate::mods) summary: String,
    pub(in crate::mods) make: Make,
    pub(in crate::mods) categories: Vec<String>,
    /// Verified content-addressed relative path (`pages/<id>/<sha256>.<png|jpg>`), if valid.
    pub(in crate::mods) icon: Option<String>,
    /// Publication date of the newest fitting release (or newest non-revoked release when not
    /// fitting), `YYYY-MM-DD`.
    pub(in crate::mods) updated: String,
    pub(in crate::mods) fit: CardFit,
    name_lc: String,
    slug_lc: String,
    mod_id_lc: String,
    summary_lc: String,
    authors_lc: String,
    repo_lc: String,
    categories_lc: Vec<String>,
    description_lc: String,
}

/// One gallery screenshot on a v2 mod page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2Screenshot {
    /// Content-addressed relative path (`pages/<id>/<sha256>.<png|jpg>`).
    pub(in crate::mods) path: String,
    pub(in crate::mods) caption: String,
}

/// Compatibility verdict of one individual release for the selected profile's exact build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) enum ReleaseVerdict {
    Fits,
    DoesNotFit(String),
    NotCheckedYet(String),
    /// Fits by this computer's check; the index says nothing of the build.
    FitsHere,
    DoesNotFitHere(String),
    Checking(String),
    Revoked(String),
    NoBuild,
}

/// This computer's fit checks for one build: what was found, and what is being checked now.
#[derive(Clone, Debug, Default)]
pub(in crate::mods) struct LocalFits {
    found: local_fit::Found,
    /// `.smod` SHA-256s (lowercase) a running check will answer.
    checking: BTreeSet<String>,
}

impl LocalFits {
    /// What this computer found for `build`, listed in the index or not, with every unanswered
    /// gap marked as being checked when `running`.
    pub(in crate::mods) fn read(index: &Index, build: Option<&str>, running: bool) -> Self {
        let Some(build) = build else {
            return Self::default();
        };
        let found = local_fit::commit_of(index, build)
            .map(|commit| local_fit::kept(&commit))
            .unwrap_or_default();
        let checking = if running {
            local_fit::gaps(index, build)
                .into_iter()
                .map(|r| r.file.sha256.to_ascii_lowercase())
                .filter(|sha256| !found.contains_key(sha256))
                .collect()
        } else {
            BTreeSet::new()
        };
        Self { found, checking }
    }

    /// The verdict for a release the index says nothing of on `build`.
    fn verdict(&self, release: &Release, label: String) -> ReleaseVerdict {
        let sha256 = release.file.sha256.to_ascii_lowercase();
        match self.found.get(&sha256) {
            Some(local_fit::Local::Fits) => ReleaseVerdict::FitsHere,
            Some(local_fit::Local::DoesNotFit) => ReleaseVerdict::DoesNotFitHere(label),
            Some(local_fit::Local::Unknown) => ReleaseVerdict::NotCheckedYet(label),
            None if self.checking.contains(&sha256) => ReleaseVerdict::Checking(label),
            None => ReleaseVerdict::NotCheckedYet(label),
        }
    }
}

impl ReleaseVerdict {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, ReleaseVerdict::Fits | ReleaseVerdict::FitsHere)
    }

    /// What a fitting release's row says of its fit.
    pub(in crate::mods) fn fit_label(&self) -> &'static str {
        match self {
            ReleaseVerdict::FitsHere => tr("Fits this build (checked on this computer)"),
            _ => tr("Fits this build"),
        }
    }

    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            ReleaseVerdict::Fits | ReleaseVerdict::FitsHere => None,
            ReleaseVerdict::DoesNotFitHere(build) => Some(trf(
                "Doesn't fit {build} (checked on this computer)",
                &[("build", build)],
            )),
            ReleaseVerdict::Checking(build) => {
                Some(trf("Checking fit for {build}…", &[("build", build)]))
            }
            ReleaseVerdict::DoesNotFit(build) => {
                Some(trf("Doesn't fit {build}", &[("build", build)]))
            }
            ReleaseVerdict::NotCheckedYet(build) => {
                Some(trf("Not checked for {build} yet", &[("build", build)]))
            }
            ReleaseVerdict::Revoked(why) => Some(trf("Taken down: {reason}", &[("reason", why)])),
            ReleaseVerdict::NoBuild => Some(tr("No game build selected").to_owned()),
        }
    }
}

/// One release row on a v2 mod page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2VersionLine {
    pub(in crate::mods) version: String,
    pub(in crate::mods) published: String,
    pub(in crate::mods) size: u64,
    pub(in crate::mods) verdict: ReleaseVerdict,
    /// The build the release was made for.
    pub(in crate::mods) made_for: String,
}

/// What one release is known to do on one game build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mods) enum BuildFit {
    /// The index says it fits.
    Fits,
    /// The index says it doesn't.
    DoesNotFit,
    /// The index says nothing; this computer found that it fits.
    FitsHere,
    /// The index says nothing; this computer found that it doesn't.
    DoesNotFitHere,
    /// Nobody has checked it yet.
    NotChecked,
}

/// Every build one release is known to work on, or not, for the release's builds window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct ReleaseBuilds {
    pub(in crate::mods) name: String,
    pub(in crate::mods) version: String,
    pub(in crate::mods) made_for: String,
    /// Each build the index lists, in its order, then each other build the release names.
    pub(in crate::mods) builds: Vec<(String, BuildFit)>,
}

/// What release `version` of project `id` does on each build: the index's verdict, else the
/// answer this computer kept for the build's id (read by `kept`).
pub(in crate::mods) fn release_builds(
    index: &Index,
    id: &str,
    version: &str,
    kept: &dyn Fn(&str) -> local_fit::Found,
) -> Option<ReleaseBuilds> {
    let project = index.project(id)?;
    let release = project.releases.iter().find(|r| r.version == version)?;
    let sha256 = release.file.sha256.to_ascii_lowercase();
    let mut builds: Vec<(String, BuildFit)> = index
        .builds
        .iter()
        .map(|build| {
            let fit = match release.fit_verdict(&build.id) {
                FitVerdict::Fits => BuildFit::Fits,
                FitVerdict::DoesNotFit => BuildFit::DoesNotFit,
                FitVerdict::Unknown => match kept(&build.commit).get(&sha256) {
                    Some(local_fit::Local::Fits) => BuildFit::FitsHere,
                    Some(local_fit::Local::DoesNotFit) => BuildFit::DoesNotFitHere,
                    Some(local_fit::Local::Unknown) | None => BuildFit::NotChecked,
                },
            };
            (build.id.clone(), fit)
        })
        .collect();
    let named = |list: &[String], fit| {
        list.iter()
            .filter(|b| index.build(b).is_none())
            .map(move |b| (b.clone(), fit))
            .collect::<Vec<_>>()
    };
    builds.extend(named(&release.fits, BuildFit::Fits));
    builds.extend(named(&release.does_not_fit, BuildFit::DoesNotFit));
    Some(ReleaseBuilds {
        name: project.name.clone(),
        version: release.version.clone(),
        made_for: release.sunrise.clone(),
        builds,
    })
}

/// Full mod page rendered from a v2 index project.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::mods) struct V2Page {
    pub(in crate::mods) id: String,
    pub(in crate::mods) mod_id: String,
    pub(in crate::mods) name: String,
    pub(in crate::mods) authors: String,
    pub(in crate::mods) summary: String,
    pub(in crate::mods) description: String,
    pub(in crate::mods) make: Make,
    pub(in crate::mods) categories: Vec<String>,
    /// Verified content-addressed icon path, if valid.
    pub(in crate::mods) icon: Option<String>,
    /// Verified content-addressed screenshots with captions; never fetched until the page opens.
    pub(in crate::mods) screenshots: Vec<V2Screenshot>,
    /// `owner/repo` on GitHub.
    pub(in crate::mods) repository_name: String,
    /// Validated `https://github.com/<owner>/<repo>` link.
    pub(in crate::mods) repository_url: Option<SourceLink>,
    pub(in crate::mods) source: Option<SourceLink>,
    pub(in crate::mods) homepage: Option<SourceLink>,
    pub(in crate::mods) issues: Option<SourceLink>,
    pub(in crate::mods) license: String,
    pub(in crate::mods) updated: String,
    pub(in crate::mods) fit: CardFit,
    /// The newest fitting release that Install will pick, if any.
    pub(in crate::mods) will_install: Option<V2VersionLine>,
    /// All non-revoked releases whose `fits` includes the selected build, newest first.
    pub(in crate::mods) fitting_versions: Vec<V2VersionLine>,
    /// Remaining releases (not fitting this build or revoked), newest first.
    pub(in crate::mods) other_versions: Vec<V2VersionLine>,
    /// What the release Install picks (else the newest) depends on, required ones first.
    pub(in crate::mods) dependencies: Vec<V2Dependency>,
}

/// One dependency on a v2 mod page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2Dependency {
    pub(in crate::mods) mod_id: String,
    /// The semver range the provider's release must satisfy; `None` when any version will do.
    pub(in crate::mods) range: Option<String>,
    pub(in crate::mods) required: bool,
    pub(in crate::mods) provider: V2Provider,
}

/// Which registry project provides a dependency, found the way an install finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) enum V2Provider {
    /// The pinned project, or the only listed project with that in-game ID.
    Listed {
        id: String,
        name: String,
        icon: Option<String>,
    },
    /// Several listed projects provide the ID: the install asks which one.
    Several(usize),
    /// No listed project provides it.
    Missing,
}

fn provider(index: &Index, dep: &index_v2::Dependency) -> V2Provider {
    let listed = |p: &&Project| make_of(&p.kind).is_some() && index.revoked(&p.id, None).is_none();
    let found: Vec<&Project> = match dep.project.as_deref() {
        Some(pin) => index
            .project(pin)
            .filter(|p| p.mod_id == dep.mod_id)
            .filter(listed)
            .into_iter()
            .collect(),
        None => index.projects_for_mod(&dep.mod_id).filter(listed).collect(),
    };
    match found.as_slice() {
        [] => V2Provider::Missing,
        [p] => V2Provider::Listed {
            id: p.id.clone(),
            name: p.name.clone(),
            icon: content_addressed_path(&p.id, &p.icon),
        },
        several => V2Provider::Several(several.len()),
    }
}

fn make_of(kind: &Kind) -> Option<Make> {
    match kind {
        Kind::Mod => Some(Make::Content),
        Kind::NativeMod => Some(Make::Native),
        _ => None,
    }
}

fn compare_releases_newest_first(a: &Release, b: &Release) -> Ordering {
    let va = semver::Version::parse(&a.version).ok();
    let vb = semver::Version::parse(&b.version).ok();
    vb.cmp(&va)
        .then_with(|| b.published.cmp(&a.published))
        .then_with(|| b.version.cmp(&a.version))
}

fn content_addressed_path(project_id: &str, relative_path: &str) -> Option<String> {
    index_v2::content_addressed_image_sha256(project_id, relative_path)
        .ok()
        .map(|_| relative_path.to_owned())
}

fn join_authors(project: &Project) -> String {
    if project.authors.is_empty() {
        project
            .repository
            .name
            .split('/')
            .next()
            .unwrap_or_default()
            .to_owned()
    } else {
        project.authors.join(", ")
    }
}

fn release_verdict(
    index: &Index,
    project: &Project,
    release: &Release,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> ReleaseVerdict {
    if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
        return ReleaseVerdict::Revoked(revoked.reason.clone());
    }
    let Some(build) = exact_build else {
        return ReleaseVerdict::NoBuild;
    };
    match release.fit_verdict(build) {
        FitVerdict::Fits => ReleaseVerdict::Fits,
        FitVerdict::DoesNotFit => ReleaseVerdict::DoesNotFit(build_label(build)),
        FitVerdict::Unknown => local.verdict(release, build_label(build)),
    }
}

fn project_fit(
    index: &Index,
    project: &Project,
    sorted_releases: &[&Release],
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> (CardFit, String) {
    if let Some(revoked) = index.revoked(&project.id, None) {
        let updated = sorted_releases
            .first()
            .map(|r| r.published.clone())
            .unwrap_or_default();
        return (CardFit::Revoked(revoked.reason.clone()), updated);
    }

    let mut active: Vec<&Release> = Vec::new();
    let mut first_revoked: Option<&str> = None;
    for release in sorted_releases {
        if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
            first_revoked.get_or_insert(revoked.reason.as_str());
        } else {
            active.push(*release);
        }
    }

    if active.is_empty() {
        let reason = first_revoked.unwrap_or("taken down").to_owned();
        let updated = sorted_releases
            .first()
            .map(|r| r.published.clone())
            .unwrap_or_default();
        return (CardFit::Revoked(reason), updated);
    }

    let newest_active = active[0];
    let Some(build) = exact_build else {
        return (CardFit::NoBuild, newest_active.published.clone());
    };

    if let Some(fitting) = active
        .iter()
        .copied()
        .find(|r| r.fit_verdict(build) == FitVerdict::Fits)
    {
        return (
            CardFit::Fits {
                version: fitting.version.clone(),
            },
            fitting.published.clone(),
        );
    }

    if let Some(fitting) = active
        .iter()
        .copied()
        .find(|r| release_verdict(index, project, r, exact_build, build_label, local).fits())
    {
        return (
            CardFit::FitsHere {
                version: fitting.version.clone(),
            },
            fitting.published.clone(),
        );
    }

    let fit = match release_verdict(
        index,
        project,
        newest_active,
        exact_build,
        build_label,
        local,
    ) {
        ReleaseVerdict::DoesNotFit(build_label) => CardFit::DoesNotFit { build_label },
        ReleaseVerdict::DoesNotFitHere(build_label) => CardFit::DoesNotFitHere { build_label },
        ReleaseVerdict::Checking(build_label) => CardFit::Checking { build_label },
        _ => CardFit::NotCheckedYet {
            build_label: build_label(build),
        },
    };
    (fit, newest_active.published.clone())
}

/// Builds every mod card from a verified v2 index for `exact_build`.
pub(in crate::mods) fn cards(
    index: &Index,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> Vec<V2Card> {
    index
        .projects
        .iter()
        .filter_map(|p| {
            let make = make_of(&p.kind)?;
            let mut sorted: Vec<&Release> = p.releases.iter().collect();
            sorted.sort_by(|a, b| compare_releases_newest_first(a, b));
            let (fit, updated) = project_fit(index, p, &sorted, exact_build, build_label, local);
            let authors = join_authors(p);
            Some(V2Card {
                id: p.id.clone(),
                name_lc: p.name.to_lowercase(),
                slug_lc: p.slug.to_lowercase(),
                mod_id_lc: p.mod_id.to_lowercase(),
                summary_lc: p.summary.to_lowercase(),
                authors_lc: authors.to_lowercase(),
                repo_lc: p.repository.name.to_lowercase(),
                categories_lc: p.categories.iter().map(|c| c.to_lowercase()).collect(),
                description_lc: p.description.to_lowercase(),
                name: p.name.clone(),
                authors,
                summary: p.summary.clone(),
                make,
                categories: p.categories.clone(),
                icon: content_addressed_path(&p.id, &p.icon),
                updated,
                fit,
            })
        })
        .collect()
}

/// Categories used by at least one Discover mod card, most used first.
pub(in crate::mods) fn categories(cards: &[V2Card]) -> Vec<String> {
    let mut uses: HashMap<&str, usize> = HashMap::new();
    for card in cards {
        for category in &card.categories {
            *uses.entry(category.as_str()).or_default() += 1;
        }
    }
    let mut all: Vec<(&str, usize)> = uses.into_iter().collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    all.into_iter().map(|(k, _)| k.to_owned()).collect()
}

fn query_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

fn card_matches_query(card: &V2Card, words: &[String]) -> bool {
    words.iter().all(|w| {
        card.name_lc.contains(w)
            || card.slug_lc.contains(w)
            || card.mod_id_lc.contains(w)
            || card.summary_lc.contains(w)
            || card.authors_lc.contains(w)
            || card.repo_lc.contains(w)
            || card.categories_lc.iter().any(|c| c.contains(w))
            || card.description_lc.contains(w)
    })
}

fn relevance_score(card: &V2Card, full_query: &str, words: &[String]) -> u32 {
    if words.is_empty() {
        return 0;
    }
    let mut score: u32 = 0;
    if card.name_lc == full_query || card.mod_id_lc == full_query || card.slug_lc == full_query {
        score += 1000;
    } else if card.name_lc.starts_with(full_query) {
        score += 500;
    } else if card.name_lc.contains(full_query) {
        score += 250;
    }
    for word in words {
        if card
            .name_lc
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| token == word)
        {
            score += 120;
        } else if card.name_lc.contains(word) {
            score += 80;
        }
        if card.mod_id_lc.contains(word) || card.slug_lc.contains(word) {
            score += 60;
        }
        if card.summary_lc.contains(word) {
            score += 30;
        }
        if card.authors_lc.contains(word) || card.repo_lc.contains(word) {
            score += 20;
        }
        if card.categories_lc.iter().any(|c| c.contains(word)) {
            score += 15;
        }
        if card.description_lc.contains(word) {
            score += 5;
        }
    }
    score
}

/// Whether the index lists `exact_build` among its offered builds: Discover notes once that an
/// unlisted build's mods are checked on this computer. A profile without a build is handled by
/// its own empty state.
pub(in crate::mods) fn build_listed(index: &Index, exact_build: Option<&str>) -> bool {
    exact_build.is_none_or(|build| index.build(build).is_some())
}

/// Indices into `cards` matching `filters`, ordered by `filters.sort`.
pub(in crate::mods) fn discover(cards: &[V2Card], filters: &V2Filters) -> Vec<usize> {
    let words = query_words(&filters.text);
    let full_query = filters.text.trim().to_lowercase();
    let mut scored: Vec<(usize, u32)> = cards
        .iter()
        .enumerate()
        .filter(|(_, c)| filters.show_all || c.fit.fits())
        .filter(|(_, c)| {
            filters
                .category
                .as_ref()
                .is_none_or(|k| c.categories.contains(k))
        })
        .filter(|(_, c)| card_matches_query(c, &words))
        .map(|(i, c)| (i, relevance_score(c, &full_query, &words)))
        .collect();

    scored.sort_by(|&(ai, ascore), &(bi, bscore)| {
        let (a, b) = (&cards[ai], &cards[bi]);
        match filters.sort {
            V2Sort::Relevance => bscore
                .cmp(&ascore)
                .then_with(|| {
                    if !words.is_empty() {
                        b.fit.fits().cmp(&a.fit.fits())
                    } else {
                        Ordering::Equal
                    }
                })
                .then_with(|| b.updated.cmp(&a.updated))
                .then_with(|| a.name_lc.cmp(&b.name_lc))
                .then_with(|| a.id.cmp(&b.id)),
            V2Sort::Updated => b
                .updated
                .cmp(&a.updated)
                .then_with(|| a.name_lc.cmp(&b.name_lc))
                .then_with(|| a.id.cmp(&b.id)),
            V2Sort::Name => a.name_lc.cmp(&b.name_lc).then_with(|| a.id.cmp(&b.id)),
        }
    });
    scored.into_iter().map(|(i, _)| i).collect()
}

/// How many cards match the active text and category filters but do not fit the selected build.
pub(in crate::mods) fn hidden_other_build_count(cards: &[V2Card], filters: &V2Filters) -> usize {
    let words = query_words(&filters.text);
    cards
        .iter()
        .filter(|c| !c.fit.fits())
        .filter(|c| {
            filters
                .category
                .as_ref()
                .is_none_or(|k| c.categories.contains(k))
        })
        .filter(|c| card_matches_query(c, &words))
        .count()
}

/// A publisher's collection, its members resolved to their Discover cards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2Collection {
    pub(in crate::mods) id: String,
    pub(in crate::mods) name: String,
    pub(in crate::mods) summary: String,
    pub(in crate::mods) description: String,
    pub(in crate::mods) publisher: String,
    /// Verified content-addressed relative path (`pages/<id>/<sha256>.<png|jpg>`), if valid.
    pub(in crate::mods) icon: Option<String>,
    /// Each member's position in the cards, and whether it is picked when the collection opens.
    pub(in crate::mods) members: Vec<(usize, bool)>,
}

impl V2Collection {
    /// How many members have a release for the selected build.
    pub(in crate::mods) fn fitting(&self, cards: &[V2Card]) -> usize {
        self.members
            .iter()
            .filter(|&&(i, _)| cards[i].fit.fits())
            .count()
    }

    /// What opening the collection picks: the members on by default that fit the build.
    pub(in crate::mods) fn initial_picks(&self, cards: &[V2Card]) -> BTreeSet<String> {
        self.members
            .iter()
            .filter(|&&(i, default)| default && cards[i].fit.fits())
            .map(|&(i, _)| cards[i].id.clone())
            .collect()
    }

    /// What Install installs: the picked members that fit and aren't installed, in the
    /// collection's order. A member that doesn't fit the build is left out.
    pub(in crate::mods) fn to_install(
        &self,
        cards: &[V2Card],
        picked: &BTreeSet<String>,
        installed: &dyn Fn(&str) -> bool,
    ) -> Vec<String> {
        self.members
            .iter()
            .map(|&(i, _)| &cards[i])
            .filter(|card| card.fit.fits() && picked.contains(&card.id) && !installed(&card.id))
            .map(|card| card.id.clone())
            .collect()
    }
}

/// The index's collections, each member found among `cards`.
pub(in crate::mods) fn collections(index: &Index, cards: &[V2Card]) -> Vec<V2Collection> {
    let at: HashMap<&str, usize> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| (card.id.as_str(), i))
        .collect();
    index
        .collections
        .iter()
        .map(|c| V2Collection {
            id: c.id.clone(),
            name: c.name.clone(),
            summary: c.summary.clone(),
            description: c.description.clone(),
            publisher: c.publisher.clone(),
            icon: content_addressed_path(&c.id, &c.icon),
            members: c
                .members
                .iter()
                .filter_map(|m| Some((*at.get(m.project.as_str())?, m.default)))
                .collect(),
        })
        .collect()
}

/// Browsing shows a collection's members only inside its card, Show all included; a search
/// still finds a member by name.
pub(in crate::mods) fn fold_members(
    shown: &mut Vec<usize>,
    collections: &[V2Collection],
    filters: &V2Filters,
) {
    if !filters.text.trim().is_empty() {
        return;
    }
    let folded: HashSet<usize> = collections
        .iter()
        .flat_map(|c| c.members.iter().map(|&(i, _)| i))
        .collect();
    shown.retain(|i| !folded.contains(i));
}

/// The collections Discover lists for `filters`: unless Show all is on, only those with a member
/// that fits, and under a category only those with such a member in it, since browsing folds
/// members into their collection there too. Search matches the collection's own text or any
/// member's.
pub(in crate::mods) fn discover_collections(
    collections: &[V2Collection],
    cards: &[V2Card],
    filters: &V2Filters,
) -> Vec<usize> {
    let words = query_words(&filters.text);
    collections
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.members.iter().any(|&(i, _)| {
                let card = &cards[i];
                (filters.show_all || card.fit.fits())
                    && filters
                        .category
                        .as_ref()
                        .is_none_or(|k| card.categories.contains(k))
            })
        })
        .filter(|(_, c)| {
            let own = [&c.name, &c.summary, &c.publisher].map(|s| s.to_lowercase());
            words.iter().all(|w| own.iter().any(|s| s.contains(w)))
                || (!words.is_empty()
                    && c.members
                        .iter()
                        .any(|&(i, _)| card_matches_query(&cards[i], &words)))
        })
        .map(|(i, _)| i)
        .collect()
}

/// Builds a v2 mod page from `index` for project `id` and `exact_build`.
pub(in crate::mods) fn page(
    index: &Index,
    id: &str,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> Option<V2Page> {
    let p = index.project(id)?;
    let make = make_of(&p.kind)?;
    let mut sorted: Vec<&Release> = p.releases.iter().collect();
    sorted.sort_by(|a, b| compare_releases_newest_first(a, b));

    let (fit, updated) = project_fit(index, p, &sorted, exact_build, build_label, local);
    let mut fitting_versions = Vec::new();
    let mut other_versions = Vec::new();
    let mut picked: Option<&Release> = None;
    for release in &sorted {
        let verdict = release_verdict(index, p, release, exact_build, build_label, local);
        let line = V2VersionLine {
            version: release.version.clone(),
            published: release.published.clone(),
            size: release.file.size,
            verdict,
            made_for: release.sunrise.clone(),
        };
        if line.verdict.fits() {
            picked.get_or_insert(release);
            fitting_versions.push(line);
        } else {
            other_versions.push(line);
        }
    }
    let will_install = fitting_versions.first().cloned();
    let screenshots = p
        .screenshots
        .iter()
        .filter_map(|shot| {
            let path = content_addressed_path(&p.id, &shot.path)?;
            Some(V2Screenshot {
                path,
                caption: shot.caption.clone(),
            })
        })
        .collect();
    let repository_url = SourceLink::read(&format!("https://github.com/{}", p.repository.name));
    let mut dependencies: Vec<V2Dependency> = picked
        .or(sorted.first().copied())
        .map_or(&[][..], |release| &release.dependencies)
        .iter()
        .map(|dep| V2Dependency {
            mod_id: dep.mod_id.clone(),
            range: (dep.version != semver::VersionReq::STAR).then(|| dep.version.to_string()),
            required: dep.required,
            provider: provider(index, dep),
        })
        .collect();
    dependencies.sort_by_key(|dep| !dep.required);

    Some(V2Page {
        id: p.id.clone(),
        mod_id: p.mod_id.clone(),
        name: p.name.clone(),
        authors: join_authors(p),
        summary: p.summary.clone(),
        description: p.description.clone(),
        make,
        categories: p.categories.clone(),
        icon: content_addressed_path(&p.id, &p.icon),
        screenshots,
        repository_name: p.repository.name.clone(),
        repository_url,
        source: p.source.as_deref().and_then(SourceLink::read),
        homepage: p.homepage.as_deref().and_then(SourceLink::read),
        issues: p.issues.as_deref().and_then(SourceLink::read),
        license: p.license.clone(),
        updated,
        fit,
        will_install,
        fitting_versions,
        other_versions,
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_launcher_smod::index_v2::{
        Build, Category, Collection, File, Member, Repository, Revoked, Screenshot,
    };

    fn hex64(ch: char) -> String {
        std::iter::repeat_n(ch, 64).collect()
    }

    fn label_build(id: &str) -> String {
        match id {
            "26.3-0.1.0" => "Stable · 0.1.0".to_owned(),
            "nightly-2026-11-20-abcdef0" => "Nightly · Nov 20".to_owned(),
            "dev-2026-11-22-7cfdf81" => "Dev · Nov 22".to_owned(),
            other => other.to_owned(),
        }
    }

    fn make_release(
        version: &str,
        published: &str,
        sunrise: &str,
        fits: &[&str],
        does_not_fit: &[&str],
        sha_ch: char,
    ) -> Release {
        Release {
            version: version.to_owned(),
            published: published.to_owned(),
            sunrise: sunrise.to_owned(),
            workflow: format!(
                "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v{version}"
            ),
            repository: None,
            fits: fits.iter().map(|s| (*s).to_owned()).collect(),
            does_not_fit: does_not_fit.iter().map(|s| (*s).to_owned()).collect(),
            file: File {
                url: format!("https://github.com/example/mod/releases/download/v{version}/m.smod"),
                size: 120_000,
                sha256: hex64(sha_ch),
                attestation: format!(
                    "https://github.com/example/mod/releases/download/v{version}/m.smod.sigstore.json"
                ),
            },
            dependencies: Vec::new(),
        }
    }

    fn make_project(
        id: &str,
        slug: &str,
        name: &str,
        summary: &str,
        categories: &[&str],
        releases: Vec<Release>,
    ) -> Project {
        Project {
            id: id.to_owned(),
            slug: slug.to_owned(),
            mod_id: slug.replace('-', "_"),
            kind: Kind::NativeMod,
            name: name.to_owned(),
            summary: summary.to_owned(),
            description: format!("# {name}\n\nDetailed description of {name}."),
            authors: vec!["Author".to_owned()],
            owners: vec!["author".to_owned()],
            license: "MIT".to_owned(),
            homepage: Some("https://example.com/mod".to_owned()),
            issues: Some("https://github.com/author/mod/issues".to_owned()),
            source: Some("https://github.com/author/mod".to_owned()),
            icon: format!("pages/{id}/{}.png", hex64('a')),
            screenshots: vec![
                Screenshot {
                    path: format!("pages/{id}/{}.png", hex64('b')),
                    caption: "Overworld minimap and waypoints".to_owned(),
                },
                Screenshot {
                    path: format!("pages/{id}/not-hashed.png"),
                    caption: "Ignored non-content-addressed shot".to_owned(),
                },
            ],
            categories: categories.iter().map(|s| (*s).to_owned()).collect(),
            repository: Repository {
                id: 42,
                name: format!("author/{slug}"),
                path: ".".to_owned(),
            },
            workflow: "sunrise-org/sunrise-mods/.github/workflows/release.yml".to_owned(),
            releases,
        }
    }

    fn sample_index(projects: Vec<Project>, revoked: Vec<Revoked>) -> Index {
        Index {
            schema: index_v2::SCHEMA,
            serial: 1,
            generated: "2026-11-22T12:00:00Z".to_owned(),
            builds: vec![
                Build {
                    id: "26.3-0.1.0".to_owned(),
                    commit: "a".repeat(40),
                },
                Build {
                    id: "nightly-2026-11-20-abcdef0".to_owned(),
                    commit: format!("abcdef0{}", "0".repeat(33)),
                },
            ],
            categories: vec![
                Category {
                    id: "utility".to_owned(),
                    icon: "wrench".to_owned(),
                    kinds: vec![Kind::Mod, Kind::NativeMod],
                },
                Category {
                    id: "worldgen".to_owned(),
                    icon: "compass".to_owned(),
                    kinds: vec![Kind::Mod, Kind::NativeMod],
                },
            ],
            projects,
            revoked,
            collections: Vec::new(),
        }
    }

    #[test]
    fn exact_fit_never_infers_from_sunrise_or_semver_and_picks_newest_fitting() {
        let build = "26.3-0.1.0";
        // Project A: 2.0.0 does NOT list build in fits even though sunrise == build;
        // 1.2.0 and 1.0.0 list build in fits -> newest fitting must be 1.2.0.
        let p_a = make_project(
            "Ab3dE9xQ",
            "wayfinder",
            "Wayfinder",
            "Minimap with waypoints",
            &["utility"],
            vec![
                make_release("1.0.0", "2026-11-10", build, &[build], &[], '1'),
                make_release("2.0.0", "2026-11-22", build, &[], &[build], '2'),
                make_release("1.2.0", "2026-11-20", build, &[build], &[], '3'),
            ],
        );
        // Project B: sunrise == build, but fits is empty and does_not_fit is empty -> Unknown, NOT Fits!
        let p_b = make_project(
            "Cd5fG7hJ",
            "unchecked",
            "Unchecked Mod",
            "Waiting for fit check",
            &["utility"],
            vec![make_release("1.0.0", "2026-11-21", build, &[], &[], '4')],
        );
        // Project C: explicitly listed in does_not_fit -> DoesNotFit.
        let p_c = make_project(
            "Ef7gH9iK",
            "incompatible",
            "Incompatible Mod",
            "Needs newer API",
            &["worldgen"],
            vec![make_release(
                "1.0.0",
                "2026-11-21",
                "nightly-2026-11-20-abcdef0",
                &["nightly-2026-11-20-abcdef0"],
                &[build],
                '5',
            )],
        );

        let index = sample_index(vec![p_a, p_b, p_c], Vec::new());
        let list = cards(&index, Some(build), &label_build, &LocalFits::default());
        assert_eq!(list.len(), 3);

        // Wayfinder fits at newest fitting release 1.2.0 (skipping non-fitting 2.0.0).
        assert_eq!(list[0].fit.fitting_version(), Some("1.2.0"));
        assert!(list[0].fit.fits());
        assert_eq!(list[0].fit.reason(), None);

        // Unchecked Mod is NOT inferred from sunrise; shows "Not checked for ... yet".
        assert!(!list[1].fit.fits());
        assert_eq!(
            list[1].fit.reason().as_deref(),
            Some("Not checked for Stable · 0.1.0 yet")
        );

        // Incompatible Mod is in does_not_fit; shows "Doesn't fit ...".
        assert!(!list[2].fit.fits());
        assert_eq!(
            list[2].fit.reason().as_deref(),
            Some("Doesn't fit Stable · 0.1.0")
        );

        // Default Discover (show_all = false) shows ONLY fitting mods.
        let default_filters = V2Filters::default();
        let shown = discover(&list, &default_filters);
        assert_eq!(shown, vec![0]);
        assert_eq!(hidden_other_build_count(&list, &default_filters), 2);

        // Show all includes the non-fitting mods with their exact reasons.
        let all_filters = V2Filters {
            show_all: true,
            ..V2Filters::default()
        };
        let shown_all = discover(&list, &all_filters);
        assert_eq!(shown_all.len(), 3);
    }

    #[test]
    fn a_release_lists_every_build_with_the_index_verdict_first_and_local_answers_in_gaps() {
        let night = "nightly-2026-11-20-abcdef0";
        let stable = "26.3-0.1.0";
        let gone = "dev-2026-11-01-1111111";
        let project = make_project(
            "Ab3dE9xQ",
            "enchanting",
            "Dis-Enchanting Table",
            "Takes enchantments off",
            &["utility"],
            vec![
                make_release("5.1.1", "2026-11-24", night, &[night, gone], &[], '1'),
                make_release("5.1.0", "2026-11-22", night, &[], &[], '2'),
            ],
        );
        let mut index = sample_index(vec![project], Vec::new());
        let stable_commit = index.build(stable).unwrap().commit.clone();
        let night_commit = index.build(night).unwrap().commit.clone();
        // This computer found both releases not to fit either build.
        let kept = |commit: &str| {
            if commit != stable_commit && commit != night_commit {
                return local_fit::Found::new();
            }
            local_fit::Found::from([
                (hex64('1'), local_fit::Local::DoesNotFit),
                (hex64('2'), local_fit::Local::DoesNotFit),
            ])
        };

        let newest = release_builds(&index, "Ab3dE9xQ", "5.1.1", &kept).unwrap();
        assert_eq!(newest.made_for, night);
        assert_eq!(
            newest.builds,
            [
                (stable.to_owned(), BuildFit::DoesNotFitHere),
                (night.to_owned(), BuildFit::Fits),
                (gone.to_owned(), BuildFit::Fits),
            ],
            "the index's own verdict wins over this computer's"
        );
        let older = release_builds(&index, "Ab3dE9xQ", "5.1.0", &|_| local_fit::Found::new());
        assert_eq!(
            older.unwrap().builds,
            [
                (stable.to_owned(), BuildFit::NotChecked),
                (night.to_owned(), BuildFit::NotChecked),
            ]
        );

        index.projects[0].releases[1]
            .does_not_fit
            .push(stable.into());
        let judged = release_builds(&index, "Ab3dE9xQ", "5.1.0", &kept).unwrap();
        assert_eq!(judged.builds[0], (stable.to_owned(), BuildFit::DoesNotFit));
        assert!(release_builds(&index, "Ab3dE9xQ", "9.9.9", &kept).is_none());
    }

    #[test]
    fn relevance_ranks_exact_name_ahead_of_summary_and_description() {
        let build = "26.3-0.1.0";
        let summary_hit = make_project(
            "Aa11Bb22",
            "compass-plus",
            "Compass Plus",
            "Adds a wayfinder overlay to the compass",
            &["utility"],
            vec![make_release(
                "1.0.0",
                "2026-11-22",
                build,
                &[build],
                &[],
                '1',
            )],
        );
        let name_hit = make_project(
            "Cc33Dd44",
            "wayfinder",
            "Wayfinder",
            "Minimap with waypoints",
            &["utility"],
            vec![make_release(
                "1.0.0",
                "2026-11-18",
                build,
                &[build],
                &[],
                '2',
            )],
        );
        let index = sample_index(vec![summary_hit, name_hit], Vec::new());
        let list = cards(&index, Some(build), &label_build, &LocalFits::default());
        let shown = discover(
            &list,
            &V2Filters {
                text: "wayfinder".to_owned(),
                ..V2Filters::default()
            },
        );
        assert_eq!(shown.len(), 2);
        assert_eq!(list[shown[0]].name, "Wayfinder");
        assert_eq!(list[shown[1]].name, "Compass Plus");
    }

    #[test]
    fn takedowns_block_whole_project_and_single_release_digests() {
        let build = "26.3-0.1.0";
        let whole_revoked = make_project(
            "Rr11Rr22",
            "bad-mod",
            "Bad Mod",
            "Taken down",
            &["utility"],
            vec![make_release(
                "1.0.0",
                "2026-11-20",
                build,
                &[build],
                &[],
                '9',
            )],
        );
        let one_release_revoked = make_project(
            "Ss33Ss44",
            "good-mod",
            "Good Mod",
            "Has a clean older release",
            &["utility"],
            vec![
                make_release("2.0.0", "2026-11-22", build, &[build], &[], '7'),
                make_release("1.0.0", "2026-11-18", build, &[build], &[], '8'),
            ],
        );
        let index = sample_index(
            vec![whole_revoked, one_release_revoked],
            vec![
                Revoked {
                    project: "Rr11Rr22".to_owned(),
                    sha256: None,
                    reason: "Malware".to_owned(),
                    date: "2026-11-22".to_owned(),
                },
                Revoked {
                    project: "Ss33Ss44".to_owned(),
                    sha256: Some(hex64('7')),
                    reason: "Broken build".to_owned(),
                    date: "2026-11-22".to_owned(),
                },
            ],
        );
        let list = cards(&index, Some(build), &label_build, &LocalFits::default());
        assert!(!list[0].fit.fits());
        assert_eq!(list[0].fit.reason().as_deref(), Some("Taken down: Malware"));
        // 2.0.0 is revoked by sha256, so Good Mod falls back to fitting 1.0.0.
        assert_eq!(list[1].fit.fitting_version(), Some("1.0.0"));
    }

    #[test]
    fn v2_page_keeps_captioned_content_addressed_screenshots_and_publisher() {
        let build = "26.3-0.1.0";
        let proj = make_project(
            "Ab3dE9xQ",
            "wayfinder",
            "Wayfinder",
            "Clean minimap with waypoints",
            &["utility", "worldgen"],
            vec![
                make_release("1.2.0", "2026-11-22", build, &[build], &[], '1'),
                make_release(
                    "1.1.0",
                    "2026-11-18",
                    "nightly-2026-11-20-abcdef0",
                    &["nightly-2026-11-20-abcdef0"],
                    &[build],
                    '2',
                ),
            ],
        );
        let index = sample_index(vec![proj], Vec::new());
        let p = page(
            &index,
            "Ab3dE9xQ",
            Some(build),
            &label_build,
            &LocalFits::default(),
        )
        .unwrap();
        assert_eq!(p.repository_name, "author/wayfinder");
        assert_eq!(p.categories, vec!["utility", "worldgen"]);
        assert_eq!(p.summary, "Clean minimap with waypoints");
        assert!(p.icon.is_some());
        // Non-content-addressed screenshot is dropped; content-addressed one keeps its caption.
        assert_eq!(p.screenshots.len(), 1);
        assert_eq!(p.screenshots[0].caption, "Overworld minimap and waypoints");
        assert_eq!(p.fitting_versions.len(), 1);
        assert_eq!(p.fitting_versions[0].version, "1.2.0");
        assert_eq!(p.other_versions.len(), 1);
        assert_eq!(
            p.other_versions[0].verdict.reason().as_deref(),
            Some("Doesn't fit Stable · 0.1.0")
        );
    }

    #[test]
    fn v2_page_lists_the_installed_release_dependencies_with_their_providers() {
        let build = "26.3-0.1.0";
        let dep = |mod_id: &str, version: &str, project: Option<&str>, required: bool| {
            index_v2::Dependency {
                mod_id: mod_id.to_owned(),
                version: semver::VersionReq::parse(version).unwrap(),
                project: project.map(str::to_owned),
                required,
            }
        };
        let mut fitting = make_release("1.0.0", "2026-11-10", build, &[build], &[], '1');
        fitting.dependencies = vec![
            dep("map_icons", "*", None, false),
            dep("core_lib", "^2.1", Some("CoreLib1"), true),
            dep("render_api", "^1", None, true),
            dep("ghost", "^1", None, true),
            dep("other_lib", "^1", Some("CoreLib1"), true),
        ];
        let mut newer = make_release("2.0.0", "2026-11-22", build, &[], &[build], '2');
        newer.dependencies = vec![dep("only_in_newer", "^1", None, true)];
        let index = sample_index(
            vec![
                make_project(
                    "Ab3dE9xQ",
                    "wayfinder",
                    "Wayfinder",
                    "",
                    &[],
                    vec![fitting, newer],
                ),
                make_project("CoreLib1", "core-lib", "Core Lib", "", &[], Vec::new()),
                make_project("Render01", "render-api", "Render A", "", &[], Vec::new()),
                make_project("Render02", "render-api", "Render B", "", &[], Vec::new()),
                make_project("MapIcon1", "map-icons", "Map Icons", "", &[], Vec::new()),
            ],
            Vec::new(),
        );
        let p = page(
            &index,
            "Ab3dE9xQ",
            Some(build),
            &label_build,
            &LocalFits::default(),
        )
        .unwrap();
        let rows: Vec<_> = p
            .dependencies
            .iter()
            .map(|d| {
                (
                    d.mod_id.as_str(),
                    d.range.as_deref(),
                    d.required,
                    &d.provider,
                )
            })
            .collect();
        let listed = |id: &str, name: &str| V2Provider::Listed {
            id: id.to_owned(),
            name: name.to_owned(),
            icon: Some(format!("pages/{id}/{}.png", hex64('a'))),
        };
        assert_eq!(
            rows,
            vec![
                (
                    "core_lib",
                    Some("^2.1"),
                    true,
                    &listed("CoreLib1", "Core Lib")
                ),
                ("render_api", Some("^1"), true, &V2Provider::Several(2)),
                ("ghost", Some("^1"), true, &V2Provider::Missing),
                // Pinned to a project that provides a different mod: nothing to link to.
                ("other_lib", Some("^1"), true, &V2Provider::Missing),
                ("map_icons", None, false, &listed("MapIcon1", "Map Icons")),
            ]
        );
    }

    #[test]
    fn a_build_outside_the_index_builds_is_unlisted_and_nothing_fits_it() {
        let listed = "26.3-0.1.0";
        let unlisted = "nightly-2026-10-01-0000000";
        let project = make_project(
            "Ab3dE9xQ",
            "wayfinder",
            "Wayfinder",
            "Minimap with waypoints",
            &["utility"],
            vec![make_release(
                "1.0.0",
                "2026-11-10",
                listed,
                &[listed],
                &[],
                '1',
            )],
        );
        let index = sample_index(vec![project], Vec::new());
        assert!(build_listed(&index, Some(listed)));
        assert!(build_listed(&index, None));
        assert!(!build_listed(&index, Some(unlisted)));
        let cards = cards(&index, Some(unlisted), &label_build, &LocalFits::default());
        assert!(discover(&cards, &V2Filters::default()).is_empty());
        let all = V2Filters {
            show_all: true,
            ..V2Filters::default()
        };
        assert_eq!(discover(&cards, &all).len(), 1);
    }

    #[test]
    fn a_collection_picks_its_default_members_that_fit_and_installs_only_the_rest() {
        let build = "26.3-0.1.0";
        let project = |id: &str, slug: &str, fits: &[&str], ch| {
            let not = if fits.is_empty() { vec![build] } else { vec![] };
            make_project(
                id,
                slug,
                slug,
                "A block set.",
                &["utility"],
                vec![make_release("1.0.0", "2026-11-10", build, fits, &not, ch)],
            )
        };
        let mut index = sample_index(
            vec![
                project("Ab3dE9xQ", "fences", &[build], '1'),
                project("Bc4eF0yR", "doors", &[build], '2'),
                project("Cd5fG1zS", "roofs", &[], '3'),
            ],
            vec![],
        );
        let member = |project: &str, default| Member {
            project: project.to_owned(),
            default,
        };
        index.collections = vec![Collection {
            id: "Qm4tZ81c".to_owned(),
            name: "Macaw's Builds".to_owned(),
            summary: "Fences, doors and roofs.".to_owned(),
            description: String::new(),
            publisher: "author".to_owned(),
            icon: format!("pages/Qm4tZ81c/{}.png", hex64('9')),
            members: vec![
                member("Ab3dE9xQ", true),
                member("Bc4eF0yR", false),
                member("Cd5fG1zS", true),
            ],
        }];
        let cards = cards(&index, Some(build), &label_build, &LocalFits::default());
        let found = collections(&index, &cards);
        let collection = &found[0];
        assert_eq!(collection.fitting(&cards), 2);
        // Doors is off by default, and Roofs has no release for this build.
        let picks = collection.initial_picks(&cards);
        assert_eq!(picks, BTreeSet::from(["Ab3dE9xQ".to_owned()]));
        let all = ["Ab3dE9xQ", "Bc4eF0yR", "Cd5fG1zS"]
            .map(str::to_owned)
            .into();
        assert_eq!(
            collection.to_install(&cards, &all, &|id| id == "Ab3dE9xQ"),
            ["Bc4eF0yR"]
        );

        let filters = |text: &str, category: Option<&str>| V2Filters {
            text: text.to_owned(),
            category: category.map(str::to_owned),
            ..V2Filters::default()
        };
        let shown = |text, category| discover_collections(&found, &cards, &filters(text, category));
        assert_eq!(shown("macaw", None), [0]);
        assert_eq!(
            shown("doors", None),
            [0],
            "a member's name finds its collection"
        );
        assert!(shown("lantern", None).is_empty());
        // Under a category the collection stands in for its members in it, as when browsing.
        assert_eq!(shown("", Some("utility")), [0]);
        assert!(shown("", Some("decoration")).is_empty());

        // Browsing shows the collection's card in place of its members, Show all included; a
        // search finds a member.
        let mods = |text: &str, show_all| {
            let filters = V2Filters {
                text: text.to_owned(),
                show_all,
                ..V2Filters::default()
            };
            let mut shown = discover(&cards, &filters);
            fold_members(&mut shown, &found, &filters);
            shown.len()
        };
        assert_eq!(mods("", false), 0);
        assert_eq!(mods("", true), 0);
        assert_eq!(mods("doors", false), 1);
        // Members stay folded under a category too: only their collection shows there.
        let in_category = V2Filters {
            category: Some("utility".to_owned()),
            ..V2Filters::default()
        };
        let mut shown = discover(&cards, &in_category);
        fold_members(&mut shown, &found, &in_category);
        assert!(shown.is_empty());
    }

    #[test]
    fn a_local_check_fills_only_cards_the_index_says_nothing_of() {
        let stable = "26.3-0.1.0";
        let nightly = "nightly-2026-11-20-abcdef0";
        let project = |id: &str, fits: &[&str], not: &[&str], ch| {
            make_project(
                id,
                id,
                id,
                "summary",
                &["utility"],
                vec![make_release("1.0.0", "2026-11-10", stable, fits, not, ch)],
            )
        };
        let index = sample_index(
            vec![
                project("AAAAAAAA", &[], &[], '1'),
                project("BBBBBBBB", &[], &[], '2'),
                project("CCCCCCCC", &[], &[nightly], '3'),
                project("DDDDDDDD", &[], &[], '4'),
            ],
            Vec::new(),
        );
        let local = LocalFits {
            found: local_fit::Found::from([
                (hex64('1'), local_fit::Local::Fits),
                (hex64('2'), local_fit::Local::DoesNotFit),
                // The index's own verdict wins over this.
                (hex64('3'), local_fit::Local::Fits),
            ]),
            checking: BTreeSet::from([hex64('4')]),
        };
        let fit = |id: &str| {
            cards(&index, Some(nightly), &label_build, &local)
                .into_iter()
                .find(|c| c.id == id)
                .unwrap()
                .fit
        };
        let a = fit("AAAAAAAA");
        assert_eq!(
            a,
            CardFit::FitsHere {
                version: "1.0.0".into()
            }
        );
        assert!(a.fits() && !a.faded());
        assert!(matches!(fit("BBBBBBBB"), CardFit::DoesNotFitHere { .. }));
        assert!(matches!(fit("CCCCCCCC"), CardFit::DoesNotFit { .. }));
        let d = fit("DDDDDDDD");
        assert!(matches!(d, CardFit::Checking { .. }));
        assert!(
            !d.fits() && !d.faded(),
            "a card being checked isn't drawn as unfit"
        );

        // On a build the index doesn't list, every card takes this computer's answer, the one
        // the index judged only for another build included.
        let unlisted = |id: &str| {
            cards(&index, Some("dev-2026-11-30-1234567"), &label_build, &local)
                .into_iter()
                .find(|c| c.id == id)
                .unwrap()
                .fit
        };
        assert!(matches!(unlisted("AAAAAAAA"), CardFit::FitsHere { .. }));
        assert!(matches!(
            unlisted("BBBBBBBB"),
            CardFit::DoesNotFitHere { .. }
        ));
        assert!(matches!(unlisted("CCCCCCCC"), CardFit::FitsHere { .. }));
        assert!(matches!(unlisted("DDDDDDDD"), CardFit::Checking { .. }));
    }
}
