//! The builds each release lists, for the Mod Page's versions.
use super::*;

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
