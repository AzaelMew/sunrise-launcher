//! Grouping competing ports and the switch requests between them, without a window.

use super::model::{is_mod_project, matches_query, pick_release};
use super::*;

fn build_switch_request(
    from_project_id: &str,
    from_repo: &str,
    from_version: &str,
    to: &index_v2::Project,
    to_release: &index_v2::Release,
) -> Option<PortSwitch> {
    (from_project_id != to.id).then(|| PortSwitch {
        mod_id: to.mod_id.clone(),
        mod_name: to.name.clone(),
        from_repo: from_repo.to_owned(),
        from_version: from_version.to_owned(),
        to_project_id: to.id.clone(),
        to_repo: to.repository.name.clone(),
        to_version: to_release.version.clone(),
    })
}

/// Groups v2 projects sharing an in-game mod ID when two or more distinct non-revoked projects
/// provide the same slot. Single-project mods stay ungrouped. Without `show_all`, a group keeps
/// only the ports that fit the build (and the installed one) and needs two of them; a lone
/// fitting port shows as an ordinary card.
pub(crate) fn group_ports(
    index: &index_v2::Index,
    build_id: Option<&str>,
    installed: &BTreeMap<String, InstalledPort>,
    query: &str,
    category: Option<&str>,
    show_all: bool,
) -> Vec<PortGroup> {
    let mut by_mod: BTreeMap<&str, Vec<&index_v2::Project>> = BTreeMap::new();
    for project in &index.projects {
        if !is_mod_project(project) || index.revoked(&project.id, None).is_some() {
            continue;
        }
        by_mod
            .entry(project.mod_id.as_str())
            .or_default()
            .push(project);
    }

    let mut groups = Vec::new();
    for (mod_id, projects) in by_mod {
        if projects.len() < 2 {
            continue;
        }
        if !projects.iter().any(|p| matches_query(p, query)) {
            continue;
        }
        if category.is_some_and(|cat| {
            !projects
                .iter()
                .any(|p| p.categories.iter().any(|c| c == cat))
        }) {
            continue;
        }
        let installed_here = installed.get(mod_id);
        let mut ports: Vec<PortOption> = projects
            .iter()
            .map(|project| {
                let (release, fit) = pick_release(index, project, build_id);
                let latest_version = release.map(|r| r.version.clone());
                let (action, switch_request) = match installed_here {
                    Some(inst) if inst.project_id == project.id => (PortAction::Installed, None),
                    _ if fit != FitVerdict::Fits || release.is_none() => {
                        (PortAction::Unavailable, None)
                    }
                    Some(inst) => {
                        let req = release.and_then(|r| {
                            build_switch_request(
                                &inst.project_id,
                                &inst.repo,
                                &inst.version,
                                project,
                                r,
                            )
                        });
                        (PortAction::SwitchToPort, req)
                    }
                    None => (PortAction::Install, None),
                };
                PortOption {
                    project_id: project.id.clone(),
                    slug: project.slug.clone(),
                    name: project.name.clone(),
                    summary: project.summary.clone(),
                    publisher_repo: project.repository.name.clone(),
                    categories: project.categories.clone(),
                    icon: project.icon.clone(),
                    latest_version,
                    fit,
                    action,
                    switch_request,
                }
            })
            .collect();
        if !show_all {
            ports.retain(|p| p.fit == FitVerdict::Fits || p.action == PortAction::Installed);
            if ports.len() < 2 {
                continue;
            }
        }

        // Keep the currently installed port first, then fitting alternatives, then by publisher.
        ports.sort_by(|a, b| {
            let rank = |p: &PortOption| match (p.action, p.fit) {
                (PortAction::Installed, _) => 0u8,
                (_, FitVerdict::Fits) => 1u8,
                (_, FitVerdict::Unknown) => 2u8,
                (_, FitVerdict::DoesNotFit) => 3u8,
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.publisher_repo
                    .to_lowercase()
                    .cmp(&b.publisher_repo.to_lowercase())
            })
        });

        // The plainest name heads the group ("Simply Light", not "Simply Light (ports collection)").
        let display_name = ports
            .iter()
            .map(|p| p.name.as_str())
            .min_by_key(|name| name.chars().count())
            .map_or_else(|| mod_id.to_owned(), str::to_owned);
        groups.push(PortGroup {
            mod_id: mod_id.to_owned(),
            display_name,
            ports,
        });
    }

    groups.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.mod_id.cmp(&b.mod_id))
    });
    groups
}

/// Every project ID and slug currently displayed inside a competing-ports group.
pub(crate) fn grouped_project_ids(groups: &[PortGroup]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for group in groups {
        for port in &group.ports {
            ids.insert(port.project_id.clone());
            ids.insert(port.slug.clone());
        }
    }
    ids
}

/// Finds other reviewed projects in `index` providing `current_mod_id` besides `current_project_id`.
pub(crate) fn other_ports_for(
    index: &index_v2::Index,
    current_project_id: &str,
    current_mod_id: &str,
    current_version: &str,
    build_id: Option<&str>,
) -> Option<OtherPorts> {
    let current = index
        .project(current_project_id)
        .or_else(|| index.project_by_slug(current_project_id));
    let mod_id = current.map(|p| p.mod_id.as_str()).unwrap_or(current_mod_id);
    let from_project_id = current.map(|p| p.id.as_str()).unwrap_or(current_project_id);
    let from_repo = current
        .map(|p| p.repository.name.as_str())
        .unwrap_or(current_project_id);
    let mod_name = current
        .map(|p| p.name.clone())
        .unwrap_or_else(|| mod_id.to_owned());

    let mut alternatives = Vec::new();
    for project in index.projects_for_mod(mod_id) {
        if !is_mod_project(project)
            || index.revoked(&project.id, None).is_some()
            || project.id == from_project_id
            || project.slug == current_project_id
        {
            continue;
        }
        let (release, fit) = pick_release(index, project, build_id);
        let switch_request = (fit == FitVerdict::Fits)
            .then_some(release)
            .flatten()
            .and_then(|r| {
                build_switch_request(from_project_id, from_repo, current_version, project, r)
            });
        alternatives.push(OtherPortOption {
            publisher_repo: project.repository.name.clone(),
            fit,
            switch_request,
        });
    }
    if alternatives.is_empty() {
        return None;
    }
    alternatives.sort_by(|a, b| {
        let fit_rank = |f: FitVerdict| match f {
            FitVerdict::Fits => 0u8,
            FitVerdict::Unknown => 1u8,
            FitVerdict::DoesNotFit => 2u8,
        };
        fit_rank(a.fit).cmp(&fit_rank(b.fit)).then_with(|| {
            a.publisher_repo
                .to_lowercase()
                .cmp(&b.publisher_repo.to_lowercase())
        })
    });
    Some(OtherPorts {
        mod_id: mod_id.to_owned(),
        mod_name,
        alternatives,
    })
}
