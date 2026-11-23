//! Discover and Mod Page view models built strictly from the signed v2 registry index.
//!
//! Pure data in and out: no files, no network, no GPUI. Compatibility comes solely from each
//! release's generated `fits` and `does_not_fit` lists for the selected profile's exact build,
//! never inferred from `sunrise` or semver. Images are kept only when their path is
//! content-addressed (`pages/<project id>/<sha256>.<png|jpg>`).
use super::{Make, SourceLink};
use std::cmp::Ordering;
use std::collections::HashMap;
use sunrise_launcher::i18n::{tr, trf};
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
    /// The project or every one of its releases is taken down in `index.revoked`.
    Revoked(String),
    /// The selected profile has no game build yet.
    NoBuild,
}

impl CardFit {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, CardFit::Fits { .. })
    }

    pub(in crate::mods) fn fitting_version(&self) -> Option<&str> {
        match self {
            CardFit::Fits { version, .. } => Some(version),
            _ => None,
        }
    }

    /// The reason a card is greyed out and cannot be installed on the selected profile.
    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            CardFit::Fits { .. } => None,
            CardFit::DoesNotFit { build_label } => {
                Some(trf("Doesn't fit {build}", &[("build", build_label)]))
            }
            CardFit::NotCheckedYet { build_label } => Some(trf(
                "Not checked for {build} yet",
                &[("build", build_label)],
            )),
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
    Revoked(String),
    NoBuild,
}

impl ReleaseVerdict {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, ReleaseVerdict::Fits)
    }

    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            ReleaseVerdict::Fits => None,
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
        FitVerdict::Unknown => ReleaseVerdict::NotCheckedYet(build_label(build)),
    }
}

fn project_fit(
    index: &Index,
    project: &Project,
    sorted_releases: &[&Release],
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
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

    let label = build_label(build);
    let fit = match newest_active.fit_verdict(build) {
        FitVerdict::DoesNotFit => CardFit::DoesNotFit { build_label: label },
        FitVerdict::Unknown | FitVerdict::Fits => CardFit::NotCheckedYet { build_label: label },
    };
    (fit, newest_active.published.clone())
}

/// Builds every mod card from a verified v2 index for `exact_build`.
pub(in crate::mods) fn cards(
    index: &Index,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
) -> Vec<V2Card> {
    index
        .projects
        .iter()
        .filter_map(|p| {
            let make = make_of(&p.kind)?;
            let mut sorted: Vec<&Release> = p.releases.iter().collect();
            sorted.sort_by(|a, b| compare_releases_newest_first(a, b));
            let (fit, updated) = project_fit(index, p, &sorted, exact_build, build_label);
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

/// Indices into `cards` matching `filters`, ordered by `filters.sort`.
/// Whether the index lists `exact_build` among its offered builds. Discover uses only the index's
/// precomputed fits, so a profile on an unlisted build has no fitting mods to show; a profile
/// without a build is handled by its own empty state.
pub(in crate::mods) fn build_listed(index: &Index, exact_build: Option<&str>) -> bool {
    exact_build.is_none_or(|build| index.build(build).is_some())
}

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

/// Builds a v2 mod page from `index` for project `id` and `exact_build`.
pub(in crate::mods) fn page(
    index: &Index,
    id: &str,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
) -> Option<V2Page> {
    let p = index.project(id)?;
    let make = make_of(&p.kind)?;
    let mut sorted: Vec<&Release> = p.releases.iter().collect();
    sorted.sort_by(|a, b| compare_releases_newest_first(a, b));

    let (fit, updated) = project_fit(index, p, &sorted, exact_build, build_label);
    let mut fitting_versions = Vec::new();
    let mut other_versions = Vec::new();
    let mut picked: Option<&Release> = None;
    for release in &sorted {
        let verdict = release_verdict(index, p, release, exact_build, build_label);
        let line = V2VersionLine {
            version: release.version.clone(),
            published: release.published.clone(),
            size: release.file.size,
            verdict,
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
    use sunrise_launcher_smod::index_v2::{Build, Category, File, Repository, Revoked, Screenshot};

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
        let list = cards(&index, Some(build), &label_build);
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
        let list = cards(&index, Some(build), &label_build);
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
        let list = cards(&index, Some(build), &label_build);
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
        let p = page(&index, "Ab3dE9xQ", Some(build), &label_build).unwrap();
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
        let p = page(&index, "Ab3dE9xQ", Some(build), &label_build).unwrap();
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
        let cards = cards(&index, Some(unlisted), &label_build);
        assert!(discover(&cards, &V2Filters::default()).is_empty());
        let all = V2Filters {
            show_all: true,
            ..V2Filters::default()
        };
        assert_eq!(discover(&cards, &all).len(), 1);
    }
}
