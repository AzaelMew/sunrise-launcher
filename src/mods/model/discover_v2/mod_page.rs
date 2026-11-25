//! The Mod Page: one project, its releases and its dependencies.
use super::*;

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
    /// The selected build's label when the release lists this dependency in
    /// `dependency_unavailable` for it: the provider has no release for that build.
    pub(in crate::mods) no_release_for: Option<String>,
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
    let release_of_page = picked.or(sorted.first().copied());
    let mut dependencies: Vec<V2Dependency> = release_of_page
        .map_or(&[][..], |release| &release.dependencies)
        .iter()
        .map(|dep| V2Dependency {
            mod_id: dep.mod_id.clone(),
            range: (dep.version != semver::VersionReq::STAR).then(|| dep.version.to_string()),
            required: dep.required,
            provider: provider(index, dep),
            no_release_for: exact_build
                .filter(|build| {
                    release_of_page
                        .is_some_and(|r| unavailable_dependencies(r, build).contains(&dep.mod_id))
                })
                .map(build_label),
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
