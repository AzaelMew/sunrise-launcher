//! Schema-2 game-build switching: preserves each installed mod's chosen project, relies solely on
//! authoritative `release.fits` / `release.does_not_fit` verdicts (`Fits` / `DoesNotFit` ->
//! `NoFit` / `Unknown` -> `Unproved`), verifies SHA-256 + offline Sigstore attestation + publisher
//! pin for every replacement archive, honors `chosen-versions.json` pins, and keeps verified
//! project identity in `HeldSource::Registry`.

use super::{
    check_staged_v2_candidate, chosen_version_pin, commit_pending_pins, compare_versions,
    fetch_release_bytes, inventory_v2, revoked_for_package, stage_and_verify_v2_release,
    BytesFetcher, InstalledV2Mod, PendingPin, PlannedV2Release, ReleaseVerifier, V2Ownership,
    V2Transfer,
};
use crate::{
    attestation::{self, PublisherChange},
    backend,
    browser::{
        mod_list,
        sunrise::{
            chosen,
            registry_install::{
                at, build_progress, switch::sha256_bytes, BuildChoice, Document, InstallError,
                InstallProgress, SwitchDecision, SwitchHooks, SwitchQuestion, Switched,
            },
            v2 as catalogue_v2,
        },
    },
    i18n::tr,
    instances::Instance,
    managed_play::{Held, HeldReason, HeldRoot, HeldSource, Profile},
    profile_txn::Identity,
    worlds::switch::{Fence, TargetAge},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher_smod::index_v2::{FitVerdict, Index, Project, Release};

/// Callbacks and pin directory override for v2 build switching.
pub struct V2SwitchHooks<'a> {
    pub switch: SwitchHooks<'a>,
    pub confirm_publisher_change: Option<super::ConfirmPublisherChange<'a>>,
    pub pin_dir: Option<PathBuf>,
}

/// Read-only check: true when every enabled v2-owned mod in `home` has a non-revoked release of
/// its SAME chosen project whose authoritative `fits` list names exact build `tag` (respecting any
/// `chosen-versions.json` pin).
pub fn checked_switch_fit_v2(index: &Index, home: &Path, tag: &str) -> bool {
    let Ok(installed) = inventory_v2(home, index) else {
        return false;
    };
    let pins = chosen(home);
    for (slot, item) in &installed {
        if !item.enabled {
            continue;
        }
        if item.package.directory {
            return false;
        }
        if revoked_for_package(index, &item.ownership, &item.digest).is_some() {
            return false;
        }
        let Some(project_id) = item.ownership.project_id() else {
            // Unadopted/local mod cannot be proved against `tag` from the registry alone.
            return false;
        };
        let Some(project) = index.project(project_id) else {
            return false;
        };
        if index.revoked(&project.id, None).is_some() {
            return false;
        }
        let pin = chosen_version_pin(&pins, &project.id, slot);
        let fits = project.releases.iter().any(|release| {
            pin.is_none_or(|p| release.version == p)
                && release.fit_verdict(tag) == FitVerdict::Fits
                && index
                    .revoked(&project.id, Some(&release.file.sha256))
                    .is_none()
        });
        if !fits {
            return false;
        }
    }
    true
}

/// Whether a build switch for `home` should use the signed v2 registry.
///
/// Defaults to v2 now that the official registry publishes `v2/index.signed` and no longer serves
/// the legacy v1 `index.json`. Falls back to v1 only when the caller explicitly overrides
/// `SUNRISE_MOD_INDEX` (without setting `SUNRISE_MOD_INDEX_V2`) and the profile holds no v2 mods.
/// Always propagates `installed.json` read errors so a corrupt manifest never silently falls back.
pub fn prefer_v2_switch(home: &Path) -> Result<bool, String> {
    let has_v1 = std::env::var_os("SUNRISE_MOD_INDEX").is_some_and(|v| !v.is_empty());
    let has_v2 = std::env::var_os(catalogue_v2::ENV_OVERRIDE).is_some_and(|v| !v.is_empty());
    prefer_v2_switch_with(home, has_v1 && !has_v2)
}

