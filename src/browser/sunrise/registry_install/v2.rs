//! Schema-2 signed registry planning, verification, staging, installation, updates, and explicit
//! provider switching.
//!
//! Every v2 operation keys mods by permanent 8-character project ID, treats `project.mod_id` as an
//! in-game slot with at most one enabled provider per profile, relies solely on authoritative
//! `release.fits` / `release.does_not_fit` verdicts against the exact Sunrise build ID, verifies
//! every archive's SHA-256 and offline Sigstore attestation against the repository the release
//! was signed from and `release.workflow`, pins the publisher's owner and workflow per computer,
//! and refuses to silently pick among competing providers or silently substitute a different
//! provider.

use super::{
    at, guard_replacements, held_change_refusal, known_unfit_updates, manifest_conflict,
    missing_required, mutations, prove_edit, remember_unfit_sha256, ChangeImpact, Document,
    InstallError, InstallHooks, InstallProgress, InstalledBatch, UpdateRequest,
};
use crate::{
    attestation::{self, ExpectedRelease, PublisherChange, PublisherPin, VerifiedRelease},
    backend,
    browser::{
        download, installed, mod_list,
        sunrise::{chosen, v2 as catalogue_v2, SITE},
        Kind, SourceId,
    },
    i18n::{tr, trf},
    instances::{Instance, Kind as InstanceKind},
    jars,
    managed_play::{self, FirstBuild, Profile, ScannedPackage},
    native_mods,
    profile_txn::Identity,
};
use semver::VersionReq;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher_smod::{
    dependencies::parse_version,
    index::{self, MAX_FILE},
    index_v2::{self, FitVerdict, Index, Kind as V2Kind, Project, Release, Revoked},
    manifest::{collect_jar_requirements, JarRef, Side},
};

mod switch_v2;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
pub(crate) use switch_v2::switch_profile_build_v2_with;
pub use switch_v2::{
    checked_switch_fit_v2, switch_build_v2, switch_profile_build_v2, V2SwitchHooks,
};
mod ownership;
mod plan;
mod queue;
mod types;
pub use ownership::*;
pub use plan::*;
use queue::*;
pub use types::*;

/// Maximum accepted `.sigstore.json` attestation bundle size (1 MiB).
const MAX_ATTESTATION_BYTES: u64 = 1 << 20;

/// Default production HTTPS byte fetcher for v2 release `.smod` archives and `.sigstore.json`
/// attestation bundles.
pub(crate) fn fetch_release_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    if !download::github_release_asset(url) {
        return Err(format!(
            "{url} is not a GitHub release asset URL allowed by the Sunrise v2 registry."
        ));
    }
    let agent = download::agent(url)?;
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("download failed for {url}: {e}"))?;
    let mut limited = response.into_reader().take(max_bytes.saturating_add(1));
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading {url}: {e}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "{url} exceeded maximum allowed size of {max_bytes} bytes"
        ));
    }
    Ok(bytes)
}

/// Download and verification callbacks for staging schema-2 registry releases.
#[derive(Clone, Copy)]
pub(crate) struct V2Transfer<'a> {
    pub(crate) fetch_bytes: BytesFetcher<'a>,
    pub(crate) verify_release: ReleaseVerifier<'a>,
}

/// Shared inputs for schema-2 registry install, update, and provider-switch edits.
pub(crate) struct V2EditRequest<'a> {
    pub(crate) instance: &'a Instance,
    pub(crate) identity: Identity,
    pub(crate) index: Arc<Index>,
    pub(crate) options: V2InstallOptions,
    pub(crate) transfer: V2Transfer<'a>,
}

