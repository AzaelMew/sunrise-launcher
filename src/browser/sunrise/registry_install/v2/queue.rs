//! Resolving the queue of projects a plan still has to place.

use super::*;

pub(super) struct ResolveContext<'a> {
    pub(super) index: &'a Index,
    pub(super) build: &'a str,
    pub(super) installed: &'a BTreeMap<String, InstalledV2Mod>,
    pub(super) chosen_pins: &'a BTreeMap<String, String>,
    pub(super) options: &'a V2InstallOptions,
    pub(super) unfit_hashes: &'a BTreeSet<String>,
    pub(super) choose_provider: Option<&'a ChooseProvider<'a>>,
}

pub(super) fn resolve_v2_queue(
    ctx: &ResolveContext<'_>,
    mut queue: VecDeque<Queued>,
) -> Result<V2Plan, InstallError> {
    let ResolveContext {
        index,
        build,
        installed,
        chosen_pins,
        options,
        unfit_hashes,
        choose_provider,
    } = *ctx;
    let jar_cache = options.jar_cache.as_deref();
    let mut planned: BTreeMap<String, PlannedV2Release> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut optional_dependencies: Vec<index_v2::Dependency> = Vec::new();

    while let Some((project_id, is_dependency, required_by, range)) = queue.pop_front() {
        let project = index.project(&project_id).ok_or_else(|| {
            InstallError::Refused(trf(
                "Project {project} is not in the Sunrise registry.",
                &[("project", &project_id)],
            ))
        })?;
        if let Some(existing) = planned.get_mut(&project.mod_id) {
            if existing.project_id == project.id {
                if let Some(range) = range
                    .as_ref()
                    .filter(|r| !in_range(Some(*r), &existing.version))
                {
                    return Err(InstallError::NotForVersion(trf(
                        "{mod} needs {dep} {range}, but {version} is planned for another mod.",
                        &[
                            ("mod", &required_by),
                            ("dep", &project.name),
                            ("range", &range.to_string()),
                            ("version", &existing.version),
                        ],
                    )));
                }
                if !is_dependency {
                    existing.dependency = false;
                }
                continue;
            }
            return Err(InstallError::ProviderConflict(Box::new(ProviderConflict {
                mod_id: project.mod_id.clone(),
                installed_project: Some(existing.project_id.clone()),
                installed_name: existing.name.clone(),
                requested_project: project.id.clone(),
                requested_name: project.name.clone(),
            })));
        }
        if let Some(existing) = installed.get(&project.mod_id) {
            if existing.ownership.project_id() != Some(project.id.as_str()) {
                return Err(InstallError::ProviderConflict(Box::new(ProviderConflict {
                    mod_id: project.mod_id.clone(),
                    installed_project: existing.ownership.project_id().map(str::to_owned),
                    installed_name: existing.package.manifest.name.clone(),
                    requested_project: project.id.clone(),
                    requested_name: project.name.clone(),
                })));
            }
        }
        let version_override = options
            .versions
            .get(&project.id)
            .map(String::as_str)
            .or_else(|| chosen_version_pin(chosen_pins, &project.id, &project.mod_id));
        let release = select_project_release(
            index,
            project,
            build,
            version_override,
            range.as_ref(),
            unfit_hashes,
            jar_cache,
        )?;
        let step = planned_from(project, release, is_dependency);
        order.push(project.mod_id.clone());
        planned.insert(project.mod_id.clone(), step);

        for dep in &release.dependencies {
            if !dep.required {
                optional_dependencies.push(dep.clone());
                continue;
            }
            if let Some(dep_project_id) = dep.project.as_deref() {
                let dep_project = index.project(dep_project_id).ok_or_else(|| {
                    InstallError::Refused(trf(
                        "{mod} needs project {project}, which is not in the Sunrise registry.",
                        &[("mod", &project.name), ("project", dep_project_id)],
                    ))
                })?;
                if dep_project.mod_id != dep.mod_id {
                    return Err(InstallError::Refused(trf(
                        "{mod} lists project {project} for {dep}, which provides a different mod.",
                        &[
                            ("mod", &project.name),
                            ("project", dep_project_id),
                            ("dep", &dep.mod_id),
                        ],
                    )));
                }
                if let Some(existing) = installed.get(&dep.mod_id) {
                    if existing.ownership.project_id() != Some(dep_project.id.as_str()) {
                        return Err(InstallError::ProviderConflict(Box::new(ProviderConflict {
                            mod_id: dep.mod_id.clone(),
                            installed_project: existing.ownership.project_id().map(str::to_owned),
                            installed_name: existing.package.manifest.name.clone(),
                            requested_project: dep_project.id.clone(),
                            requested_name: dep_project.name.clone(),
                        })));
                    }
                    if existing.enabled
                        && dep_project.releases.iter().any(|r| {
                            r.file.sha256.eq_ignore_ascii_case(&existing.digest)
                                && in_range(Some(&dep.version), &r.version)
                                && r.fit_verdict(build) == FitVerdict::Fits
                                && index
                                    .revoked(&dep_project.id, Some(&r.file.sha256))
                                    .is_none()
                        })
                    {
                        continue;
                    }
                }
                queue.push_back((
                    dep_project.id.clone(),
                    true,
                    project.name.clone(),
                    Some(dep.version.clone()),
                ));
                continue;
            }

            // Unpinned slot dependency (`dep.project == None`): a provider already planned in
            // this install serves it only inside the range this mod asks for.
            if let Some(existing) = planned.get(&dep.mod_id) {
                if !in_range(Some(&dep.version), &existing.version) {
                    return Err(InstallError::NotForVersion(trf(
                        "{mod} needs {dep} {range}, but {version} is planned for another mod.",
                        &[
                            ("mod", &project.name),
                            ("dep", &existing.name),
                            ("range", &dep.version.to_string()),
                            ("version", &existing.version),
                        ],
                    )));
                }
                continue;
            }
            if let Some(existing) = installed.get(&dep.mod_id) {
                if existing.enabled {
                    // Keep the profile's existing provider for this slot.
                    if let Some(existing_pid) = existing.ownership.project_id() {
                        if let Some(existing_project) = index.project(existing_pid) {
                            let current_fits = existing_project.releases.iter().any(|r| {
                                r.file.sha256.eq_ignore_ascii_case(&existing.digest)
                                    && in_range(Some(&dep.version), &r.version)
                                    && r.fit_verdict(build) == FitVerdict::Fits
                                    && index
                                        .revoked(&existing_project.id, Some(&r.file.sha256))
                                        .is_none()
                            });
                            if !current_fits {
                                queue.push_back((
                                    existing_project.id.clone(),
                                    true,
                                    project.name.clone(),
                                    Some(dep.version.clone()),
                                ));
                            }
                        }
                    }
                    continue;
                }
            }
            if let Some(chosen_pid) = options.provider_choices.get(&dep.mod_id) {
                let chosen_project = index.project(chosen_pid).ok_or_else(|| {
                    InstallError::Refused(trf(
                        "Chosen provider {project} for {dep} is not in the Sunrise registry.",
                        &[("project", chosen_pid), ("dep", &dep.mod_id)],
                    ))
                })?;
                if chosen_project.mod_id != dep.mod_id {
                    return Err(InstallError::Refused(trf(
                        "Chosen provider {project} does not provide {dep}.",
                        &[("project", chosen_pid), ("dep", &dep.mod_id)],
                    )));
                }
                queue.push_back((
                    chosen_project.id.clone(),
                    true,
                    project.name.clone(),
                    Some(dep.version.clone()),
                ));
                continue;
            }

            // Find all non-revoked projects providing `dep.mod_id` with a fitting release for `build`.
            let mut candidates: Vec<ProviderCandidate> = Vec::new();
            let mut saw_unknown = false;
            for candidate in index.projects_for_mod(&dep.mod_id) {
                if !matches!(candidate.kind, V2Kind::Mod | V2Kind::NativeMod) {
                    continue;
                }
                if index.revoked(&candidate.id, None).is_some() {
                    continue;
                }
                let best = candidate
                    .releases
                    .iter()
                    .filter(|r| {
                        r.fit_verdict(build) == FitVerdict::Fits
                            && in_range(Some(&dep.version), &r.version)
                            && index.revoked(&candidate.id, Some(&r.file.sha256)).is_none()
                            && !unfit_hashes.contains(&r.file.sha256.to_ascii_lowercase())
                    })
                    .max_by(|a, b| compare_versions(&a.version, &b.version));
                if let Some(release) = best {
                    candidates.push(ProviderCandidate {
                        project_id: candidate.id.clone(),
                        slug: candidate.slug.clone(),
                        name: candidate.name.clone(),
                        repository_name: candidate.repository.name.clone(),
                        version: release.version.clone(),
                    });
                } else if candidate.releases.iter().any(|r| {
                    r.fit_verdict(build) == FitVerdict::Unknown
                        && in_range(Some(&dep.version), &r.version)
                        && index.revoked(&candidate.id, Some(&r.file.sha256)).is_none()
                }) {
                    saw_unknown = true;
                }
            }
            // Deterministic order independent of index JSON array order.
            candidates.sort_by(|a, b| a.project_id.cmp(&b.project_id));
            match candidates.as_slice() {
                [] => {
                    return Err(InstallError::NotForVersion(if saw_unknown {
                        trf(
                            "{mod} needs {dep}, whose fit on Sunrise {build} is not known yet.",
                            &[
                                ("mod", &project.name),
                                ("dep", &dep.mod_id),
                                ("build", build),
                            ],
                        )
                    } else {
                        trf(
                            "{mod} needs {dep} {range}, which has no release fitting Sunrise {build}.",
                            &[
                                ("mod", &project.name),
                                ("dep", &dep.mod_id),
                                ("range", &dep.version.to_string()),
                                ("build", build),
                            ],
                        )
                    }));
                }
                [only] => {
                    queue.push_back((
                        only.project_id.clone(),
                        true,
                        project.name.clone(),
                        Some(dep.version.clone()),
                    ));
                }
                _ => {
                    let need = ProviderChoiceNeeded {
                        mod_id: dep.mod_id.clone(),
                        required_by: project.name.clone(),
                        candidates,
                    };
                    if let Some(picked) = choose_provider.and_then(|ask| ask(&need)) {
                        if need.candidates.iter().any(|c| c.project_id == picked) {
                            queue.push_back((
                                picked,
                                true,
                                project.name.clone(),
                                Some(dep.version.clone()),
                            ));
                            continue;
                        }
                    }
                    return Err(InstallError::AmbiguousProvider(Box::new(need)));
                }
            }
        }
    }

    let steps = order
        .into_iter()
        .filter_map(|slot| planned.remove(&slot))
        .collect();
    Ok(V2Plan {
        steps,
        optional_dependencies,
    })
}