pub fn prefer_v2_switch_with(home: &Path, legacy_v1_override: bool) -> Result<bool, String> {
    let has_v2_mod = crate::browser::installed::Installed::read(home)?
        .entries
        .iter()
        .any(|e| e.mod_id.is_some());
    Ok(has_v2_mod || !legacy_v1_override)
}

/// Switch `instance` to exact prepared Sunrise build `tag` using the signed v2 registry.
pub fn switch_build_v2(
    instance: &Instance,
    identity: Identity,
    tag: &str,
    hooks: V2SwitchHooks<'_>,
) -> Result<Switched, String> {
    switch_profile_build_v2(instance, identity, tag, None, hooks)
}

/// Switch `instance` to exact prepared Sunrise build `tag` and persist `choice` atomically.
pub fn switch_profile_build_v2(
    instance: &Instance,
    identity: Identity,
    tag: &str,
    choice: Option<BuildChoice>,
    hooks: V2SwitchHooks<'_>,
) -> Result<Switched, String> {
    switch_profile_build_v2_with_index(instance, identity, tag, choice, hooks, || {
        catalogue_v2::refreshed_index()
            .map(|c| c.index)
            .map_err(|e| e.to_string())
    })
}

fn switch_profile_build_v2_with_index(
    instance: &Instance,
    identity: Identity,
    tag: &str,
    choice: Option<BuildChoice>,
    hooks: V2SwitchHooks<'_>,
    fetch_index: impl FnOnce() -> Result<Arc<Index>, String>,
) -> Result<Switched, String> {
    if instance.kind == crate::instances::Kind::Pack {
        return Err(tr("Choose an ordinary profile to switch builds.").into());
    }
    (hooks.switch.progress)(InstallProgress::PreparingBuild);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let cancellation = hooks.switch.cancellation;
    cancellation.check()?;
    let previous_holds: Vec<Held> = profile
        .active()?
        .map(|active| active.holds().cloned().collect())
        .unwrap_or_default();
    let Some(edit) = profile.begin_switch(tag)? else {
        if let Some(choice) = choice {
            cancellation.begin_publication()?;
            crate::browser::sunrise::registry_install::switch::save_build_choice(
                &instance.id,
                tag,
                choice,
            )?;
        }
        return Ok(Switched::AlreadySelected);
    };
    let index = fetch_index()?;
    let result = switch_build_v2_edit(
        instance,
        edit,
        &previous_holds,
        index,
        tag,
        choice,
        hooks,
        &fetch_release_bytes,
        &attestation::verify_release,
    )?;
    if let (Some(_), Switched::Changed(active)) = (choice, &result) {
        if let Some(committed) = active.build_choice() {
            if let Err(error) =
                crate::instances::sync_build_choice(&instance.id, &instance.home, committed)
            {
                // The choice is already durable in active-build.json; listing or
                // editing the profile recovers instances.json on the next write.
                eprintln!("Sunrise: instances.json is not saved: {error}");
            }
        }
    }
    Ok(result)
}

pub(super) enum SlotPlan<'a> {
    /// Keep the exact on-disk archive as-is (current archive already fits `tag`, or local mod).
    Keep,
    /// Replace with a fitting release of the SAME chosen project.
    Replace(&'a Project, &'a Release),
    /// Hold the archive off because the chosen project has no fitting release for `tag`.
    Hold(HeldReason),
    /// Remove/withhold because revoked by the signed v2 index.
    Revoked { reason: String, owned: bool },
}