pub(crate) fn stage_and_verify_v2_release(
    index: &Index,
    home: &Path,
    step: &PlannedV2Release,
    pin_dir: &Path,
    accepted_publisher_changes: &BTreeSet<String>,
    confirm_publisher_change: Option<ConfirmPublisherChange<'_>>,
    transfer: V2Transfer<'_>,
) -> Result<(installed::Entry, PendingPin), InstallError> {
    let V2Transfer {
        fetch_bytes,
        verify_release,
    } = transfer;
    let project = index.project(&step.project_id).ok_or_else(|| {
        InstallError::Refused(trf(
            "Project {project} is not in the Sunrise registry.",
            &[("project", &step.project_id)],
        ))
    })?;
    let release = project
        .releases
        .iter()
        .find(|r| r.version == step.version && r.file.sha256.eq_ignore_ascii_case(&step.sha256))
        .ok_or_else(|| {
            InstallError::Refused(trf(
                "Release {version} of {mod} is no longer in the Sunrise registry.",
                &[("version", &step.version), ("mod", &step.name)],
            ))
        })?;
    if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
        return Err(InstallError::Refused(trf(
            "{mod} was taken down: {reason}",
            &[("mod", &project.name), ("reason", &revoked.reason)],
        )));
    }

    // 1. Download .smod and check exact byte length + SHA-256.
    let archive_bytes = fetch_bytes(&release.file.url, MAX_FILE).map_err(InstallError::Download)?;
    if archive_bytes.len() as u64 != release.file.size {
        return Err(InstallError::Download(trf(
            "{mod} download size did not match the registry ({got} bytes, expected {expected}).",
            &[
                ("mod", &project.name),
                ("got", &archive_bytes.len().to_string()),
                ("expected", &release.file.size.to_string()),
            ],
        )));
    }
    let actual_sha256 = index::sha256(&archive_bytes);
    if !actual_sha256.eq_ignore_ascii_case(&release.file.sha256) {
        return Err(InstallError::Download(trf(
            "{mod} failed its SHA-256 check.",
            &[("mod", &project.name)],
        )));
    }
    if let Some(revoked) = index.revoked_by_sha256(&actual_sha256) {
        return Err(InstallError::Refused(trf(
            "{mod} was taken down: {reason}",
            &[("mod", &project.name), ("reason", &revoked.reason)],
        )));
    }
    let digest_bytes = attestation::parse_sha256_hex(&actual_sha256).ok_or_else(|| {
        InstallError::Download(trf(
            "{mod} failed its SHA-256 check.",
            &[("mod", &project.name)],
        ))
    })?;

    // 2. Download .sigstore.json bundle and verify offline attestation + Sunrise release policy.
    let bundle_bytes = fetch_bytes(&release.file.attestation, MAX_ATTESTATION_BYTES)
        .map_err(InstallError::Download)?;
    let expected = ExpectedRelease {
        repository_id: project.release_repository(release),
        workflow: &release.workflow,
    };
    let verified = verify_release(&bundle_bytes, digest_bytes, &expected).map_err(|error| {
        InstallError::Refused(trf(
            "Could not verify the release signature for {mod}: {detail}",
            &[("mod", &project.name), ("detail", &error.to_string())],
        ))
    })?;

    // 3. Check per-computer publisher pin (without mutating disk until publication commits).
    let pin = verified.pin();
    let mut accepted_change = None;
    match attestation::check_pin_in(pin_dir, &project.id, &pin) {
        Ok(attestation::PinStatus::FirstInstall | attestation::PinStatus::Matched) => {}
        Err(attestation::Error::PublisherChanged(change)) => {
            let confirmed = accepted_publisher_changes.contains(&project.id)
                || confirm_publisher_change.is_some_and(|ask| {
                    ask(&PublisherChangeQuestion {
                        mod_name: project.name.clone(),
                        current_version: Document::read(home).ok().and_then(|doc| {
                            doc.row(&step.mod_id)
                                .ok()
                                .flatten()
                                .and_then(|row| row["version"].as_str().map(str::to_owned))
                        }),
                        incoming_version: release.version.clone(),
                        change: (*change).clone(),
                    })
                });
            if !confirmed {
                return Err(InstallError::PublisherChanged(change));
            }
            accepted_change = Some(*change);
        }
        Err(error) => return Err(InstallError::Unproved(error.to_string())),
    }

    // 4. Write archive into the candidate's target directory and inspect its manifest & layout.
    let folder = match project.kind {
        V2Kind::NativeMod => crate::native_mods::INSTALLED,
        V2Kind::Mod => mod_list::MODS_DIR,
        _ => {
            return Err(InstallError::Refused(trf(
                "{mod} is not a mod package.",
                &[("mod", &project.name)],
            )));
        }
    };
    let other_folder = if folder == crate::native_mods::INSTALLED {
        mod_list::MODS_DIR
    } else {
        crate::native_mods::INSTALLED
    };
    // If a mod changed kind (content <-> native), remove any old file in the sibling folder so
    // `scan_packages` never sees two archives for the same slot.
    let sibling = home
        .join(other_folder)
        .join(format!("{}.smod", project.mod_id));
    if sibling.exists() {
        fs::remove_file(&sibling).map_err(|e| InstallError::Unproved(at(&sibling, e)))?;
    }

    let relative = format!("{folder}/{}.smod", project.mod_id);
    let destination = home.join(&relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| InstallError::Unproved(at(parent, e)))?;
    }
    fs::write(&destination, &archive_bytes)
        .map_err(|e| InstallError::Unproved(at(&destination, e)))?;

    let smod_kind = match project.kind {
        V2Kind::NativeMod => index::Kind::NativeMod,
        V2Kind::Mod => index::Kind::Mod,
        _ => unreachable!(),
    };
    let facts = index::inspect(&smod_kind, &destination).map_err(|error| {
        let _ = fs::remove_file(&destination);
        InstallError::Download(trf(
            "{mod} is not a valid package: {detail}",
            &[("mod", &project.name), ("detail", &error)],
        ))
    })?;
    if facts.id.as_deref() != Some(project.mod_id.as_str())
        || facts.version.as_deref() != Some(release.version.as_str())
    {
        let _ = fs::remove_file(&destination);
        return Err(InstallError::Download(trf(
            "{mod} archive manifest did not match the registry entry.",
            &[("mod", &project.name)],
        )));
    }
    // Refuse packages that smuggle required dependencies not declared in the signed v2 release.
    for (dep_id, dep) in &facts.dependencies {
        if !dep.details().optional
            && !release
                .dependencies
                .iter()
                .any(|listed| listed.required && listed.mod_id == *dep_id)
        {
            let _ = fs::remove_file(&destination);
            return Err(InstallError::Download(trf(
                "{mod} archive declares an unlisted required dependency ({dep}).",
                &[("mod", &project.name), ("dep", dep_id)],
            )));
        }
    }

    // The signed index listed these bytes and their attestation verified: a build takes them
    // without the unreviewed-code warning.
    if matches!(project.kind, V2Kind::NativeMod) {
        backend::allow_native_files_in(pin_dir, [actual_sha256.clone()])
            .map_err(InstallError::Unproved)?;
    }
    let entry = installed::Entry {
        source: SourceId::Sunrise,
        project: project.id.clone(),
        mod_id: Some(project.mod_id.clone()),
        slug: project.slug.clone(),
        title: project.name.clone(),
        kind: Kind::Mod,
        version: release.version.clone(),
        number: release.version.clone(),
        file: relative,
        checksum: format!("SHA-256:{actual_sha256}"),
        page: format!("{SITE}/{}", project.slug),
        dependency: step.dependency,
        verdict: Some(tr("Verified").into()),
        install_hold: None,
        repository: Some(installed::VerifiedRepository {
            id: verified.attested.repository_id,
            name: verified.attested.repository_name.clone(),
            path: project.repository.path.clone(),
            workflow: release.workflow.clone(),
        }),
    };
    Ok((
        entry,
        PendingPin {
            project_id: project.id.clone(),
            pin,
            accepted_change,
        },
    ))
}

