//! The port models the Mods views read, and the pure helpers that build them.

use super::*;

/// Which project currently provides an in-game mod ID on the selected profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledPort {
    pub(crate) project_id: String,
    pub(crate) version: String,
    pub(crate) repo: String,
}

/// Per-port action state inside a competing-ports alternatives group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortAction {
    Installed,
    SwitchToPort,
    Install,
    Unavailable,
}

/// One distinct reviewed port inside a [`PortGroup`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortOption {
    pub(crate) project_id: String,
    pub(crate) slug: String,
    pub(crate) name: String,
    pub(crate) summary: String,
    pub(crate) publisher_repo: String,
    pub(crate) categories: Vec<String>,
    pub(crate) icon: String,
    pub(crate) latest_version: Option<String>,
    pub(crate) fit: FitVerdict,
    pub(crate) action: PortAction,
    pub(crate) switch_request: Option<PortSwitch>,
}

/// Two or more competing projects sharing the same in-game mod ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortGroup {
    pub(crate) mod_id: String,
    pub(crate) display_name: String,
    pub(crate) ports: Vec<PortOption>,
}

/// One alternative port shown on an installed mod's project page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OtherPortOption {
    pub(crate) publisher_repo: String,
    pub(crate) fit: FitVerdict,
    pub(crate) switch_request: Option<PortSwitch>,
}

/// Alternative reviewed projects providing the same in-game mod ID as the open installed mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OtherPorts {
    pub(crate) mod_id: String,
    pub(crate) mod_name: String,
    pub(crate) alternatives: Vec<OtherPortOption>,
}

pub(super) fn is_mod_project(project: &index_v2::Project) -> bool {
    matches!(
        project.kind,
        index_v2::Kind::Mod | index_v2::Kind::NativeMod
    )
}

pub(super) fn pick_release<'a>(
    index: &index_v2::Index,
    project: &'a index_v2::Project,
    build_id: Option<&str>,
) -> (Option<&'a index_v2::Release>, FitVerdict) {
    let available: Vec<&index_v2::Release> = project
        .releases
        .iter()
        .filter(|release| {
            index
                .revoked(&project.id, Some(&release.file.sha256))
                .is_none()
        })
        .collect();
    if let Some(build) = build_id {
        if let Some(fitting) = available.iter().copied().find(|r| r.fits_build(build)) {
            return (Some(fitting), FitVerdict::Fits);
        }
        if let Some(first) = available.first().copied() {
            return (Some(first), first.fit_verdict(build));
        }
        (None, FitVerdict::Unknown)
    } else if let Some(first) = available.first().copied() {
        (Some(first), FitVerdict::Fits)
    } else {
        (None, FitVerdict::Unknown)
    }
}

pub(super) fn matches_query(project: &index_v2::Project, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {} {} {} {} {} {}",
        project.id,
        project.slug,
        project.mod_id,
        project.name,
        project.summary,
        project.repository.name,
        project.authors.join(" "),
    )
    .to_lowercase();
    trimmed
        .split_whitespace()
        .all(|word| hay.contains(&word.to_lowercase()))
}