pub(super) fn plan_slot_for_build<'a>(
    index: &'a Index,
    slot: &str,
    item: &InstalledV2Mod,
    tag: &str,
    pins: &BTreeMap<String, String>,
) -> SlotPlan<'a> {
    if let Some(revoked) = revoked_for_package(index, &item.ownership, &item.digest) {
        return SlotPlan::Revoked {
            reason: revoked.reason.clone(),
            owned: item.ownership.is_owned(),
        };
    }
    let Some(project_id) = item.ownership.project_id() else {
        // Local or unadopted v1 mod: keep its exact bytes as-is; NEVER substitute a v2 project
        // that happens to share the same `mod_id`.
        return SlotPlan::Keep;
    };
    let Some(project) = index.project(project_id) else {
        return SlotPlan::Hold(HeldReason::Unproved {
            build: tag.to_owned(),
            detail: None,
        });
    };
    if let Some(revoked) = index.revoked(&project.id, None) {
        return SlotPlan::Revoked {
            reason: revoked.reason.clone(),
            owned: true,
        };
    }

    let pin = chosen_version_pin(pins, &project.id, slot);
    // If the currently installed exact archive already fits `tag` and matches any pin, keep it.
    let current_release = project
        .releases
        .iter()
        .find(|r| r.file.sha256.eq_ignore_ascii_case(&item.digest));
    if current_release.is_some_and(|current| {
        pin.is_none_or(|p| p == current.version) && current.fit_verdict(tag) == FitVerdict::Fits
    }) {
        return SlotPlan::Keep;
    }

    // If pinned to an explicit version in chosen-versions.json, check ONLY that pinned version.
    if let Some(pinned_ver) = pin {
        let Some(release) = project.releases.iter().find(|r| r.version == pinned_ver) else {
            return SlotPlan::Hold(HeldReason::Unproved {
                build: tag.to_owned(),
                detail: None,
            });
        };
        if index
            .revoked(&project.id, Some(&release.file.sha256))
            .is_some()
        {
            return SlotPlan::Hold(HeldReason::NoFit {
                build: tag.to_owned(),
                detail: None,
            });
        }
        return match release.fit_verdict(tag) {
            FitVerdict::Fits => SlotPlan::Replace(project, release),
            FitVerdict::DoesNotFit => SlotPlan::Hold(HeldReason::NoFit {
                build: tag.to_owned(),
                detail: None,
            }),
            FitVerdict::Unknown => SlotPlan::Hold(HeldReason::Unproved {
                build: tag.to_owned(),
                detail: None,
            }),
        };
    }

    // Otherwise search ONLY `project.releases` (NEVER another project for the same mod_id!).
    let mut fitting: Vec<&Release> = Vec::new();
    let mut saw_unknown = false;
    let mut saw_does_not_fit = false;
    for release in &project.releases {
        if index
            .revoked(&project.id, Some(&release.file.sha256))
            .is_some()
        {
            continue;
        }
        match release.fit_verdict(tag) {
            FitVerdict::Fits => fitting.push(release),
            FitVerdict::DoesNotFit => saw_does_not_fit = true,
            FitVerdict::Unknown => saw_unknown = true,
        }
    }
    if let Some(best) = fitting
        .into_iter()
        .max_by(|a, b| compare_versions(&a.version, &b.version))
    {
        return SlotPlan::Replace(project, best);
    }
    if !saw_unknown && saw_does_not_fit {
        SlotPlan::Hold(HeldReason::NoFit {
            build: tag.to_owned(),
            detail: None,
        })
    } else {
        SlotPlan::Hold(HeldReason::Unproved {
            build: tag.to_owned(),
            detail: None,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn switch_build_v2_edit(
    instance: &Instance,
    edit: crate::managed_play::Edit<'_>,
    previous_holds: &[Held],
    index: Arc<Index>,
    tag: &str,
    choice: Option<BuildChoice>,
    hooks: V2SwitchHooks<'_>,
    fetch_bytes: BytesFetcher<'_>,
    verify_release: ReleaseVerifier<'_>,
) -> Result<Switched, String> {
    (hooks.switch.progress)(InstallProgress::Checking);
    let old = edit.candidate()?;
    let before = inventory_v2(old.path(), &index)?;
    let pins = chosen(&instance.home);
    let pin_dir = hooks.pin_dir.clone().unwrap_or_else(backend::data_dir);

    let next = edit.candidate()?;
    let mut doc = Document::read(next.path())?;
    let mut pending_pins: Vec<PendingPin> = Vec::new();
    let mut holds: Vec<Held> = Vec::new();
    let mut removed: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut staged_ids: BTreeSet<String> = BTreeSet::new();
    let mut dropped_ids: BTreeSet<String> = BTreeSet::new();

    // Re-offer previously held local or source mods whose exact bytes are still present and not
    // revoked; their old holds are retired, and any that still fail `prove` below are held again
    // with today's target build and reason.
    let rechecked = collect_rechecked_local_holds(previous_holds, &before, &index);
    for slot in &rechecked {
        mod_list::set_enabled(next.path(), slot, true)?;
    }

    for (slot, item) in &before {
        match plan_slot_for_build(&index, slot, item, tag, &pins) {
            SlotPlan::Keep => {}
            SlotPlan::Replace(project, release) => {
                let step = PlannedV2Release {
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
                    repository_id: project.repository.id,
                    repository_name: project.repository.name.clone(),
                    repository_path: project.repository.path.clone(),
                    dependency: doc
                        .row(slot)?
                        .is_some_and(|row| row["dependency"].as_bool() == Some(true)),
                };
                let (entry, pin) = stage_and_verify_v2_release(
                    &index,
                    next.path(),
                    &step,
                    &pin_dir,
                    &BTreeSet::new(),
                    hooks.confirm_publisher_change,
                    V2Transfer {
                        fetch_bytes,
                        verify_release,
                    },
                )
                .map_err(|error| match error {
                    InstallError::PublisherChanged(change) => change_message(&change),
                    other => other.to_string(),
                })?;
                doc.replace(&entry)?;
                staged_ids.insert(slot.clone());
                pending_pins.push(pin);
            }
            SlotPlan::Hold(reason) => {
                mod_list::set_enabled(next.path(), slot, false)?;
                dropped_ids.insert(slot.clone());
                holds.push(make_v2_hold(slot, item, reason)?);
            }
            SlotPlan::Revoked { reason, owned } => {
                mod_list::set_enabled(next.path(), slot, false)?;
                dropped_ids.insert(slot.clone());
                if owned {
                    let rel = item
                        .package
                        .path
                        .strip_prefix(old.path())
                        .map_err(|e| at(&item.package.path, e))?;
                    let target = next.path().join(rel);
                    if target.exists() {
                        fs::remove_file(&target).map_err(|e| at(&target, e))?;
                    }
                    doc.remove(slot)?;
                    removed.insert(slot.clone(), (item.package.manifest.name.clone(), reason));
                } else {
                    holds.push(make_v2_hold(
                        slot,
                        item,
                        HeldReason::NoFit {
                            build: tag.to_owned(),
                            detail: Some(reason),
                        },
                    )?);
                }
            }
        }
    }

    // Hold any enabled dependent whose required dependency was held or removed.
    hold_missing_dependents(next.path(), &before, &mut dropped_ids, &mut holds, tag)?;

    doc.write(next.path())?;
    check_staged_v2_candidate(next.path(), &index, |id| staged_ids.contains(id))
        .map_err(|e| e.to_string())?;

    // Prove the candidate image. If an enabled local, source, or unadopted mod fails to fit or
    // compile on `tag`, hold it (and its required dependents) and retry instead of aborting the
    // whole switch.
    let proof = loop {
        (hooks.switch.progress)(InstallProgress::Preparing);
        match edit.prove(
            next.path(),
            instance.native_profile,
            hooks.switch.cancellation,
            |step| build_progress(step, hooks.switch.progress),
            None,
        ) {
            Ok(proof) => break proof,
            Err(crate::native_mods::Failed::Mods {
                left, compiling, ..
            }) => {
                hooks.switch.cancellation.check()?;
                let blamed = classify_prove_failure(&left, compiling, tag);
                let mut held_any = false;
                for (slot, reason) in blamed {
                    if dropped_ids.contains(&slot) || !mod_list::is_enabled(next.path(), &slot) {
                        continue;
                    }
                    let Some(item) = before.get(&slot) else {
                        continue;
                    };
                    mod_list::set_enabled(next.path(), &slot, false)?;
                    dropped_ids.insert(slot.clone());
                    holds.push(make_v2_hold(&slot, item, reason)?);
                    held_any = true;
                }
                if !held_any {
                    return Err(
                        tr("Sunrise couldn't prove a working mod set for this build.").into(),
                    );
                }
                hold_missing_dependents(next.path(), &before, &mut dropped_ids, &mut holds, tag)?;
            }
            Err(error) => return Err(error.to_string()),
        }
    };

    // Compute world risks and SwitchQuestion.
    let mut suspect = crate::worlds::SuspectMods::new();
    for slot in &dropped_ids {
        if before.get(slot).is_some_and(|m| m.enabled) {
            suspect.insert(slot.clone(), BTreeSet::new());
        }
    }
    let replaced_packages: Vec<&crate::managed_play::ScannedPackage> = staged_ids
        .iter()
        .filter_map(|id| before.get(id).map(|m| &m.package))
        .collect();
    let (update_drops, _) = crate::browser::sunrise::registry_install::mutations::update_worlds(
        next.path(),
        replaced_packages,
    )?;
    for (id, ns) in update_drops {
        suspect.entry(id).or_default().extend(ns);
    }

    let resources = edit.switch_resources()?;
    let mut order = None;
    let mut compare = |commit: &str| {
        order
            .get_or_insert_with(|| backend::WorldBuildOrder::for_target(tag))
            .compare_saved(commit)
    };
    let (fence, worlds) = Fence::hold(
        &instance.home,
        suspect,
        |id| {
            before
                .get(id)
                .map_or("", |item| item.package.manifest.name.as_str())
        },
        Some(TargetAge {
            data_version: backend::prepared_world_version(&resources),
            compare_saved: &mut compare,
        }),
    )?;
    let (held_names, unproved_names) = split_hold_names(&holds);
    let question = SwitchQuestion {
        build: tag.to_owned(),
        worlds,
        held: held_names,
        unproved: unproved_names,
        reasons: holds
            .iter()
            .filter_map(|hold| Some((hold.title().to_owned(), hold.reason().detail()?.to_owned())))
            .collect(),
        removed: removed.values().cloned().collect(),
    };

    let decision = if question.needed() {
        (hooks.switch.approve)(&question)?
    } else {
        SwitchDecision::TurnOff
    };
    if decision == SwitchDecision::Cancel {
        return Ok(Switched::Stayed);
    }

    let (final_proof, final_holds, final_removed) =
        if decision == SwitchDecision::Remove && !holds.is_empty() {
            let mut pruned_doc = Document::read(next.path())?;
            let mut removed_all = removed;
            for hold in &holds {
                let path = next.path().join(hold.path());
                if path.exists() {
                    fs::remove_file(&path).map_err(|e| at(&path, e))?;
                }
                pruned_doc.remove(hold.id())?;
                removed_all.insert(
                    hold.id().to_owned(),
                    (hold.title().to_owned(), hold.reason().label().to_owned()),
                );
            }
            pruned_doc.write(next.path())?;
            let proof = edit
                .prove(
                    next.path(),
                    instance.native_profile,
                    hooks.switch.cancellation,
                    |step| build_progress(step, hooks.switch.progress),
                    None,
                )
                .map_err(|e| e.to_string())?;
            (proof, Vec::new(), removed_all)
        } else {
            (proof, holds, removed)
        };

    let committed_choice = choice
        .map(|choice| {
            let (channel, version) = match choice {
                BuildChoice::Follow(channel) => (Some(channel), None),
                BuildChoice::Keep => (crate::instances::Channel::of_build(tag), Some(tag.into())),
            };
            crate::managed_play::CommittedBuildChoice::new(channel, version)
        })
        .transpose()?;

    let active = edit.publish_switch(
        final_proof,
        hooks.switch.cancellation,
        crate::managed_play::SwitchPublication {
            holds: final_holds,
            removed: final_removed,
            retired: rechecked,
            build_choice: committed_choice,
        },
        || fence.revalidate(&instance.home),
        || {
            (hooks.switch.at_cutoff)()?;
            (hooks.switch.progress)(InstallProgress::Publishing);
            commit_pending_pins(&pin_dir, &pending_pins)?;
            Ok(())
        },
    )?;
    Ok(Switched::Changed(Box::new(active)))
}

pub(super) fn make_v2_hold(
    slot: &str,
    item: &InstalledV2Mod,
    reason: HeldReason,
) -> Result<Held, String> {
    let root = if item.package.manifest.native.is_some() {
        HeldRoot::Native
    } else {
        HeldRoot::Content
    };
    let leaf = item
        .package
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| tr("Sunrise couldn't identify the mod archive filename.").to_owned())?
        .to_owned();
    let digest = sha256_bytes(&item.digest)?;
    let source = match &item.ownership {
        V2Ownership::Verified {
            project_id,
            repository,
            ..
        }
        | V2Ownership::AdoptedV1 {
            project_id,
            repository,
            ..
        } => HeldSource::Registry {
            project: project_id.clone(),
            version: item.package.manifest.version.clone(),
            repository: Some(repository.clone()),
        },
        V2Ownership::Local => HeldSource::Local {
            version: item.package.manifest.version.clone(),
        },
    };
    Held::new(
        slot.to_owned(),
        item.package.manifest.name.clone(),
        digest,
        (root, leaf),
        source,
        item.enabled,
        reason,
    )
}

fn split_hold_names(holds: &[Held]) -> (Vec<String>, Vec<String>) {
    let mut held = Vec::new();
    let mut unproved = Vec::new();
    for hold in holds {
        if hold.reason().unproved() {
            unproved.push(hold.title().to_owned());
        } else {
            held.push(hold.title().to_owned());
        }
    }
    held.sort();
    unproved.sort();
    (held, unproved)
}

fn change_message(change: &PublisherChange) -> String {
    attestation::Error::PublisherChanged(Box::new(change.clone())).to_string()
}

/// Disable any enabled mod whose required dependency was held or removed, recording a hold for it.
fn hold_missing_dependents(
    stage_home: &Path,
    before: &BTreeMap<String, InstalledV2Mod>,
    dropped_ids: &mut BTreeSet<String>,
    holds: &mut Vec<Held>,
    tag: &str,
) -> Result<(), String> {
    loop {
        let mut newly_held = Vec::new();
        for (slot, item) in before {
            if dropped_ids.contains(slot) || !mod_list::is_enabled(stage_home, slot) {
                continue;
            }
            let missing_dep = item
                .package
                .manifest
                .dependencies
                .iter()
                .any(|(dep_id, dep)| !dep.details().optional && dropped_ids.contains(dep_id));
            if missing_dep {
                newly_held.push(slot.clone());
            }
        }
        if newly_held.is_empty() {
            break;
        }
        for slot in newly_held {
            mod_list::set_enabled(stage_home, &slot, false)?;
            dropped_ids.insert(slot.clone());
            if let Some(item) = before.get(&slot) {
                holds.push(make_v2_hold(
                    &slot,
                    item,
                    HeldReason::NoFit {
                        build: tag.to_owned(),
                        detail: None,
                    },
                )?);
            }
        }
    }
    Ok(())
}

/// Previously held local or source mods whose exact archive is still on disk (currently disabled)
/// and not revoked are re-offered on the next build switch; their old holds are retired.
fn collect_rechecked_local_holds(
    previous_holds: &[Held],
    before: &BTreeMap<String, InstalledV2Mod>,
    index: &Index,
) -> BTreeSet<String> {
    let mut rechecked = BTreeSet::new();
    for hold in previous_holds {
        if !matches!(
            hold.source(),
            HeldSource::Local { .. } | HeldSource::Github { .. }
        ) {
            continue;
        }
        let Some(item) = before.get(hold.id()) else {
            continue;
        };
        if item.enabled || !hold.enabled_before() {
            continue;
        }
        let Ok(digest) = sha256_bytes(&item.digest) else {
            continue;
        };
        if &digest != hold.digest() {
            continue;
        }
        if revoked_for_package(index, &item.ownership, &item.digest).is_some() {
            continue;
        }
        rechecked.insert(hold.id().to_owned());
    }
    rechecked
}

/// Convert `native_mods::Failed::Mods` omissions into per-mod `HeldReason`s so a failing local,
/// source, or unadopted mod is held instead of aborting the entire switch.
fn classify_prove_failure(
    left: &[crate::native_mods::LeftOut],
    compiling: bool,
    tag: &str,
) -> Vec<(String, HeldReason)> {
    use crate::native_mods::Why;
    let mut blamed = Vec::new();
    for omission in left {
        let reason = match &omission.why {
            Why::Unfit(detail) if !compiling => HeldReason::NoFit {
                build: tag.to_owned(),
                detail: Some(detail.clone()),
            },
            Why::Unproved(detail) if !compiling => HeldReason::Unproved {
                build: tag.to_owned(),
                detail: Some(detail.clone()),
            },
            Why::DidNotBuild if compiling => HeldReason::BuildFailed {
                build: tag.to_owned(),
            },
            Why::OtherVersion => HeldReason::NoFit {
                build: tag.to_owned(),
                detail: None,
            },
            Why::Clash(other) => HeldReason::NoFit {
                build: tag.to_owned(),
                detail: Some(other.clone()),
            },
            Why::Needs(_) | Why::Unfit(_) | Why::Unproved(_) | Why::DidNotBuild => continue,
        };
        blamed.push((omission.id.clone(), reason));
    }
    blamed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        instances::{Channel, Kind as InstanceKind},
        native_mods::{Cancellation, LeftOut, Why},
    };

    #[test]
    fn v2_profile_switch_refuses_pack_without_fetching_index() {
        let dir = tempfile::tempdir().unwrap();
        let instance = Instance {
            id: "pack-1".into(),
            name: "Pack".into(),
            home: dir.path().to_path_buf(),
            work: dir.path().to_path_buf(),
            version: None,
            channel: Channel::Dev,
            kind: InstanceKind::Pack,
            group: None,
            icon: None,
            native_profile: Default::default(),
            last_played: None,
            played: Default::default(),
            launch: Default::default(),
            incarnation: None,
        };
        let cancel = Cancellation::default();
        let hooks = V2SwitchHooks {
            switch: SwitchHooks {
                cancellation: &cancel,
                progress: &|_| {},
                approve: &|_| Ok(SwitchDecision::Cancel),
                at_cutoff: &|| Ok(()),
            },
            confirm_publisher_change: None,
            pin_dir: None,
        };
        let Err(err) = switch_profile_build_v2_with_index(
            &instance,
            Identity::of(dir.path()).unwrap(),
            "dev-2026-11-24-abc",
            Some(BuildChoice::Follow(Channel::Dev)),
            hooks,
            || panic!("pack refusal must happen before fetching v2 index"),
        ) else {
            panic!("expected pack refusal error");
        };
        assert!(err.contains("ordinary profile"), "unexpected error: {err}");
    }

    #[test]
    fn classify_prove_failure_maps_unfit_unproved_and_build_failed_to_holds() {
        let left = vec![
            LeftOut {
                id: "local_a".into(),
                version: "1.0.0".into(),
                why: Why::Unfit("API changed".into()),
                detail: None,
            },
            LeftOut {
                id: "local_b".into(),
                version: "1.0.0".into(),
                why: Why::Unproved("no uses list".into()),
                detail: None,
            },
            LeftOut {
                id: "dep_c".into(),
                version: "1.0.0".into(),
                why: Why::Needs("local_a".into()),
                detail: None,
            },
        ];
        let pre = classify_prove_failure(&left, false, "dev-2026-11-24-abc");
        assert_eq!(pre.len(), 2);
        assert!(matches!(
            &pre[0],
            (id, HeldReason::NoFit { build, detail: Some(d) })
                if id == "local_a" && build == "dev-2026-11-24-abc" && d == "API changed"
        ));
        assert!(matches!(
            &pre[1],
            (id, HeldReason::Unproved { build, detail: Some(d) })
                if id == "local_b" && build == "dev-2026-11-24-abc" && d == "no uses list"
        ));

        let compile_left = vec![LeftOut {
            id: "local_native".into(),
            version: "1.0.0".into(),
            why: Why::DidNotBuild,
            detail: None,
        }];
        let during = classify_prove_failure(&compile_left, true, "dev-2026-11-24-abc");
        assert_eq!(during.len(), 1);
        assert!(matches!(
            &during[0],
            (id, HeldReason::BuildFailed { build })
                if id == "local_native" && build == "dev-2026-11-24-abc"
        ));
    }

    #[test]
    fn collect_rechecked_local_holds_reoffers_matching_local_holds_and_skips_registry_holds() {
        let index: Index = serde_json::from_value(serde_json::json!({
            "schema": 2,
            "serial": 1,
            "generated": "2026-11-24T00:00:00Z",
            "builds": [],
            "categories": [],
            "collections": [],
            "projects": [],
            "revoked": []
        }))
        .unwrap();
        let digest_hex = "11".repeat(32);
        let digest_bytes = [0x11u8; 32];
        let local_hold = Held::new(
            "local_mod".into(),
            "Local Mod".into(),
            digest_bytes,
            (HeldRoot::Native, "local_mod.smod".into()),
            HeldSource::Local {
                version: "1.0.0".into(),
            },
            true,
            HeldReason::BuildFailed {
                build: "dev-2026-11-20-old".into(),
            },
        )
        .unwrap();
        let registry_hold = Held::new(
            "reg_mod".into(),
            "Reg Mod".into(),
            digest_bytes,
            (HeldRoot::Content, "reg_mod.smod".into()),
            HeldSource::Registry {
                project: "reg_mod".into(),
                version: "1.0.0".into(),
                repository: None,
            },
            true,
            HeldReason::NoFit {
                build: "dev-2026-11-20-old".into(),
                detail: None,
            },
        )
        .unwrap();
        let pkg = |id: &str| {
            let manifest: sunrise_launcher_smod::manifest::Manifest = toml::from_str(&format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = \"1\"\n"
            ))
            .unwrap();
            crate::managed_play::ScannedPackage {
                path: PathBuf::from(format!("mods/{id}.smod")),
                directory: false,
                manifest,
            }
        };
        let before = BTreeMap::from([
            (
                "local_mod".into(),
                InstalledV2Mod {
                    package: pkg("local_mod"),
                    digest: digest_hex.clone(),
                    size: 0,
                    enabled: false,
                    ownership: V2Ownership::Local,
                },
            ),
            (
                "reg_mod".into(),
                InstalledV2Mod {
                    package: pkg("reg_mod"),
                    digest: digest_hex,
                    size: 0,
                    enabled: false,
                    ownership: V2Ownership::Local,
                },
            ),
        ]);
        let rechecked =
            collect_rechecked_local_holds(&[local_hold, registry_hold], &before, &index);
        assert_eq!(rechecked, BTreeSet::from(["local_mod".to_owned()]));
    }
}