pub(crate) fn commit_pending_pins(pin_dir: &Path, pins: &[PendingPin]) -> Result<(), String> {
    for pending in pins {
        if let Some(change) = &pending.accepted_change {
            attestation::accept_pin_in(pin_dir, change).map_err(|e| e.to_string())?;
        } else {
            attestation::pin_or_check_in(pin_dir, &pending.project_id, &pending.pin)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The index a change plans against: the signed one, with this computer's fit checks filling
/// only what it says nothing of.
fn local_fit_filled(index: &Index) -> Arc<Index> {
    Arc::new(crate::native_mods::local_fit::filled(index))
}

/// Install one or more schema-2 registry projects (by permanent 8-char project ID) into `instance`.
pub fn install_registry_v2(
    instance: &Instance,
    identity: Identity,
    requested_projects: Vec<String>,
    options: V2InstallOptions,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2Hooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    let catalog =
        catalogue_v2::change_index().map_err(|error| InstallError::Unproved(error.to_string()))?;
    install_registry_v2_with(
        V2EditRequest {
            instance,
            identity,
            index: local_fit_filled(&catalog.index),
            options,
            transfer: V2Transfer {
                fetch_bytes: &fetch_release_bytes,
                verify_release: &attestation::verify_release,
            },
        },
        requested_projects,
        choose,
        hooks,
    )
}

/// Update installed schema-2 registry mods in `instance`, always preserving each mod's chosen
/// project and respecting `chosen-versions.json` pins during bulk updates.
pub fn update_registry_v2(
    instance: &Instance,
    identity: Identity,
    requested: UpdateRequest,
    options: V2InstallOptions,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2Hooks<'_>,
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<InstalledBatch, InstallError> {
    let catalog =
        catalogue_v2::change_index().map_err(|error| InstallError::Unproved(error.to_string()))?;
    update_registry_v2_with(
        V2EditRequest {
            instance,
            identity,
            index: local_fit_filled(&catalog.index),
            options,
            transfer: V2Transfer {
                fetch_bytes: &fetch_release_bytes,
                verify_release: &attestation::verify_release,
            },
        },
        requested,
        choose,
        hooks,
        approve,
    )
}

/// Explicitly switch an installed mod slot (`mod_id`) to a different v2 project (`to_project_id`),
/// previewing affected dependents and saved-world risks before committing.
pub fn switch_provider_v2(
    instance: &Instance,
    identity: Identity,
    mod_id: &str,
    to_project_id: &str,
    options: V2InstallOptions,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2ProviderSwitchHooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    let catalog =
        catalogue_v2::change_index().map_err(|error| InstallError::Unproved(error.to_string()))?;
    switch_provider_v2_with(
        V2EditRequest {
            instance,
            identity,
            index: local_fit_filled(&catalog.index),
            options,
            transfer: V2Transfer {
                fetch_bytes: &fetch_release_bytes,
                verify_release: &attestation::verify_release,
            },
        },
        mod_id,
        to_project_id,
        choose,
        hooks,
    )
}

pub(crate) fn install_registry_v2_with(
    request: V2EditRequest<'_>,
    requested_projects: Vec<String>,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2Hooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    v2_edit_with(
        request,
        V2Action::Install(requested_projects),
        choose,
        hooks,
    )
}

pub(crate) fn update_registry_v2_with(
    request: V2EditRequest<'_>,
    requested: UpdateRequest,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2Hooks<'_>,
    approve: &dyn Fn(&ChangeImpact) -> bool,
) -> Result<InstalledBatch, InstallError> {
    v2_edit_with(request, V2Action::Update(requested, approve), choose, hooks)
}

enum V2Action<'a> {
    Install(Vec<String>),
    Update(UpdateRequest, &'a dyn Fn(&ChangeImpact) -> bool),
}

fn v2_edit_with(
    request: V2EditRequest<'_>,
    action: V2Action<'_>,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2Hooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    let V2EditRequest {
        instance,
        identity,
        index,
        mut options,
        transfer,
    } = request;
    options
        .jar_cache
        .get_or_insert_with(|| instance.home.join("jars"));
    if instance.kind == InstanceKind::Pack {
        return Err(InstallError::Refused(
            tr("No registry mods to install in this profile.").into(),
        ));
    }
    (hooks.install.progress)(InstallProgress::Checking);
    if matches!(&action, V2Action::Install(ids) if ids.is_empty()) {
        return Err(InstallError::Cancelled);
    }

    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let edit = profile.begin_edit(choose, |f| {
        (hooks.install.progress)(InstallProgress::Downloading(f))
    })?;
    let base_build = edit.base().to_owned();
    let default_cancelled = native_mods::Cancellation::default();
    let cancelled = hooks.install.cancellation.unwrap_or(&default_cancelled);
    let old = edit.candidate()?;
    let before = inventory_v2(old.path(), &index)?;
    let chosen_pins = chosen(&instance.home);

    let plan = match &action {
        V2Action::Install(projects) => plan_v2_install(
            &index,
            &base_build,
            projects,
            &before,
            &chosen_pins,
            &options,
            hooks.choose_provider,
        )?,
        V2Action::Update(request, _) => plan_v2_update(
            &index,
            &base_build,
            request,
            &before,
            &chosen_pins,
            &options,
            hooks.choose_provider,
        )?,
    };
    if plan.steps.is_empty() {
        return Ok(InstalledBatch::default());
    }

    let pin_dir = options.pin_dir.clone().unwrap_or_else(backend::data_dir);
    let next = edit.candidate()?;
    let mut doc = Document::read(next.path())?;
    let mut entries: BTreeMap<String, installed::Entry> = BTreeMap::new();
    let mut pending_pins: Vec<PendingPin> = Vec::new();
    let mut native = false;
    let mut replacements: Vec<(String, String)> = Vec::new();
    let mut retired_holds: BTreeSet<String> = BTreeSet::new();

    for (slot_idx, step) in plan.steps.iter().enumerate() {
        (hooks.install.progress)(InstallProgress::Downloading(
            slot_idx as f32 / plan.steps.len().max(1) as f32,
        ));
        let (mut entry, pin) = stage_and_verify_v2_release(
            &index,
            next.path(),
            step,
            &pin_dir,
            &options.accepted_publisher_changes,
            hooks.confirm_publisher_change,
            transfer,
        )?;
        // Preserve player-owned dependency flag if previously explicitly installed.
        if let Ok(Some(prev_row)) = doc.row(&step.mod_id) {
            if prev_row["dependency"].as_bool() == Some(false) {
                entry.dependency = false;
            }
        }
        // Check active Switch hold authority before replacing a held archive.
        if let Some(held) = edit.held(&step.mod_id) {
            let can_replace = matches!(action, V2Action::Update(..))
                && before.get(&step.mod_id).is_some_and(|prev| {
                    prev.ownership.is_owned()
                        && matches!(held.source(), managed_play::HeldSource::Registry { .. })
                        && prev.digest.eq_ignore_ascii_case(&{
                            let d = sha2::digest::Output::<sha2::Sha256>::from(*held.digest());
                            format!("{d:x}")
                        })
                });
            if !can_replace {
                return Err(held_change_refusal(&step.mod_id));
            }
            retired_holds.insert(step.mod_id.clone());
        }
        doc.replace(&entry)?;
        mod_list::set_enabled(next.path(), &step.mod_id, true)?;
        native |= step.kind == V2Kind::NativeMod;
        if let Some(prev) = before.get(&step.mod_id) {
            replacements.push((prev.package.manifest.name.clone(), step.mod_id.clone()));
        }
        entries.insert(step.mod_id.clone(), entry);
        pending_pins.push(pin);
    }
    doc.write(next.path())?;

    // Check enabled manifests and v2 revocations on the staged candidate before any build.
    check_staged_v2_candidate(next.path(), &index, |id| entries.contains_key(id))?;

    // A reviewed registry release asks nothing before its build: its attestation and the
    // signed index were verified while it was fetched and staged above.
    if native {
        if let Some(error) = native_mods::known_host_block(Some(&base_build)) {
            return Err(InstallError::Host(error));
        }
        cancelled.check().map_err(InstallError::Unproved)?;
    }

    // Every original jar the new set of registry mods lists is in this profile's cache (the game
    // reads `<home>/jars`) before anything is published; a jar Modrinth no longer has fails the
    // edit here.
    let staged = inventory_v2(next.path(), &index)?;
    let listed = collect_jar_requirements(
        staged
            .values()
            .filter(|item| item.enabled && item.ownership.is_owned())
            .map(|item| {
                (
                    item.package.manifest.id.as_str(),
                    item.package.manifest.jar.as_slice(),
                )
            }),
    )
    .map_err(|error| InstallError::Refused(jars::LaunchError::Conflict(error).to_string()))?;
    jars::fetch_listed(&instance.home, &listed)
        .map_err(|error| InstallError::Download(error.to_string()))?;

    (hooks.install.progress)(InstallProgress::Preparing);
    edit.prepare_first(&super::preparation_line(hooks.install.progress))
        .map_err(InstallError::Unproved)?;
    let proof = match prove_edit(&edit, next.path(), instance, cancelled, &hooks.install) {
        Ok(proof) => proof,
        Err(native_mods::Failed::Mods {
            left,
            error,
            compiling: false,
            ..
        }) => {
            for failed in &left {
                if matches!(failed.why, native_mods::Why::Unfit(_)) {
                    if let Some(step) = plan.steps.iter().find(|step| {
                        step.kind == V2Kind::NativeMod
                            && step.mod_id == failed.id
                            && step.version == failed.version
                    }) {
                        remember_unfit_sha256(&base_build, &step.sha256);
                    }
                }
            }
            return Err(InstallError::Unproved(error));
        }
        Err(_) if cancelled.is_stopped() => return Err(InstallError::Cancelled),
        Err(error) => return Err(InstallError::Unproved(error.to_string())),
    };

    // World impact review for updates (or guard against unexpected replacements on Install).
    let (suspect, reviewed_worlds) = match &action {
        V2Action::Install(_) => {
            // A dependency that replaces an installed mod may drop what saved worlds use.
            let suspect = if replacements.is_empty() {
                crate::worlds::SuspectMods::new()
            } else {
                crate::worlds::unproved_mod_effects(
                    old.path(),
                    edit.current(),
                    next.path(),
                    edit.current(),
                    replacements.iter().map(|(_, id)| id.as_str()),
                )?
            };
            let reviewed = guard_replacements(
                &instance.home,
                &suspect,
                replacements.iter().map(|(name, _)| name.as_str()),
            )?;
            (suspect, reviewed)
        }
        V2Action::Update(_, approve) => {
            let old_packages: Vec<&ScannedPackage> = entries
                .keys()
                .filter_map(|id| before.get(id).map(|m| &m.package))
                .collect();
            let (suspect, unchecked) = mutations::update_worlds(next.path(), old_packages)?;
            let reviewed_updates: Vec<mutations::ReviewedUpdate> = entries
                .iter()
                .map(|(id, entry)| mutations::ReviewedUpdate {
                    name: entry.title.clone(),
                    installed: before.get(id).map(|p| p.package.manifest.version.clone()),
                    replacement: entry.version.clone(),
                })
                .collect();
            let impact = ChangeImpact::read(
                &instance.home,
                mutations::Change::Update(reviewed_updates),
                &suspect,
            )?
            .with_unchecked_update(unchecked);
            if !approve(&impact) {
                return Err(InstallError::Cancelled);
            }
            (suspect, impact.into_evidence())
        }
    };

    (hooks.install.progress)(InstallProgress::Publishing);
    cancelled.check().map_err(|_| InstallError::Cancelled)?;
    // Persist verified publisher pins before publishing so a pin-store failure aborts the
    // install cleanly instead of failing after the mods are already live.
    commit_pending_pins(&pin_dir, &pending_pins).map_err(InstallError::Unproved)?;
    let authorize = || mutations::bind_worlds(&instance.home, &suspect, &reviewed_worlds);
    edit.publish_install(proof, cancelled, &retired_holds, authorize)?;

    let mut result = InstalledBatch::default();
    for (id, entry) in entries {
        if let Some(previous) = before.get(&id) {
            result
                .updated
                .push((previous.package.manifest.version.clone(), entry));
        } else {
            result.added.push(entry);
        }
    }
    Ok(result)
}

/// Internal testable entrypoint for explicit provider switching.
pub(crate) fn switch_provider_v2_with(
    request: V2EditRequest<'_>,
    mod_id: &str,
    to_project_id: &str,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: V2ProviderSwitchHooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    let V2EditRequest {
        instance,
        identity,
        index,
        mut options,
        transfer,
    } = request;
    options
        .jar_cache
        .get_or_insert_with(|| instance.home.join("jars"));
    if instance.kind == InstanceKind::Pack {
        return Err(InstallError::Refused(
            tr("No registry mods to install in this profile.").into(),
        ));
    }
    let to_project = index.project(to_project_id).ok_or_else(|| {
        InstallError::Refused(trf(
            "Project {project} is not in the Sunrise registry.",
            &[("project", to_project_id)],
        ))
    })?;
    if to_project.mod_id != mod_id {
        return Err(InstallError::Refused(trf(
            "{project} provides {actual}, not {expected}.",
            &[
                ("project", &to_project.name),
                ("actual", &to_project.mod_id),
                ("expected", mod_id),
            ],
        )));
    }
    if !matches!(to_project.kind, V2Kind::Mod | V2Kind::NativeMod) {
        return Err(InstallError::Refused(trf(
            "{mod} is not a mod package.",
            &[("mod", &to_project.name)],
        )));
    }

    (hooks.install.install.progress)(InstallProgress::Checking);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let edit = profile.begin_edit(choose, |f| {
        (hooks.install.install.progress)(InstallProgress::Downloading(f))
    })?;
    let base_build = edit.base().to_owned();
    let default_cancelled = native_mods::Cancellation::default();
    let cancelled = hooks
        .install
        .install
        .cancellation
        .unwrap_or(&default_cancelled);
    let old = edit.candidate()?;
    let before = inventory_v2(old.path(), &index)?;

    let current = before.get(mod_id).ok_or_else(|| {
        InstallError::Refused(trf(
            "{mod} is not installed in this profile.",
            &[("mod", mod_id)],
        ))
    })?;
    if current.ownership.project_id() == Some(to_project.id.as_str()) {
        return Err(InstallError::Refused(trf(
            "{mod} is already using {project}.",
            &[("mod", mod_id), ("project", &to_project.name)],
        )));
    }

    let unfit_hashes = known_unfit_updates(&base_build);
    let jar_cache = options.jar_cache.as_deref();
    let version_override = options.versions.get(&to_project.id).map(String::as_str);
    let release = select_project_release(
        &index,
        to_project,
        &base_build,
        version_override,
        None,
        &unfit_hashes,
        jar_cache,
    )?;
    let step = planned_from(to_project, release, false);

    // An enabled dependent pinned to the old project, or whose requirement the new release can't
    // meet, refuses the switch before anything downloads.
    if let Some(incompatible) =
        find_affected_dependents(&index, &before, mod_id, &to_project.id, &release.version).first()
    {
        return Err(InstallError::Refused(trf(
            "Cannot switch {mod} to {project}: {dependent} ({reason}).",
            &[
                ("mod", mod_id),
                ("project", &to_project.name),
                ("dependent", &incompatible.name),
                ("reason", &incompatible.reason),
            ],
        )));
    }

    // Plan any new required dependencies of the incoming provider.
    let mut dep_queue: VecDeque<Queued> = VecDeque::new();
    for dep in &release.dependencies {
        if !dep.required || dep.mod_id == mod_id {
            continue;
        }
        let range = Some(dep.version.clone());
        let installed = before.get(&dep.mod_id).filter(|m| m.enabled);
        let in_range_now =
            installed.is_some_and(|m| in_range(Some(&dep.version), &m.package.manifest.version));
        if let Some(dep_pid) = dep.project.as_deref() {
            if installed.is_none_or(|m| m.ownership.project_id() != Some(dep_pid)) || !in_range_now
            {
                dep_queue.push_back((dep_pid.to_owned(), true, to_project.name.clone(), range));
            }
        } else if let Some(owner) = installed
            .filter(|_| !in_range_now)
            .and_then(|m| m.ownership.project_id())
        {
            // Keep the profile's provider, at a release in the range.
            dep_queue.push_back((owner.to_owned(), true, to_project.name.clone(), range));
        } else if installed.is_none() {
            if let Some(picked) = options.provider_choices.get(&dep.mod_id) {
                dep_queue.push_back((picked.clone(), true, to_project.name.clone(), range));
            } else {
                let mut candidates: Vec<ProviderCandidate> = index
                    .projects_for_mod(&dep.mod_id)
                    .filter(|p| {
                        matches!(p.kind, V2Kind::Mod | V2Kind::NativeMod)
                            && index.revoked(&p.id, None).is_none()
                    })
                    .filter_map(|p| {
                        let best = p
                            .releases
                            .iter()
                            .filter(|r| {
                                r.fit_verdict(&base_build) == FitVerdict::Fits
                                    && in_range(Some(&dep.version), &r.version)
                                    && index.revoked(&p.id, Some(&r.file.sha256)).is_none()
                            })
                            .max_by(|a, b| compare_versions(&a.version, &b.version))?;
                        Some(ProviderCandidate {
                            project_id: p.id.clone(),
                            slug: p.slug.clone(),
                            name: p.name.clone(),
                            repository_name: p.repository.name.clone(),
                            version: best.version.clone(),
                        })
                    })
                    .collect();
                candidates.sort_by(|a, b| a.project_id.cmp(&b.project_id));
                match candidates.as_slice() {
                    [] => {
                        return Err(InstallError::NotForVersion(trf(
                            "{mod} needs {dep} {range}, which has no release fitting Sunrise {build}.",
                            &[
                                ("mod", &to_project.name),
                                ("dep", &dep.mod_id),
                                ("range", &dep.version.to_string()),
                                ("build", &base_build),
                            ],
                        )));
                    }
                    [only] => dep_queue.push_back((
                        only.project_id.clone(),
                        true,
                        to_project.name.clone(),
                        range,
                    )),
                    _ => {
                        let need = ProviderChoiceNeeded {
                            mod_id: dep.mod_id.clone(),
                            required_by: to_project.name.clone(),
                            candidates,
                        };
                        if let Some(picked) =
                            hooks.install.choose_provider.and_then(|ask| ask(&need))
                        {
                            if need.candidates.iter().any(|c| c.project_id == picked) {
                                dep_queue.push_back((picked, true, to_project.name.clone(), range));
                                continue;
                            }
                        }
                        return Err(InstallError::AmbiguousProvider(Box::new(need)));
                    }
                }
            }
        }
    }
    let dep_steps = if dep_queue.is_empty() {
        Vec::new()
    } else {
        let chosen_pins = chosen(&instance.home);
        resolve_v2_queue(
            &ResolveContext {
                index: &index,
                build: &base_build,
                installed: &before,
                chosen_pins: &chosen_pins,
                options: &options,
                unfit_hashes: &unfit_hashes,
                choose_provider: hooks.install.choose_provider,
            },
            dep_queue,
        )?
        .steps
    };

    let pin_dir = options.pin_dir.clone().unwrap_or_else(backend::data_dir);
    let next = edit.candidate()?;
    let mut doc = Document::read(next.path())?;
    let mut entries: BTreeMap<String, installed::Entry> = BTreeMap::new();
    let mut pending_pins: Vec<PendingPin> = Vec::new();
    let mut native = false;

    for planned_step in std::iter::once(&step).chain(dep_steps.iter()) {
        let (entry, pin) = stage_and_verify_v2_release(
            &index,
            next.path(),
            planned_step,
            &pin_dir,
            &options.accepted_publisher_changes,
            hooks.install.confirm_publisher_change,
            transfer,
        )?;
        doc.replace(&entry)?;
        mod_list::set_enabled(next.path(), &planned_step.mod_id, true)?;
        native |= planned_step.kind == V2Kind::NativeMod;
        entries.insert(planned_step.mod_id.clone(), entry);
        pending_pins.push(pin);
    }
    doc.write(next.path())?;

    check_staged_v2_candidate(next.path(), &index, |id| entries.contains_key(id))?;

    // A reviewed registry release asks nothing before its build: its attestation and the
    // signed index were verified while it was fetched and staged above.
    if native {
        if let Some(error) = native_mods::known_host_block(Some(&base_build)) {
            return Err(InstallError::Host(error));
        }
        cancelled.check().map_err(InstallError::Unproved)?;
    }

    (hooks.install.install.progress)(InstallProgress::Preparing);
    edit.prepare_first(&super::preparation_line(hooks.install.install.progress))
        .map_err(InstallError::Unproved)?;
    let proof = prove_edit(
        &edit,
        next.path(),
        instance,
        cancelled,
        &hooks.install.install,
    )
    .map_err(|error| {
        if cancelled.is_stopped() {
            InstallError::Cancelled
        } else {
            InstallError::Unproved(error.to_string())
        }
    })?;

    let (suspect, _) = mutations::update_worlds(next.path(), std::iter::once(&current.package))?;
    let worlds = mutations::world_evidence(&instance.home, &suspect)?;
    let preview = ProviderSwitchPreview {
        mod_id: mod_id.to_owned(),
        from_project: current.ownership.project_id().map(str::to_owned),
        from_name: current.package.manifest.name.clone(),
        to_project: to_project.id.clone(),
        to_name: to_project.name.clone(),
        to_version: release.version.clone(),
        worlds: worlds.clone(),
    };
    if !(hooks.approve_switch)(&preview) {
        return Err(InstallError::Cancelled);
    }

    (hooks.install.install.progress)(InstallProgress::Publishing);
    cancelled.check().map_err(|_| InstallError::Cancelled)?;
    commit_pending_pins(&pin_dir, &pending_pins).map_err(InstallError::Unproved)?;
    let authorize = || mutations::bind_worlds(&instance.home, &suspect, &worlds);
    edit.publish_install(proof, cancelled, &BTreeSet::new(), authorize)?;

    let mut result = InstalledBatch::default();
    for (id, entry) in entries {
        if let Some(previous) = before.get(&id) {
            result
                .updated
                .push((previous.package.manifest.version.clone(), entry));
        } else {
            result.added.push(entry);
        }
    }
    Ok(result)
}

fn find_affected_dependents(
    index: &Index,
    installed: &BTreeMap<String, InstalledV2Mod>,
    slot_mod_id: &str,
    to_project_id: &str,
    to_version: &str,
) -> Vec<V2AffectedDependent> {
    let parsed_new = parse_version(to_version).ok();
    let mut affected = Vec::new();
    for (id, item) in installed {
        if id == slot_mod_id || !item.enabled {
            continue;
        }
        // Check v2 release-level pinned-project dependency.
        if let Some(dep_pid) = item.ownership.project_id() {
            if let Some(project) = index.project(dep_pid) {
                if let Some(rel) = project
                    .releases
                    .iter()
                    .find(|r| r.file.sha256.eq_ignore_ascii_case(&item.digest))
                {
                    for dep in &rel.dependencies {
                        if let Some(req) = dep.project.as_deref() {
                            if dep.required && dep.mod_id == slot_mod_id && req != to_project_id {
                                affected.push(V2AffectedDependent {
                                    mod_id: id.clone(),
                                    name: item.package.manifest.name.clone(),
                                    reason: trf(
                                        "requires project {project} specifically",
                                        &[("project", req)],
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        // Check manifest semver constraint against the new provider's version.
        if let Some(dep) = item.package.manifest.dependencies.get(slot_mod_id) {
            let details = dep.details();
            if !details.optional {
                let fits = parsed_new
                    .as_ref()
                    .is_none_or(|ver| details.version.matches(ver));
                if !fits {
                    let req = details.version.to_string();
                    affected.push(V2AffectedDependent {
                        mod_id: id.clone(),
                        name: item.package.manifest.name.clone(),
                        reason: trf(
                            "needs {mod} {req}, not {version}",
                            &[("mod", slot_mod_id), ("req", &req), ("version", to_version)],
                        ),
                    });
                }
            }
        }
    }
    affected
}

/// Validate that all enabled packages in staged candidate `home` have no manifest conflicts and no
/// active v2 revocations.
pub(crate) fn check_staged_v2_candidate(
    home: &Path,
    index: &Index,
    staged: impl Fn(&str) -> bool,
) -> Result<(), InstallError> {
    let current = inventory_v2(home, index)?;
    let enabled_manifests: BTreeMap<&str, &sunrise_launcher_smod::manifest::Manifest> = current
        .iter()
        .filter(|(_, m)| m.enabled)
        .map(|(k, m)| (k.as_str(), &m.package.manifest))
        .collect();
    for item in current.values() {
        if !item.enabled || item.package.directory {
            continue;
        }
        if let Some(revoked) = revoked_for_package(index, &item.ownership, &item.digest) {
            return Err(InstallError::Refused(trf(
                "{mod} was taken down: {reason}",
                &[
                    ("mod", &item.package.manifest.name),
                    ("reason", &revoked.reason),
                ],
            )));
        }
        if let Some(missing) = missing_required(&item.package.manifest, &enabled_manifests) {
            return Err(InstallError::Refused(trf(
                "{mod} needs {dep} to be installed and enabled.",
                &[("mod", &item.package.manifest.name), ("dep", missing)],
            )));
        }
    }
    if let Some(conflict) = manifest_conflict(home, Side::Both, staged)? {
        return Err(InstallError::Refused(conflict.reason));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
