//! Planning an install or update: which release of each project, its dependencies, and why a release can't be used.

use super::*;

/// Read the player's pinned version for a project (keyed by permanent project ID, falling back to
/// legacy v1 in-game mod ID from `mod-browser/chosen-versions.json`).
pub fn chosen_version_pin<'a>(
    pins: &'a BTreeMap<String, String>,
    project_id: &str,
    mod_id: &str,
) -> Option<&'a str> {
    pins.get(project_id)
        .or_else(|| pins.get(mod_id))
        .map(String::as_str)
}

/// Deterministic version comparison (semver when parseable, falling back to string order).
pub(crate) fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_version(a), parse_version(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb).then_with(|| a.cmp(b)),
        _ => a.cmp(b),
    }
}

/// A project the planner still has to place: its ID, whether it is only a dependency, the mod
/// that asked for it, and the version range that mod requires of it.
pub(super) type Queued = (String, bool, String, Option<VersionReq>);

/// Whether `version` is inside `range`; a version that isn't semver is in no range.
pub(super) fn in_range(range: Option<&VersionReq>, version: &str) -> bool {
    range.is_none_or(|range| parse_version(version).is_ok_and(|v| range.matches(&v)))
}

/// The jars `release` lists that Modrinth no longer serves and `jar_cache` doesn't hold. A jar
/// that is cached still installs, so it is not named.
fn unavailable_jars<'a>(release: &'a Release, jar_cache: Option<&Path>) -> Vec<&'a JarRef> {
    release
        .jar_unavailable
        .iter()
        .filter(|jar| !jar_cache.is_some_and(|cache| jar.cache_path(cache).is_file()))
        .collect()
}

