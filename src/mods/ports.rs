//! Competing ports sharing an in-game mod ID (mocks 03 and 04a).
//!
//! Projects in the v2 index that share a `mod` slot are grouped as alternatives labelled by
//! their GitHub publisher (`owner/repo`), never as a collection or a single install for both.
//! An installed mod's page lists other reviewed ports of the same in-game ID and launches the
//! staged provider switch (`Launcher::begin_provider_switch`).
use super::model::Row;
use super::ui::*;
use super::*;
use crate::paint::palette;
use std::collections::BTreeMap;
use sunrise_launcher::browser::{
    details::category_label,
    images,
    sunrise::{v2, ProviderSwitchRequest},
};
use sunrise_launcher_smod::index_v2::{self, FitVerdict};

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
    pub(crate) switch_request: Option<ProviderSwitchRequest>,
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
    pub(crate) switch_request: Option<ProviderSwitchRequest>,
}

/// Alternative reviewed projects providing the same in-game mod ID as the open installed mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OtherPorts {
    pub(crate) mod_id: String,
    pub(crate) mod_name: String,
    pub(crate) alternatives: Vec<OtherPortOption>,
}

fn is_mod_project(project: &index_v2::Project) -> bool {
    matches!(
        project.kind,
        index_v2::Kind::Mod | index_v2::Kind::NativeMod
    )
}

fn pick_release<'a>(
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

