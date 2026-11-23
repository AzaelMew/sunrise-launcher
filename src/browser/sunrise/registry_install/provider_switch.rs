//! Staged provider switch for competing ports sharing an in-game mod ID.
//!
//! Reuses the managed profile edit transaction (`Profile::open_for`, `begin_edit`, `candidate`,
//! `prove`, `publish_install` / `publish_pending`) and saved-world checks so the replacement
//! port is staged privately, inspected for affected dependents and world impact, and never
//! touches the live profile if the player cancels or a check fails.
use super::*;
use crate::{attestation::PublisherChange, browser::FileRef, worlds};
use sunrise_launcher_smod::{index_v2, manifest::Package};

/// An installed mod in the profile that declares a dependency on the switched in-game mod ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSwitchDependent {
    pub id: String,
    pub name: String,
    pub required: bool,
}

/// A validated request to switch an installed mod from one reviewed v2 project to another
/// project providing the same in-game mod ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSwitchRequest {
    pub mod_id: String,
    pub mod_name: String,
    pub from_project_id: String,
    pub from_repo: String,
    pub from_version: String,
    pub to_project_id: String,
    pub to_slug: String,
    pub to_repo: String,
    pub to_repo_path: String,
    pub to_version: String,
    pub to_workflow: String,
    pub to_repo_id: u64,
    pub to_file: index_v2::File,
    pub to_kind: index_v2::Kind,
}

impl ProviderSwitchRequest {
    /// Builds a provider-switch request between two distinct v2 projects that share `mod_id`.
    pub fn from_v2(
        from: &index_v2::Project,
        from_version: &str,
        to: &index_v2::Project,
        to_release: &index_v2::Release,
    ) -> Result<Self, String> {
        if from.mod_id != to.mod_id {
            return Err(trf(
                "{from} and {to} do not provide the same in-game mod ID.",
                &[("from", &from.name), ("to", &to.name)],
            ));
        }
        if from.id == to.id {
            return Err(trf(
                "{mod} is already using {repo}.",
                &[("mod", &to.name), ("repo", &to.repository.name)],
            ));
        }
        Ok(Self {
            mod_id: to.mod_id.clone(),
            mod_name: to.name.clone(),
            from_project_id: from.id.clone(),
            from_repo: from.repository.name.clone(),
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
}

/// The confirmation question shown after staging a provider switch privately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSwitchQuestion {
    pub profile_name: String,
    pub build_label: String,
    pub mod_id: String,
    pub mod_name: String,
    pub from_repo: String,
    pub from_version: String,
    pub to_repo: String,
    pub to_version: String,
    pub dependents: Vec<ProviderSwitchDependent>,
    pub world_risks: Vec<String>,
}

/// The player's answer to a staged provider-switch prompt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProviderSwitchDecision {
    #[default]
    Cancel,
    Switch,
}

/// Outcome of a staged provider switch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderSwitched {
    /// The player cancelled or the switch was stopped before publication.
    Stayed,
    /// The replacement port was published atomically into the profile.
    Switched {
        mod_id: String,
        project_id: String,
        repo: String,
        version: String,
    },
}

/// Callbacks connecting the provider-switch worker to the UI or CLI.
pub struct ProviderSwitchHooks<'a> {
    pub cancellation: &'a native_mods::Cancellation,
    pub progress: &'a dyn Fn(InstallProgress),
    pub registry_crates: Option<&'a dyn Fn(&native_mods::Consent) -> bool>,
    pub approve: &'a dyn Fn(&ProviderSwitchQuestion) -> Result<ProviderSwitchDecision, String>,
}

/// Context for the publisher-change prompt shown when a verified release's repository ID or
/// build workflow path differs from the computer's remembered pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherChangeQuestion {
    pub mod_name: String,
    pub current_version: Option<String>,
    pub incoming_version: String,
    pub change: PublisherChange,
}

/// The player's decision on a publisher-change prompt. Defaults to keeping the current version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PublisherChangeDecision {
    #[default]
    KeepCurrent,
    AcceptNewPublisher,
}