/// Refuses a release for `jars`, one line per jar.
fn jars_gone(jars: &[&JarRef]) -> InstallError {
    InstallError::Refused(
        jars.iter()
            .map(|jar| {
                trf(
                    "The original file {id} {version} is no longer on Modrinth.",
                    &[("id", jar.id()), ("version", jar.version())],
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub(super) fn select_project_release<'a>(
    index: &Index,
    project: &'a Project,
    build: &str,
    pinned_version: Option<&str>,
    range: Option<&VersionReq>,
    unfit_hashes: &BTreeSet<String>,
    jar_cache: Option<&Path>,
) -> Result<&'a Release, InstallError> {
    if let Some(revoked) = index.revoked(&project.id, None) {
        return Err(InstallError::Refused(trf(
            "{mod} was taken down: {reason}",
            &[("mod", &project.name), ("reason", &revoked.reason)],
        )));
    }
    if let Some(pinned) = pinned_version {
        let Some(release) = project.releases.iter().find(|r| r.version == pinned) else {
            return Err(InstallError::NotForVersion(trf(
                "Pinned version {version} of {mod} is not in the Sunrise registry.",
                &[("version", pinned), ("mod", &project.name)],
            )));
        };
        if let Some(range) = range.filter(|_| !in_range(range, pinned)) {
            return Err(InstallError::NotForVersion(trf(
                "Pinned version {version} of {mod} is outside {range}, which another mod needs.",
                &[
                    ("version", pinned),
                    ("mod", &project.name),
                    ("range", &range.to_string()),
                ],
            )));
        }
        if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
            return Err(InstallError::Refused(trf(
                "{mod} {version} was taken down: {reason}",
                &[
                    ("mod", &project.name),
                    ("version", pinned),
                    ("reason", &revoked.reason),
                ],
            )));
        }
        if unfit_hashes.contains(&release.file.sha256.to_ascii_lowercase()) {
            return Err(InstallError::NotForVersion(trf(
                "{mod} {version} does not fit Sunrise {build}.",
                &[
                    ("mod", &project.name),
                    ("version", pinned),
                    ("build", build),
                ],
            )));
        }
        return match release.fit_verdict(build) {
            FitVerdict::Fits => match unavailable_jars(release, jar_cache).as_slice() {
                [] => Ok(release),
                gone => Err(jars_gone(gone)),
            },
            FitVerdict::DoesNotFit => Err(InstallError::NotForVersion(trf(
                "{mod} {version} does not fit Sunrise {build}.",
                &[
                    ("mod", &project.name),
                    ("version", pinned),
                    ("build", build),
                ],
            ))),
            FitVerdict::Unknown => Err(InstallError::NotForVersion(trf(
                "Fit for {mod} {version} on Sunrise {build} is not known yet.",
                &[
                    ("mod", &project.name),
                    ("version", pinned),
                    ("build", build),
                ],
            ))),
        };
    }

    let mut fitting: Vec<&Release> = Vec::new();
    let mut saw_unknown = false;
    let mut saw_does_not_fit = false;
    let mut saw_revoked = false;
    let mut saw_out_of_range = false;
    // The newest fitting release whose jar Modrinth no longer serves, with the jars missing.
    let mut newest_gone: Option<(&str, Vec<&JarRef>)> = None;
    for release in &project.releases {
        if index
            .revoked(&project.id, Some(&release.file.sha256))
            .is_some()
        {
            saw_revoked = true;
            continue;
        }
        if !in_range(range, &release.version) {
            saw_out_of_range = true;
            continue;
        }
        if unfit_hashes.contains(&release.file.sha256.to_ascii_lowercase()) {
            saw_does_not_fit = true;
            continue;
        }
        match release.fit_verdict(build) {
            FitVerdict::Fits => {
                let gone = unavailable_jars(release, jar_cache);
                if gone.is_empty() {
                    fitting.push(release);
                } else if newest_gone.as_ref().is_none_or(|(version, _)| {
                    compare_versions(&release.version, version) == std::cmp::Ordering::Greater
                }) {
                    newest_gone = Some((release.version.as_str(), gone));
                }
            }
            FitVerdict::DoesNotFit => saw_does_not_fit = true,
            FitVerdict::Unknown => saw_unknown = true,
        }
    }
    if let Some(best) = fitting
        .into_iter()
        .max_by(|a, b| compare_versions(&a.version, &b.version))
    {
        return Ok(best);
    }
    if let Some(range) = range.filter(|_| saw_out_of_range) {
        return Err(InstallError::NotForVersion(trf(
            "No release of {mod} in {range} fits Sunrise {build}.",
            &[
                ("mod", &project.name),
                ("range", &range.to_string()),
                ("build", build),
            ],
        )));
    }
    if let Some((_, gone)) = newest_gone {
        Err(jars_gone(&gone))
    } else if saw_unknown {
        Err(InstallError::NotForVersion(trf(
            "Fit for {mod} on Sunrise {build} is not known yet.",
            &[("mod", &project.name), ("build", build)],
        )))
    } else if !saw_does_not_fit && saw_revoked {
        Err(InstallError::Refused(trf(
            "All releases of {mod} were taken down.",
            &[("mod", &project.name)],
        )))
    } else {
        Err(InstallError::NotForVersion(trf(
            "{mod} does not fit Sunrise {build}.",
            &[("mod", &project.name), ("build", build)],
        )))
    }
}

pub(super) fn planned_from(
    project: &Project,
    release: &Release,
    dependency: bool,
) -> PlannedV2Release {
    PlannedV2Release {
        project_id: project.id.clone(),
        slug: project.slug.clone(),
        mod_id: project.mod_id.clone(),
        name: project.name.clone(),
        kind: project.kind.clone(),
        version: release.version.clone(),
        sha256: release.file.sha256.clone(),
        size: release.file.size,
        url: release.file.url.clone(),
        attestation_url: release.file.attestation.clone(),
        workflow: release.workflow.clone(),
        repository_id: project.release_repository(release),
        repository_name: project.repository.name.clone(),
        repository_path: project.repository.path.clone(),
        dependency,
    }
}

/// Plan a schema-2 install of `requested_projects` (permanent 8-character project IDs) for exact
/// Sunrise build `build`.
///
/// - Uses only authoritative `release.fits` / `release.does_not_fit` verdicts (`FitVerdict::Fits`).
/// - Honors explicit version overrides and `chosen-versions.json` pins.
/// - Auto-installs ONLY required dependencies (`dep.required == true`); optional dependencies are
///   returned for display without being added to `steps`.
/// - When a required dependency has several fitting providers and no provider is already installed
///   or supplied, asks `choose_provider` or returns [`InstallError::AmbiguousProvider`] — never
///   silently picking one.
/// - Refuses with [`InstallError::ProviderConflict`] rather than silently substituting a different
///   provider in an occupied slot.
pub fn plan_v2_install(
    index: &Index,
    build: &str,
    requested_projects: &[String],
    installed: &BTreeMap<String, InstalledV2Mod>,
    chosen_pins: &BTreeMap<String, String>,
    options: &V2InstallOptions,
    choose_provider: Option<&ChooseProvider<'_>>,
) -> Result<V2Plan, InstallError> {
    if requested_projects.is_empty() {
        return Err(InstallError::Cancelled);
    }
    let unfit_hashes = known_unfit_updates(build);
    let mut seen_slots: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: VecDeque<Queued> = VecDeque::new();
    for project_id in requested_projects {
        let project = index.project(project_id).ok_or_else(|| {
            InstallError::Refused(trf(
                "Project {project} is not in the Sunrise registry.",
                &[("project", project_id)],
            ))
        })?;
        if !matches!(project.kind, V2Kind::Mod | V2Kind::NativeMod) {
            return Err(InstallError::Refused(trf(
                "{mod} is not a mod package.",
                &[("mod", &project.name)],
            )));
        }
        if let Some(other_id) = seen_slots.insert(project.mod_id.clone(), project.id.clone()) {
            if other_id != project.id {
                let other_name = index
                    .project(&other_id)
                    .map_or(other_id.as_str(), |p| p.name.as_str());
                return Err(InstallError::ProviderConflict(Box::new(ProviderConflict {
                    mod_id: project.mod_id.clone(),
                    installed_project: Some(other_id.clone()),
                    installed_name: other_name.to_owned(),
                    requested_project: project.id.clone(),
                    requested_name: project.name.clone(),
                })));
            }
        }
        queue.push_back((project.id.clone(), false, project.name.clone(), None));
    }
    resolve_v2_queue(
        &ResolveContext {
            index,
            build,
            installed,
            chosen_pins,
            options,
            unfit_hashes: &unfit_hashes,
            choose_provider,
        },
        queue,
    )
}

/// Plan a schema-2 update for `request` (Strict or Bulk). Accepts either permanent project IDs or
/// in-game mod IDs, ALWAYS keeps each installed mod's chosen project, honors `chosen-versions.json`
/// pins on bulk updates, and resolves any new required dependencies using the same non-silent rules.
pub fn plan_v2_update(
    index: &Index,
    build: &str,
    request: &UpdateRequest,
    installed: &BTreeMap<String, InstalledV2Mod>,
    chosen_pins: &BTreeMap<String, String>,
    options: &V2InstallOptions,
    choose_provider: Option<&ChooseProvider<'_>>,
) -> Result<V2Plan, InstallError> {
    let (requested, strict) = match request {
        UpdateRequest::Strict(ids) => (ids.as_slice(), true),
        UpdateRequest::Bulk(ids) => (ids.as_slice(), false),
    };
    let unfit_hashes = known_unfit_updates(build);
    let jar_cache = options.jar_cache.as_deref();
    let mut queue: VecDeque<Queued> = VecDeque::new();
    let mut effective_options = options.clone();

    for key in requested {
        let found = installed.get(key).or_else(|| {
            installed
                .values()
                .find(|m| m.ownership.project_id() == Some(key.as_str()))
        });
        let Some(current) = found else {
            if strict {
                return Err(InstallError::Refused(trf(
                    "{mod} is not installed.",
                    &[("mod", key)],
                )));
            }
            continue;
        };
        let Some(project_id) = current.ownership.project_id() else {
            if strict {
                return Err(InstallError::Refused(trf(
                    "Sunrise couldn't confirm that {mod} came from the Sunrise registry, so it can't be updated here.",
                    &[("mod", &current.package.manifest.name)],
                )));
            }
            continue;
        };
        let Some(project) = index.project(project_id) else {
            if strict {
                return Err(InstallError::Refused(trf(
                    "{mod} is no longer listed in the Sunrise registry.",
                    &[("mod", &current.package.manifest.name)],
                )));
            }
            continue;
        };
        if let Some(revoked) = index.revoked(&project.id, None) {
            if strict {
                return Err(InstallError::Refused(trf(
                    "{mod} was taken down: {reason}",
                    &[("mod", &project.name), ("reason", &revoked.reason)],
                )));
            }
            continue;
        }
        // Respect chosen-versions.json pins during Update all (bulk).
        let pin = chosen_version_pin(chosen_pins, &project.id, &project.mod_id);
        if !strict && pin.is_some() && !effective_options.versions.contains_key(&project.id) {
            continue;
        }
        let target_version = effective_options
            .versions
            .get(&project.id)
            .map(String::as_str);
        if let Some(explicit) = target_version {
            let release = select_project_release(
                index,
                project,
                build,
                Some(explicit),
                None,
                &unfit_hashes,
                jar_cache,
            )?;
            if release.version != current.package.manifest.version
                || !release.file.sha256.eq_ignore_ascii_case(&current.digest)
            {
                queue.push_back((project.id.clone(), false, project.name.clone(), None));
            }
            continue;
        }
        let current_ver = &current.package.manifest.version;
        let newer = project
            .releases
            .iter()
            .filter(|release| {
                release.fit_verdict(build) == FitVerdict::Fits
                    && index
                        .revoked(&project.id, Some(&release.file.sha256))
                        .is_none()
                    && !unfit_hashes.contains(&release.file.sha256.to_ascii_lowercase())
                    && unavailable_jars(release, jar_cache).is_empty()
                    && compare_versions(&release.version, current_ver)
                        == std::cmp::Ordering::Greater
            })
            .max_by(|a, b| compare_versions(&a.version, &b.version));
        match newer {
            Some(release) => {
                effective_options
                    .versions
                    .insert(project.id.clone(), release.version.clone());
                queue.push_back((project.id.clone(), false, project.name.clone(), None));
            }
            None if strict => {
                // Report why no update could be planned (jar gone vs unknown vs does-not-fit vs
                // up-to-date).
                let newer_gone = project
                    .releases
                    .iter()
                    .filter(|r| {
                        compare_versions(&r.version, current_ver) == std::cmp::Ordering::Greater
                            && r.fit_verdict(build) == FitVerdict::Fits
                            && index.revoked(&project.id, Some(&r.file.sha256)).is_none()
                            && !unavailable_jars(r, jar_cache).is_empty()
                    })
                    .max_by(|a, b| compare_versions(&a.version, &b.version));
                if let Some(release) = newer_gone {
                    return Err(jars_gone(&unavailable_jars(release, jar_cache)));
                }
                let has_newer_unknown = project.releases.iter().any(|r| {
                    compare_versions(&r.version, current_ver) == std::cmp::Ordering::Greater
                        && r.fit_verdict(build) == FitVerdict::Unknown
                        && index.revoked(&project.id, Some(&r.file.sha256)).is_none()
                });
                let has_newer_no_fit = project.releases.iter().any(|r| {
                    compare_versions(&r.version, current_ver) == std::cmp::Ordering::Greater
                        && r.fit_verdict(build) == FitVerdict::DoesNotFit
                        && index.revoked(&project.id, Some(&r.file.sha256)).is_none()
                });
                if has_newer_unknown {
                    return Err(InstallError::NotForVersion(trf(
                        "Fit for newer {mod} on Sunrise {build} is not known yet.",
                        &[("mod", &project.name), ("build", build)],
                    )));
                } else if has_newer_no_fit {
                    return Err(InstallError::NotForVersion(trf(
                        "Newer {mod} does not fit Sunrise {build}.",
                        &[("mod", &project.name), ("build", build)],
                    )));
                }
            }
            None => {}
        }
    }

    if queue.is_empty() {
        return Ok(V2Plan {
            steps: Vec::new(),
            optional_dependencies: Vec::new(),
        });
    }
    resolve_v2_queue(
        &ResolveContext {
            index,
            build,
            installed,
            chosen_pins,
            options: &effective_options,
            unfit_hashes: &unfit_hashes,
            choose_provider,
        },
        queue,
    )
}