fn matches_query(project: &index_v2::Project, query: &str) -> bool {
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

fn build_switch_request(
    from_project_id: &str,
    from_repo: &str,
    from_version: &str,
    to: &index_v2::Project,
    to_release: &index_v2::Release,
) -> Option<ProviderSwitchRequest> {
    if from_project_id == to.id {
        return None;
    }
    Some(ProviderSwitchRequest {
        mod_id: to.mod_id.clone(),
        mod_name: to.name.clone(),
        from_project_id: from_project_id.to_owned(),
        from_repo: from_repo.to_owned(),
        from_version: from_version.to_owned(),
        to_project_id: to.id.clone(),
        to_slug: to.slug.clone(),
        to_repo: to.repository.name.clone(),
        to_repo_path: to.repository.path.clone(),
        to_version: to_release.version.clone(),
        to_workflow: to_release.workflow.clone(),
        to_repo_id: to.repository.id,
        to_file: to_release.file.clone(),
        to_kind: to.kind.clone(),
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

impl Launcher {
    pub(crate) fn refresh_mods_after_port_switch(
        &mut self,
        notice: Result<String, String>,
        open_project_id: Option<String>,
    ) {
        self.ui.mods.snapshot = None;
        self.ui.mods.notice = Some(notice);
        if let Some(id) = open_project_id.filter(|_| self.ui.page == Page::Project) {
            self.mods_show_project(id);
        }
    }

    pub(super) fn discover_port_groups(&self) -> Vec<PortGroup> {
        let Some(catalog) = v2::known_index() else {
            return Vec::new();
        };
        let index = &catalog.index;
        let mut installed_map = BTreeMap::new();
        if let Some(rows) = self.discover_installed() {
            for entry in &rows.entries {
                if entry.source != SourceId::Sunrise || entry.kind != Kind::Mod {
                    continue;
                }
                let project = index
                    .project(&entry.project)
                    .or_else(|| index.project_by_slug(&entry.slug));
                let Some(mod_id) = project
                    .map(|p| p.mod_id.as_str())
                    .or_else(|| sunrise::mod_id(entry))
                else {
                    continue;
                };
                installed_map.insert(
                    mod_id.to_owned(),
                    InstalledPort {
                        project_id: project
                            .map(|p| p.id.clone())
                            .unwrap_or_else(|| entry.project.clone()),
                        version: entry.number.clone(),
                        repo: project
                            .map(|p| p.repository.name.clone())
                            .unwrap_or_else(|| entry.project.clone()),
                    },
                );
            }
        }
        let build_id = self.ports_build();
        group_ports(
            index,
            build_id.as_deref(),
            &installed_map,
            &self.ui.mods.filters.text,
            self.ui.mods.filters.category.as_deref(),
            self.ui.mods.filters.show_all,
        )
    }

    /// The selected profile's exact Sunrise build, as the index's `fits` lists name it: a native
    /// profile's version carries a build suffix that its base build drops.
    fn ports_build(&self) -> Option<String> {
        let raw = self.profile_target(self.selected_instance()?)?;
        if native_mods::is_native(&raw) {
            crate::glass_instance::native_base(&raw).or(Some(raw))
        } else {
            Some(raw)
        }
    }

    pub(super) fn with_port_groups(
        &mut self,
        grid: Div,
        ungrouped_cards: usize,
        groups: &[PortGroup],
        cx: &mut Context<Self>,
    ) -> Div {
        if groups.is_empty() {
            grid
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(self.render_port_groups(groups, cx))
                .when(ungrouped_cards > 0, |d| d.child(grid))
        }
    }

    fn render_port_groups(&mut self, groups: &[PortGroup], cx: &mut Context<Self>) -> Div {
        let busy = self.mods_busy() || self.mods_locked().is_some();
        let mut container = div().flex().flex_col().gap(px(12.));
        for (group_idx, group) in groups.iter().enumerate() {
            let header = div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(tokens::BODY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(palette().text))
                        .child(trf(
                            "{count} ports of {name}",
                            &[
                                ("count", &group.ports.len().to_string()),
                                ("name", &group.display_name),
                            ],
                        )),
                )
                .child(muted(trf(
                    "Same in-game ID ({id}) · Pick one provider per profile",
                    &[("id", &group.mod_id)],
                )));

            let mut cards_row = div().flex().flex_wrap().gap(px(12.));
            for (port_idx, port) in group.ports.iter().enumerate() {
                let picture = self.v2_mod_image(&port.project_id, &port.icon, images::ICON);
                let is_installed = port.action == PortAction::Installed;
                let success = c(palette().success);
                let fit_badge = port.latest_version.as_ref().map(|ver| {
                    let (label, color) = match port.fit {
                        FitVerdict::Fits => (
                            trf("{version} · Fits build", &[("version", ver)]),
                            palette().success,
                        ),
                        FitVerdict::DoesNotFit => (
                            trf("{version} · Doesn't fit build", &[("version", ver)]),
                            palette().muted,
                        ),
                        FitVerdict::Unknown => (
                            trf("{version} · Not checked yet", &[("version", ver)]),
                            palette().muted,
                        ),
                    };
                    div()
                        .flex_none()
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(crate::tokens::r(crate::tokens::R_SMALL))
                        .border_1()
                        .border_color(c(color).opacity(0.4))
                        .bg(c(color).opacity(0.1))
                        .text_size(px(tokens::CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(color))
                        .child(label)
                });

                let view_id = port.project_id.clone();
                let view_btn =
                    crate::frame::button(("alt-port-view", group_idx * 100 + port_idx), tr("View"))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.mods_show_project(view_id.clone());
                            cx.notify();
                        }));

                let action_el: AnyElement = match port.action {
                    PortAction::Installed => div()
                        .flex_none()
                        .h(px(tokens::CONTROL))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                        .border_1()
                        .border_color(success.opacity(0.45))
                        .bg(success.opacity(0.12))
                        .text_size(px(tokens::LABEL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(success)
                        .child(tr("Installed"))
                        .into_any_element(),
                    PortAction::SwitchToPort => {
                        let btn = crate::frame::button(
                            ("alt-port-switch", group_idx * 100 + port_idx),
                            tr("Switch to this port"),
                        );
                        if let Some(req) = port.switch_request.clone().filter(|_| !busy) {
                            btn.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.begin_provider_switch(req.clone());
                                cx.notify();
                            }))
                            .into_any_element()
                        } else {
                            dim(btn).into_any_element()
                        }
                    }
                    PortAction::Install => {
                        let btn = crate::frame::button(
                            ("alt-port-install", group_idx * 100 + port_idx),
                            tr("Install"),
                        );
                        if busy {
                            dim(btn).into_any_element()
                        } else {
                            let id = port.project_id.clone();
                            btn.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.add(id.clone());
                                cx.notify();
                            }))
                            .into_any_element()
                        }
                    }
                    PortAction::Unavailable => dim(crate::frame::button(
                        ("alt-port-install", group_idx * 100 + port_idx),
                        tr("Install"),
                    ))
                    .into_any_element(),
                };

                let port_card = card()
                    .flex_1()
                    .min_w(px(300.))
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .when(is_installed, |d| d.border_color(success.opacity(0.45)))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(tile(picture, &port.project_id, &port.name, 40.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(tokens::BODY))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(c(palette().text))
                                                    .child(port.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .self_start()
                                                    .max_w_full()
                                                    .truncate()
                                                    .px(px(6.))
                                                    .py(px(1.))
                                                    .rounded(crate::tokens::r(
                                                        crate::tokens::R_SMALL,
                                                    ))
                                                    .border_1()
                                                    .border_color(c(palette().divider))
                                                    .bg(c(palette().field))
                                                    .font_family(crate::theme::MONO)
                                                    .text_size(px(tokens::CAPTION))
                                                    .text_color(c(palette().muted))
                                                    .child(port.publisher_repo.clone()),
                                            ),
                                    ),
                            )
                            .children(fit_badge),
                    )
                    .child(muted(port.summary.clone()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div().flex().flex_wrap().gap(px(6.)).children(
                                    port.categories
                                        .iter()
                                        .take(3)
                                        .map(|cat| category_chip(category_label(cat))),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(view_btn)
                                    .child(action_el),
                            ),
                    );
                cards_row = cards_row.child(port_card);
            }

            container = container.child(
                card()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(header)
                    .child(cards_row),
            );
        }
        container
    }

    pub(super) fn other_ports_card(
        &self,
        project_id: &str,
        installed_row: Option<&Row>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let row = installed_row?;
        if row.local.is_some() {
            return None;
        }
        let catalog = v2::known_index()?;
        let profile_name = self
            .selected_instance()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| tr("Sunrise").into());
        let build_id = self.ports_build();
        let current_project = row
            .entry
            .as_ref()
            .map(|entry| entry.project.as_str())
            .unwrap_or(project_id);
        let other = other_ports_for(
            &catalog.index,
            current_project,
            &row.id,
            &row.version,
            build_id.as_deref(),
        )?;
        let heading = if other.alternatives.len() == 1 {
            trf(
                "OTHER PORT OF {mod}",
                &[("mod", &other.mod_name.to_uppercase())],
            )
        } else {
            trf(
                "OTHER PORTS OF {mod}",
                &[("mod", &other.mod_name.to_uppercase())],
            )
        };
        let description = if other.alternatives.len() == 1 {
            trf(
                "Another reviewed project provides the {id} in-game ID. Switching replaces this port on {profile}.",
                &[("id", &other.mod_id), ("profile", &profile_name)],
            )
        } else {
            trf(
                "Other reviewed projects provide the {id} in-game ID. Switching replaces this port on {profile}.",
                &[("id", &other.mod_id), ("profile", &profile_name)],
            )
        };

        let mut box_el = card()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(tokens::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(palette().muted))
                    .child(heading),
            )
            .child(muted(description));

        for (i, alt) in other.alternatives.iter().enumerate() {
            let label = trf("Switch to {repo}", &[("repo", &alt.publisher_repo)]);
            let btn = crate::frame::button(("alt-project-switch-port", i), label).w_full();
            let control = if let Some(req) = alt.switch_request.clone().filter(|_| !busy) {
                btn.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.begin_provider_switch(req.clone());
                    cx.notify();
                }))
            } else {
                dim(btn)
            };
            let fit_note = match alt.fit {
                FitVerdict::Fits => None,
                FitVerdict::DoesNotFit => Some(note(tr("Doesn't fit this build"))),
                FitVerdict::Unknown => Some(note(tr("Not checked for this build yet"))),
            };
            box_el = box_el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(control)
                    .children(fit_note),
            );
        }
        Some(box_el)
    }
}