/// Lists installed mods in `home` that depend on `mod_id`, required dependents first.
pub fn provider_switch_dependents(
    home: &Path,
    mod_id: &str,
) -> Result<Vec<ProviderSwitchDependent>, String> {
    let packages = managed_play::scan_packages(home)?;
    Ok(provider_switch_dependents_from(&packages, mod_id))
}

fn provider_switch_dependents_from(
    packages: &[ScannedPackage],
    mod_id: &str,
) -> Vec<ProviderSwitchDependent> {
    let mut out = Vec::new();
    for package in packages {
        if package.manifest.id == mod_id {
            continue;
        }
        if let Some(dep) = package.manifest.dependencies.get(mod_id) {
            let details = dep.details();
            out.push(ProviderSwitchDependent {
                id: package.manifest.id.clone(),
                name: package.manifest.name.clone(),
                required: !details.optional,
            });
        }
    }
    out.sort_by(|a, b| {
        b.required
            .cmp(&a.required)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

struct StagedProvider {
    dependents: Vec<ProviderSwitchDependent>,
    suspect: worlds::SuspectMods,
    world_evidence: Vec<worlds::WorldRisk>,
    world_risks: Vec<String>,
    needs_native_proof: bool,
}

fn v2_installed_entry(
    request: &ProviderSwitchRequest,
    relative: &str,
) -> Result<installed::Entry, String> {
    let repo_path = if request.to_repo_path.trim().is_empty() {
        "."
    } else {
        request.to_repo_path.as_str()
    };
    serde_json::from_value(serde_json::json!({
        "source": SourceId::Sunrise,
        "project": request.to_project_id,
        "mod_id": request.mod_id,
        "slug": request.to_slug,
        "title": request.mod_name,
        "kind": Kind::Mod,
        "version": request.to_version,
        "number": request.to_version,
        "file": relative,
        "checksum": format!("SHA-256:{}", request.to_file.sha256),
        "page": format!("{}/{}", super::super::SITE, request.to_slug),
        "dependency": false,
        "verdict": "Verified",
        "repository": {
            "id": request.to_repo_id,
            "name": request.to_repo,
            "path": repo_path,
            "workflow": request.to_workflow,
        },
    }))
    .map_err(|error| error.to_string())
}

fn file_name_for_release(url: &str, mod_id: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty() && name.ends_with(".smod"))
        .map_or_else(|| format!("{mod_id}.smod"), str::to_owned)
}

fn stage_provider_candidate(
    home: &Path,
    candidate: &Path,
    request: &ProviderSwitchRequest,
    fetch_archive: &dyn Fn(&index_v2::File, &Path) -> Result<PathBuf, String>,
    cancellation: &native_mods::Cancellation,
) -> Result<StagedProvider, String> {
    cancellation.check()?;
    let before = managed_play::scan_packages(candidate)?;
    let old_pkg = before
        .iter()
        .find(|pkg| pkg.manifest.id == request.mod_id)
        .ok_or_else(|| {
            trf(
                "{mod} isn't installed in this profile.",
                &[("mod", &request.mod_name)],
            )
        })?;
    if old_pkg.directory {
        return Err(trf(
            "{mod} is an unpacked folder in this profile. Move it away before switching ports.",
            &[("mod", &request.mod_name)],
        ));
    }
    let dependents = provider_switch_dependents_from(&before, &request.mod_id);
    let staging = tempfile::Builder::new()
        .prefix("provider-switch-")
        .tempdir_in(candidate)
        .map_err(|e| at(candidate, e))?;
    let fetched = fetch_archive(&request.to_file, staging.path())?;
    cancellation.check()?;

    let bytes = fs::read(&fetched).map_err(|e| at(&fetched, e))?;
    let actual_sha = sunrise_launcher_smod::index::sha256(&bytes);
    if !actual_sha.eq_ignore_ascii_case(&request.to_file.sha256) {
        return Err(trf(
            "{mod}'s download didn't match its expected checksum. Nothing was changed.",
            &[("mod", &request.mod_name)],
        ));
    }
    let package = Package::read(&fetched).map_err(|e| at(&fetched, e))?;
    if package.manifest.id != request.mod_id {
        return Err(trf(
            "{mod}'s download carries in-game ID {got}, expected {expected}. Nothing was changed.",
            &[
                ("mod", &request.mod_name),
                ("got", &package.manifest.id),
                ("expected", &request.mod_id),
            ],
        ));
    }

    let is_native = package.is_native();
    let dir = if is_native {
        native_mods::INSTALLED
    } else {
        mod_list::MODS_DIR
    };
    let relative = format!("{dir}/{}.smod", request.mod_id);
    let target = candidate.join(&relative);

    if old_pkg.path != target && old_pkg.path.exists() {
        fs::remove_file(&old_pkg.path).map_err(|e| at(&old_pkg.path, e))?;
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| at(parent, e))?;
    }
    fs::rename(&fetched, &target).map_err(|e| at(&target, e))?;

    let mut installed = installed::Installed::read(candidate)?;
    installed.entries.retain(|entry| {
        !(entry.source == SourceId::Sunrise
            && (entry.project == request.from_project_id
                || entry.project == request.to_project_id
                || super::super::mod_id(entry) == Some(request.mod_id.as_str())))
    });
    installed
        .entries
        .push(v2_installed_entry(request, &relative)?);
    let manifest_text =
        serde_json::to_string_pretty(&installed).map_err(|error| error.to_string())?;
    let manifest_path = candidate.join(installed::MANIFEST);
    fs::write(&manifest_path, manifest_text).map_err(|e| at(&manifest_path, e))?;

    let after = managed_play::scan_packages(candidate)?;
    let has_enabled_native = after.iter().any(|pkg| {
        pkg.manifest.native.is_some() && mod_list::is_enabled(candidate, &pkg.manifest.id)
    });
    let (suspect, _) = mutations::update_worlds(candidate, std::iter::once(old_pkg))?;
    let world_evidence = mutations::world_evidence(home, &suspect)?;
    let world_risks = world_evidence
        .iter()
        .filter(|risk| !risk.mods.is_empty())
        .map(|risk| {
            trf(
                "{world}: content from {mod} may be affected.",
                &[("world", &risk.name), ("mod", &request.mod_name)],
            )
        })
        .collect();

    Ok(StagedProvider {
        dependents,
        suspect,
        world_evidence,
        world_risks,
        needs_native_proof: is_native || has_enabled_native,
    })
}

/// Stages switching an installed mod to another port providing the same in-game ID, asks the
/// player to confirm with the exact list of affected dependents, and publishes atomically only
/// on confirmation. Cancelling leaves the current port completely intact.
pub fn switch_provider(
    instance: &Instance,
    identity: Identity,
    build_label: &str,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    request: &ProviderSwitchRequest,
    hooks: ProviderSwitchHooks<'_>,
) -> Result<ProviderSwitched, String> {
    if instance.kind == InstanceKind::Pack {
        return Err(tr("Choose an ordinary profile to switch ports.").into());
    }
    if request.from_project_id == request.to_project_id {
        return Err(trf(
            "{mod} is already using {repo}.",
            &[("mod", &request.mod_name), ("repo", &request.to_repo)],
        ));
    }
    (hooks.progress)(InstallProgress::Checking);
    hooks.cancellation.check()?;

    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let edit = profile.begin_edit(choose, |f| {
        (hooks.progress)(InstallProgress::Downloading(f));
    })?;
    hooks.cancellation.check()?;

    let candidate = edit.candidate()?;
    let local = !super::super::v2::location().starts_with("https://");
    let fetch_archive = |file: &index_v2::File, folder: &Path| {
        let file_ref = FileRef {
            url: file.url.clone(),
            name: file_name_for_release(&file.url, &request.mod_id),
            size: file.size,
            sha512: None,
            sha256: Some(file.sha256.clone()),
            sha1: None,
            primary: true,
        };
        fetch_cancellable(&file_ref, folder, local, Some(hooks.cancellation), &|f| {
            (hooks.progress)(InstallProgress::Downloading(f));
        })
    };

    let StagedProvider {
        dependents,
        suspect,
        world_evidence,
        world_risks,
        needs_native_proof,
    } = stage_provider_candidate(
        &instance.home,
        candidate.path(),
        request,
        &fetch_archive,
        hooks.cancellation,
    )?;

    let proof = if needs_native_proof {
        (hooks.progress)(InstallProgress::Preparing);
        let proved = edit
            .prove(
                candidate.path(),
                instance.native_profile,
                hooks.cancellation,
                |step| super::build_progress(step, hooks.progress),
                hooks.registry_crates,
                None,
            )
            .map_err(String::from)?;
        Some(proved)
    } else {
        None
    };

    let committed_inputs = match &proof {
        Some(proved) => proved.inputs(),
        None => managed_play::inputs(candidate.path(), None)?,
    };

    let question = ProviderSwitchQuestion {
        profile_name: instance.name.clone(),
        build_label: build_label.to_owned(),
        mod_id: request.mod_id.clone(),
        mod_name: request.mod_name.clone(),
        from_repo: request.from_repo.clone(),
        from_version: request.from_version.clone(),
        to_repo: request.to_repo.clone(),
        to_version: request.to_version.clone(),
        dependents,
        world_risks,
    };
    (hooks.progress)(InstallProgress::Reviewing);
    if hooks.cancellation.is_stopped() {
        return Ok(ProviderSwitched::Stayed);
    }
    match (hooks.approve)(&question)? {
        ProviderSwitchDecision::Cancel => return Ok(ProviderSwitched::Stayed),
        ProviderSwitchDecision::Switch => (),
    }
    if hooks.cancellation.is_stopped() {
        return Ok(ProviderSwitched::Stayed);
    }

    (hooks.progress)(InstallProgress::Publishing);
    let authorize = || mutations::bind_worlds(&instance.home, &suspect, &world_evidence);
    let retired = BTreeSet::new();
    if let Some(proved) = proof {
        edit.publish_install(proved, hooks.cancellation, &retired, authorize)?;
    } else {
        edit.publish_pending(
            candidate.path(),
            committed_inputs,
            hooks.cancellation,
            authorize,
        )?;
    }
    Ok(ProviderSwitched::Switched {
        mod_id: request.mod_id.clone(),
        project_id: request.to_project_id.clone(),
        repo: request.to_repo.clone(),
        version: request.to_version.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_smod(dir: &Path, file_name: &str, manifest: &str) -> (PathBuf, Vec<u8>, String) {
        fs::create_dir_all(dir).unwrap();
        let bytes = Package::write_files(manifest, &[]).unwrap();
        let sha = sunrise_launcher_smod::index::sha256(&bytes);
        let path = dir.join(file_name);
        fs::write(&path, &bytes).unwrap();
        (path, bytes, sha)
    }

    fn sample_v2_project(
        id: &str,
        slug: &str,
        mod_id: &str,
        repo: &str,
        repo_id: u64,
        sha: &str,
    ) -> index_v2::Project {
        index_v2::Project {
            id: id.into(),
            slug: slug.into(),
            mod_id: mod_id.into(),
            kind: index_v2::Kind::Mod,
            name: "Jade".into(),
            summary: "Shows what you are looking at.".into(),
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
                name: repo.into(),
                path: ".".into(),
            },
            workflow: "sunrise-org/sunrise-mods/.github/workflows/release.yml".into(),
            releases: vec![index_v2::Release {
                version: "1.4.2".into(),
                published: "2026-11-22".into(),
                sunrise: "26.3-0.1.0".into(),
                workflow:
                    "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1"
                        .into(),
                fits: vec!["26.3-0.1.0".into()],
                does_not_fit: vec![],
                file: index_v2::File {
                    url: "https://example.invalid/jade-1.4.2.smod".into(),
                    size: 128,
                    sha256: sha.into(),
                    attestation: "https://example.invalid/jade-1.4.2.sigstore.json".into(),
                },
                dependencies: vec![],
            }],
        }
    }

    fn run_staged_switch(
        home: &Path,
        request: &ProviderSwitchRequest,
        archive_bytes: &[u8],
        decision: ProviderSwitchDecision,
    ) -> Result<(ProviderSwitched, ProviderSwitchQuestion), String> {
        let candidate = tempfile::tempdir_in(home).unwrap();
        crate::managed_play::copy_install_inputs(home, candidate.path())?;
        let cancellation = native_mods::Cancellation::default();
        let bytes = archive_bytes.to_vec();
        let staged = stage_provider_candidate(
            home,
            candidate.path(),
            request,
            &|_, folder| {
                let path = folder.join("replacement.smod");
                fs::write(&path, &bytes).map_err(|e| at(&path, e))?;
                Ok(path)
            },
            &cancellation,
        )?;
        let question = ProviderSwitchQuestion {
            profile_name: "Sunrise".into(),
            build_label: "Stable · 0.1.0".into(),
            mod_id: request.mod_id.clone(),
            mod_name: request.mod_name.clone(),
            from_repo: request.from_repo.clone(),
            from_version: request.from_version.clone(),
            to_repo: request.to_repo.clone(),
            to_version: request.to_version.clone(),
            dependents: staged.dependents,
            world_risks: staged.world_risks,
        };
        if decision == ProviderSwitchDecision::Cancel {
            return Ok((ProviderSwitched::Stayed, question));
        }
        // Apply staged files back to home atomically for the offline test harness.
        let src_smod =
            candidate
                .path()
                .join(format!("{}/{}.smod", mod_list::MODS_DIR, request.mod_id));
        let dst_smod = home.join(format!("{}/{}.smod", mod_list::MODS_DIR, request.mod_id));
        fs::copy(&src_smod, &dst_smod).map_err(|e| at(&dst_smod, e))?;
        let src_manifest = candidate.path().join(installed::MANIFEST);
        let dst_manifest = home.join(installed::MANIFEST);
        fs::copy(&src_manifest, &dst_manifest).map_err(|e| at(&dst_manifest, e))?;
        Ok((
            ProviderSwitched::Switched {
                mod_id: request.mod_id.clone(),
                project_id: request.to_project_id.clone(),
                repo: request.to_repo.clone(),
                version: request.to_version.clone(),
            },
            question,
        ))
    }

    fn test_installed_entry(
        project: &str,
        slug: &str,
        version: &str,
        sha: &str,
    ) -> installed::Entry {
        serde_json::from_value(serde_json::json!({
            "source": SourceId::Sunrise,
            "project": project,
            "mod_id": "jade",
            "slug": slug,
            "title": "Jade",
            "kind": Kind::Mod,
            "version": version,
            "number": version,
            "file": "sunrise-mods/jade.smod",
            "checksum": format!("SHA-256:{sha}"),
            "page": format!("https://playsunrise.net/mods/{slug}"),
            "dependency": false,
            "verdict": "Verified",
        }))
        .unwrap()
    }

    #[test]
    fn provider_switch_dependents_lists_required_before_optional_and_skips_unrelated() {
        let home = tempfile::tempdir().unwrap();
        let mods_dir = home.path().join(mod_list::MODS_DIR);
        write_smod(
            &mods_dir,
            "jade.smod",
            "id='jade'\nname='Jade'\nversion='26.3.8'\napi='0.2'\n",
        );
        write_smod(
            &mods_dir,
            "jei.smod",
            "id='jei'\nname='JEI'\nversion='1.0.0'\napi='0.2'\n[dependencies]\njade = { version = '^1', optional = true }\n",
        );
        write_smod(
            &mods_dir,
            "iron_furnaces.smod",
            "id='iron_furnaces'\nname='Iron Furnaces'\nversion='2.0.0'\napi='0.2'\n[dependencies]\njade = '^1'\n",
        );
        write_smod(
            &mods_dir,
            "unrelated.smod",
            "id='unrelated'\nname='Unrelated'\nversion='1.0.0'\napi='0.2'\n",
        );

        let deps = provider_switch_dependents(home.path(), "jade").unwrap();
        assert_eq!(
            deps,
            vec![
                ProviderSwitchDependent {
                    id: "iron_furnaces".into(),
                    name: "Iron Furnaces".into(),
                    required: true,
                },
                ProviderSwitchDependent {
                    id: "jei".into(),
                    name: "JEI".into(),
                    required: false,
                },
            ]
        );
    }

    #[test]
    fn provider_switch_request_rejects_mismatched_mod_id_or_same_project() {
        let a = sample_v2_project(
            "aaaa1111",
            "sunrise-jade",
            "jade",
            "joeypc6/sunrise-jade",
            10,
            &"b".repeat(64),
        );
        let b = sample_v2_project(
            "bbbb2222",
            "jade-sunrise",
            "jade",
            "zekrenlight/jade-sunrise",
            20,
            &"c".repeat(64),
        );
        let other = sample_v2_project(
            "cccc3333",
            "other-mod",
            "other",
            "other/mod",
            30,
            &"d".repeat(64),
        );

        assert!(ProviderSwitchRequest::from_v2(&a, "26.3.8", &a, &a.releases[0]).is_err());
        assert!(ProviderSwitchRequest::from_v2(&a, "26.3.8", &other, &other.releases[0]).is_err());
        let ok = ProviderSwitchRequest::from_v2(&a, "26.3.8", &b, &b.releases[0]).unwrap();
        assert_eq!(ok.mod_id, "jade");
        assert_eq!(ok.from_project_id, "aaaa1111");
        assert_eq!(ok.to_project_id, "bbbb2222");
        assert_eq!(ok.to_repo, "zekrenlight/jade-sunrise");
    }

    #[test]
    fn staged_provider_switch_cancel_leaves_profile_untouched() {
        let home = tempfile::tempdir().unwrap();
        let mods_dir = home.path().join(mod_list::MODS_DIR);
        let (old_path, old_bytes, old_sha) = write_smod(
            &mods_dir,
            "jade.smod",
            "id='jade'\nname='Jade'\nversion='26.3.8'\napi='0.2'\n",
        );
        write_smod(
            &mods_dir,
            "iron_furnaces.smod",
            "id='iron_furnaces'\nname='Iron Furnaces'\nversion='2.0.0'\napi='0.2'\n[dependencies]\njade = '^1'\n",
        );
        mod_list::set_enabled(home.path(), "jade", true).unwrap();
        let initial_installed = installed::Installed {
            entries: vec![test_installed_entry(
                "aaaa1111",
                "sunrise-jade",
                "26.3.8",
                &old_sha,
            )],
        };
        initial_installed.save(home.path()).unwrap();

        let new_bytes =
            Package::write_files("id='jade'\nname='Jade'\nversion='1.4.2'\napi='0.2'\n", &[])
                .unwrap();
        let new_sha = sunrise_launcher_smod::index::sha256(&new_bytes);
        let from = sample_v2_project(
            "aaaa1111",
            "sunrise-jade",
            "jade",
            "joeypc6/sunrise-jade",
            10,
            &old_sha,
        );
        let to = sample_v2_project(
            "bbbb2222",
            "jade-sunrise",
            "jade",
            "zekrenlight/jade-sunrise",
            20,
            &new_sha,
        );
        let request =
            ProviderSwitchRequest::from_v2(&from, "26.3.8", &to, &to.releases[0]).unwrap();

        let (outcome, question) = run_staged_switch(
            home.path(),
            &request,
            &new_bytes,
            ProviderSwitchDecision::Cancel,
        )
        .unwrap();
        assert_eq!(outcome, ProviderSwitched::Stayed);
        assert_eq!(question.dependents.len(), 1);
        assert_eq!(question.dependents[0].id, "iron_furnaces");
        assert!(question.dependents[0].required);

        // Live profile must remain completely unchanged after Cancel.
        assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
        let after = installed::Installed::read(home.path()).unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].project, "aaaa1111");
        assert_eq!(after.entries[0].version, "26.3.8");
        assert!(mod_list::is_enabled(home.path(), "jade"));
    }

    #[test]
    fn staged_provider_switch_confirm_replaces_provider_and_preserves_enabled() {
        let home = tempfile::tempdir().unwrap();
        let mods_dir = home.path().join(mod_list::MODS_DIR);
        let (old_path, _, old_sha) = write_smod(
            &mods_dir,
            "jade.smod",
            "id='jade'\nname='Jade'\nversion='26.3.8'\napi='0.2'\n",
        );
        mod_list::set_enabled(home.path(), "jade", true).unwrap();
        let initial_installed = installed::Installed {
            entries: vec![test_installed_entry(
                "aaaa1111",
                "sunrise-jade",
                "26.3.8",
                &old_sha,
            )],
        };
        initial_installed.save(home.path()).unwrap();

        let new_bytes =
            Package::write_files("id='jade'\nname='Jade'\nversion='1.4.2'\napi='0.2'\n", &[])
                .unwrap();
        let new_sha = sunrise_launcher_smod::index::sha256(&new_bytes);
        let from = sample_v2_project(
            "aaaa1111",
            "sunrise-jade",
            "jade",
            "joeypc6/sunrise-jade",
            10,
            &old_sha,
        );
        let to = sample_v2_project(
            "bbbb2222",
            "jade-sunrise",
            "jade",
            "zekrenlight/jade-sunrise",
            20,
            &new_sha,
        );
        let request =
            ProviderSwitchRequest::from_v2(&from, "26.3.8", &to, &to.releases[0]).unwrap();

        let (outcome, _) = run_staged_switch(
            home.path(),
            &request,
            &new_bytes,
            ProviderSwitchDecision::Switch,
        )
        .unwrap();
        assert_eq!(
            outcome,
            ProviderSwitched::Switched {
                mod_id: "jade".into(),
                project_id: "bbbb2222".into(),
                repo: "zekrenlight/jade-sunrise".into(),
                version: "1.4.2".into(),
            }
        );
        assert_eq!(fs::read(&old_path).unwrap(), new_bytes);
        let after = installed::Installed::read(home.path()).unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].project, "bbbb2222");
        assert_eq!(after.entries[0].slug, "jade-sunrise");
        assert_eq!(after.entries[0].version, "1.4.2");
        assert!(mod_list::is_enabled(home.path(), "jade"));
    }

    #[test]
    fn staged_provider_switch_rejects_archive_with_wrong_mod_id() {
        let home = tempfile::tempdir().unwrap();
        let mods_dir = home.path().join(mod_list::MODS_DIR);
        let (_, _, old_sha) = write_smod(
            &mods_dir,
            "jade.smod",
            "id='jade'\nname='Jade'\nversion='26.3.8'\napi='0.2'\n",
        );
        let wrong_bytes = Package::write_files(
            "id='something_else'\nname='Wrong'\nversion='1.4.2'\napi='0.2'\n",
            &[],
        )
        .unwrap();
        let wrong_sha = sunrise_launcher_smod::index::sha256(&wrong_bytes);
        let from = sample_v2_project(
            "aaaa1111",
            "sunrise-jade",
            "jade",
            "joeypc6/sunrise-jade",
            10,
            &old_sha,
        );
        let to = sample_v2_project(
            "bbbb2222",
            "jade-sunrise",
            "jade",
            "zekrenlight/jade-sunrise",
            20,
            &wrong_sha,
        );
        let request =
            ProviderSwitchRequest::from_v2(&from, "26.3.8", &to, &to.releases[0]).unwrap();

        let err = run_staged_switch(
            home.path(),
            &request,
            &wrong_bytes,
            ProviderSwitchDecision::Switch,
        )
        .unwrap_err();
        assert!(err.contains("something_else"), "{err}");
        assert!(err.contains("jade"), "{err}");
    }
}
