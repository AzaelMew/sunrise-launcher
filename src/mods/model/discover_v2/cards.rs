//! Discover's cards, built from the index's projects.
use super::*;

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
    pub(super) name_lc: String,
    pub(super) slug_lc: String,
    pub(super) mod_id_lc: String,
    pub(super) summary_lc: String,
    pub(super) authors_lc: String,
    pub(super) repo_lc: String,
    pub(super) categories_lc: Vec<String>,
    pub(super) description_lc: String,
}

/// One gallery screenshot on a v2 mod page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) struct V2Screenshot {
    /// Content-addressed relative path (`pages/<id>/<sha256>.<png|jpg>`).
    pub(in crate::mods) path: String,
    pub(in crate::mods) caption: String,
}

pub(super) fn make_of(kind: &Kind) -> Option<Make> {
    match kind {
        Kind::Mod => Some(Make::Content),
        Kind::NativeMod => Some(Make::Native),
        _ => None,
    }
}

pub(super) fn content_addressed_path(project_id: &str, relative_path: &str) -> Option<String> {
    index_v2::content_addressed_image_sha256(project_id, relative_path)
        .ok()
        .map(|_| relative_path.to_owned())
}

pub(super) fn join_authors(project: &Project) -> String {
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

/// Whether the index lists `exact_build` among its offered builds: Discover notes once that an
/// unlisted build's mods are checked on this computer. A profile without a build is handled by
/// its own empty state.
pub(in crate::mods) fn build_listed(index: &Index, exact_build: Option<&str>) -> bool {
    exact_build.is_none_or(|build| index.build(build).is_some())
}