fn category_chip(label: String) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(crate::tokens::r(crate::tokens::R_SMALL))
        .border_1()
        .border_color(c(palette().line))
        .text_size(px(tokens::CAPTION))
        .text_color(c(palette().muted))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(
        id: &str,
        mod_id: &str,
        name: &str,
        repo: (u64, &str),
        version: &str,
        fit: (&[&str], &[&str]),
    ) -> index_v2::Project {
        let (repo_id, repo_name) = repo;
        let (fits, does_not_fit) = fit;
        let slug = repo_name.rsplit('/').next().unwrap_or(mod_id);
        index_v2::Project {
            id: id.into(),
            slug: slug.into(),
            mod_id: mod_id.into(),
            kind: index_v2::Kind::Mod,
            name: name.into(),
            summary: format!("{name} from {repo_name}"),
            description: String::new(),
            authors: vec!["Author".into()],
            owners: vec!["owner".into()],
            license: "MIT".into(),
            homepage: None,
            issues: None,
            source: None,
            icon: format!("pages/{id}/{}.png", "a".repeat(64)),
            screenshots: vec![],
            categories: vec!["utility".into()],
            repository: index_v2::Repository {
                id: repo_id,
                name: repo_name.into(),
                path: ".".into(),
            },
            workflow: "sunrise-org/sunrise-mods/.github/workflows/release.yml".into(),
            releases: vec![index_v2::Release {
                version: version.into(),
                published: "2026-11-22".into(),
                sunrise: "26.3-0.1.0".into(),
                workflow:
                    "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1"
                        .into(),
                fits: fits.iter().map(|s| (*s).to_owned()).collect(),
                does_not_fit: does_not_fit.iter().map(|s| (*s).to_owned()).collect(),
                file: index_v2::File {
                    url: format!("https://example.invalid/{slug}-{version}.smod"),
                    size: 128,
                    sha256: "b".repeat(64),
                    attestation: format!("https://example.invalid/{slug}-{version}.sigstore.json"),
                },
                dependencies: vec![],
            }],
        }
    }

    fn index_with(projects: Vec<index_v2::Project>) -> index_v2::Index {
        index_v2::Index {
            schema: index_v2::SCHEMA,
            serial: 1,
            generated: "2026-11-22T00:00:00Z".into(),
            builds: vec![],
            categories: vec![index_v2::Category {
                id: "utility".into(),
                icon: "wrench".into(),
                kinds: vec![index_v2::Kind::Mod],
            }],
            projects,
            revoked: vec![],
        }
    }

    #[test]
    fn group_ports_groups_competing_ports_by_mod_id_with_distinct_publishers() {
        let index = index_with(vec![
            project(
                "jade0001",
                "jade",
                "Jade",
                (10, "joeypc6/sunrise-jade"),
                "26.3.8",
                (&["26.3-0.1.0"], &[]),
            ),
            project(
                "jade0002",
                "jade",
                "Jade",
                (20, "zekrenlight/jade-sunrise"),
                "1.4.2",
                (&["26.3-0.1.0"], &[]),
            ),
            project(
                "solo0001",
                "frontier",
                "Frontier",
                (30, "Zekrenlight/frontier"),
                "1.2.0",
                (&["26.3-0.1.0"], &[]),
            ),
        ]);

        let groups = group_ports(&index, Some("26.3-0.1.0"), &BTreeMap::new(), "", None, true);
        assert_eq!(groups.len(), 1);
        let jade = &groups[0];
        assert_eq!(jade.mod_id, "jade");
        assert_eq!(jade.display_name, "Jade");
        assert_eq!(jade.ports.len(), 2);
        assert_eq!(jade.ports[0].publisher_repo, "joeypc6/sunrise-jade");
        assert_eq!(jade.ports[1].publisher_repo, "zekrenlight/jade-sunrise");
        // Each competing port has its own Install action; never one shared install.
        assert_eq!(jade.ports[0].action, PortAction::Install);
        assert_eq!(jade.ports[1].action, PortAction::Install);
        assert_ne!(jade.ports[0].project_id, jade.ports[1].project_id);

        // Single-project mods stay out of competing-port groups.
        let grouped = grouped_project_ids(&groups);
        assert!(grouped.contains("jade0001"));
        assert!(grouped.contains("jade0002"));
        assert!(!grouped.contains("solo0001"));
    }

    #[test]
    fn group_ports_marks_installed_port_and_offers_switch_on_alternatives() {
        let index = index_with(vec![
            project(
                "jade0001",
                "jade",
                "Jade",
                (10, "joeypc6/sunrise-jade"),
                "26.3.8",
                (&["26.3-0.1.0"], &[]),
            ),
            project(
                "jade0002",
                "jade",
                "Jade",
                (20, "zekrenlight/jade-sunrise"),
                "1.4.2",
                (&["26.3-0.1.0"], &[]),
            ),
        ]);
        let mut installed = BTreeMap::new();
        installed.insert(
            "jade".into(),
            InstalledPort {
                project_id: "jade0001".into(),
                version: "26.3.8".into(),
                repo: "joeypc6/sunrise-jade".into(),
            },
        );

        let groups = group_ports(&index, Some("26.3-0.1.0"), &installed, "", None, true);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].ports[0].action, PortAction::Installed);
        assert_eq!(groups[0].ports[1].action, PortAction::SwitchToPort);
        let req = groups[0].ports[1].switch_request.as_ref().unwrap();
        assert_eq!(req.from_project_id, "jade0001");
        assert_eq!(req.to_project_id, "jade0002");
        assert_eq!(req.to_repo, "zekrenlight/jade-sunrise");
    }

    #[test]
    fn group_ports_marks_unfitting_port_unavailable() {
        let index = index_with(vec![
            project(
                "jade0001",
                "jade",
                "Jade",
                (10, "joeypc6/sunrise-jade"),
                "26.3.8",
                (&["26.3-0.1.0"], &[]),
            ),
            project(
                "jade0002",
                "jade",
                "Jade",
                (20, "zekrenlight/jade-sunrise"),
                "1.4.2",
                (&[], &["26.3-0.1.0"]),
            ),
        ]);

        let groups = group_ports(&index, Some("26.3-0.1.0"), &BTreeMap::new(), "", None, true);
        assert_eq!(groups[0].ports[0].action, PortAction::Install);
        assert_eq!(groups[0].ports[1].fit, FitVerdict::DoesNotFit);
        assert_eq!(groups[0].ports[1].action, PortAction::Unavailable);
        assert!(groups[0].ports[1].switch_request.is_none());

        // Without Show all, the port that doesn't fit leaves, and one fitting port is no group.
        assert!(group_ports(
            &index,
            Some("26.3-0.1.0"),
            &BTreeMap::new(),
            "",
            None,
            false
        )
        .is_empty());
    }

    #[test]
    fn other_ports_for_returns_alternatives_with_switch_requests() {
        let index = index_with(vec![
            project(
                "jade0001",
                "jade",
                "Jade",
                (10, "joeypc6/sunrise-jade"),
                "26.3.8",
                (&["26.3-0.1.0"], &[]),
            ),
            project(
                "jade0002",
                "jade",
                "Jade",
                (20, "zekrenlight/jade-sunrise"),
                "1.4.2",
                (&["26.3-0.1.0"], &[]),
            ),
        ]);

        let other =
            other_ports_for(&index, "jade0001", "jade", "26.3.8", Some("26.3-0.1.0")).unwrap();
        assert_eq!(other.mod_id, "jade");
        assert_eq!(other.mod_name, "Jade");
        assert_eq!(other.alternatives.len(), 1);
        assert_eq!(
            other.alternatives[0].publisher_repo,
            "zekrenlight/jade-sunrise"
        );
        let req = other.alternatives[0].switch_request.as_ref().unwrap();
        assert_eq!(req.from_repo, "joeypc6/sunrise-jade");
        assert_eq!(req.from_version, "26.3.8");
        assert_eq!(req.to_project_id, "jade0002");
        assert_eq!(req.to_repo, "zekrenlight/jade-sunrise");
        assert_eq!(req.to_version, "1.4.2");

        // No alternatives for a solo project.
        let solo = index_with(vec![project(
            "solo0001",
            "frontier",
            "Frontier",
            (30, "Zekrenlight/frontier"),
            "1.2.0",
            (&["26.3-0.1.0"], &[]),
        )]);
        assert!(
            other_ports_for(&solo, "solo0001", "frontier", "1.2.0", Some("26.3-0.1.0")).is_none()
        );
    }
}
