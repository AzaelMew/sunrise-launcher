//! Exact same-base installs use the same private preparation and durable publication as Switch.
use super::{
    build, durable,
    switch::{self, CandidateBuild},
    unsettled, Active, Image, Lease, Target,
};
use crate::{
    browser::{installed, sunrise, Kind, SourceId},
    github_mods,
    i18n::{tr, trf},
    instances::Instance,
    native_mods, worlds,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher_smod::{index, mod_list};

/// The profile stays admitted while a complete candidate and optional sidecars are prepared.
pub(in crate::browser::sunrise) struct StagedInstall {
    owner: Arc<Lease>,
    basis: Basis,
    input: [u8; 32],
    before: switch::Inventory,
    stage: PathBuf,
    published: bool,
}

enum Basis {
    Committed(Active),
    First(build::Bootstrap),
}

pub(in crate::browser::sunrise) type RegistryChanges = BTreeMap<String, (String, BTreeSet<String>)>;

#[derive(Default)]
struct Retention<'a> {
    explicitly_off: &'a [String],
    compile_failed: BTreeMap<String, build::WaitingMod>,
}

impl StagedInstall {
    pub fn for_instance(
        owner: Arc<Lease>,
        instance: &Instance,
        download: impl Fn(&str, f32),
    ) -> Result<Self, String> {
        if owner.home() != instance.home {
            return Err(unsettled());
        }
        match owner.active_build()? {
            Some(active) => Self::begin(owner, active),
            None => {
                let bound = owner.bind_bootstrap(|| Target::for_profile(instance))?;
                owner.prepare_bound(&bound, |fraction| download(bound.base(), fraction))?;
                Self::first(owner, &bound)
            }
        }
    }

    pub fn begin(owner: Arc<Lease>, prior: Active) -> Result<Self, String> {
        prior.verify_artifacts()?;
        Self::copy_current(owner, |owner| {
            Ok((owner.reserve_install_set(&prior)?, Basis::Committed(prior)))
        })
    }

    pub fn first(owner: Arc<Lease>, bound: &Target) -> Result<Self, String> {
        Self::copy_current(owner, |owner| {
            let (root, binding) = owner.reserve_first_install(bound)?;
            Ok((root, Basis::First(binding)))
        })
    }

    fn copy_current(
        owner: Arc<Lease>,
        reserve: impl FnOnce(&Lease) -> Result<(PathBuf, Basis), String>,
    ) -> Result<Self, String> {
        let input = build::inputs(owner.home())?;
        let before = switch::Inventory::read(owner.home())?;
        let (root, basis) = reserve(&owner)?;
        let stage = root.join("candidate");
        let plan = Self {
            owner,
            basis,
            input,
            before,
            stage,
            published: false,
        };
        fs::create_dir(&plan.stage).map_err(|e| at(&plan.stage, e))?;
        for folder in [mod_list::DIR, mod_list::NATIVE_DIR] {
            let from = plan.owner.home().join(folder);
            match fs::symlink_metadata(&from) {
                Ok(_) => switch::copy_tree(&from, &plan.stage.join(folder), false)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(at(&from, e)),
            }
        }
        for name in [installed::MANIFEST, mod_list::FILE] {
            if let Some(bytes) = build::optional_bytes(&plan.owner.home().join(name))? {
                write(&plan.stage.join(name), &bytes)?;
            }
        }
        if build::inputs(&plan.stage)? != input || build::inputs(plan.owner.home())? != input {
            return Err(unsettled());
        }
        Ok(plan)
    }

    pub fn stage(&self) -> &Path {
        &self.stage
    }

    pub fn package(&self, id: &str) -> Option<(Option<&installed::Entry>, &str)> {
        self.before
            .packages
            .get(id)
            .map(|pkg| (pkg.row.as_ref(), pkg.digest.as_str()))
    }

    pub fn base(&self) -> &str {
        match &self.basis {
            Basis::Committed(prior) => prior.base(),
            Basis::First(binding) => binding.target.base(),
        }
    }

    pub fn prepare(
        self,
        index: &index::Index,
        sidecars: &[&str],
        profile: native_mods::Profile,
        progress: impl Fn(native_mods::Step),
        registry_consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
    ) -> Result<ReadyInstall, native_mods::Failed> {
        let compiler = CandidateBuild {
            profile,
            cancellation: native_mods::Cancellation::for_profile(self.owner.clone()),
            registry_consent,
        };
        self.prepare_with(index, sidecars, compiler, progress)
    }

    pub(super) fn prepare_with(
        self,
        index: &index::Index,
        sidecars: &[&str],
        compiler: CandidateBuild<'_>,
        progress: impl Fn(native_mods::Step),
    ) -> Result<ReadyInstall, native_mods::Failed> {
        let sidecars = sidecars
            .iter()
            .map(|name| (PathBuf::from(name), Some(self.stage.join(name))))
            .collect();
        let after = self.proposed_inventory(index)?;
        self.seal(after, sidecars, Retention::default(), compiler, progress)
    }

    pub(super) fn inputs(&self) -> [u8; 32] {
        self.input
    }

    /// Which requested roots reach each newly staged package. An unchanged preimage must
    /// never be removed from the live profile to make a failing candidate work.
    pub(in crate::browser::sunrise) fn registry_candidate(
        &self,
        index: &index::Index,
        requested: &[&str],
        omitted: &BTreeSet<(String, String)>,
    ) -> Result<Option<RegistryChanges>, String> {
        let inventory = self.proposed_inventory(index)?;
        if !switch::fits(&chosen(&inventory))
            || inventory
                .packages
                .iter()
                .any(|(id, pkg)| omitted.contains(&(id.clone(), pkg.digest.clone())))
        {
            return Ok(None);
        }
        let mut changes = BTreeMap::new();
        for &root in requested {
            let mut seen = BTreeSet::new();
            let mut next = vec![root];
            while let Some(id) = next.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let pkg = inventory.packages.get(id).ok_or_else(unsettled)?;
                if self
                    .before
                    .packages
                    .get(id)
                    .is_none_or(|old| old.digest != pkg.digest)
                {
                    changes
                        .entry(id.to_owned())
                        .or_insert_with(|| (pkg.digest.clone(), BTreeSet::new()))
                        .1
                        .insert(root.to_owned());
                }
                next.extend(
                    pkg.manifest
                        .dependencies
                        .iter()
                        .filter(|(_, dependency)| !dependency.details().optional)
                        .map(|(id, _)| id.as_str()),
                );
            }
        }
        Ok(Some(changes))
    }

    fn proposed_inventory(&self, index: &index::Index) -> Result<switch::Inventory, String> {
        let after = switch::Inventory::read(&self.stage)?;
        for (id, pkg) in &after.packages {
            if pkg.directory {
                continue;
            }
            let origin = pkg.row.as_ref().map(|row| row.project.as_str());
            if let Some(revoked) =
                crate::browser::sunrise::package_revocation(index, id, &pkg.digest, origin)
                    .filter(|_| pkg.on)
            {
                return Err(trf(
                    "{name} was taken down: {reason}",
                    &[("name", &pkg.manifest.name), ("reason", &revoked.reason)],
                ));
            }
        }
        Ok(after)
    }

    pub(super) fn installed_compile_failures(
        &self,
        left: &[native_mods::LeftOut],
    ) -> Option<Vec<native_mods::BuildFailure>> {
        let mut failed = BTreeMap::new();
        for item in left {
            match item.why {
                native_mods::Why::Needs(_) => (),
                native_mods::Why::DidNotBuild
                    if item
                        .detail
                        .as_ref()?
                        .log
                        .as_ref()
                        .is_some_and(|log| !log.is_empty()) =>
                {
                    let pkg = self.before.packages.get(&item.id)?;
                    if !pkg.on
                        || pkg.manifest.native.is_none()
                        || pkg.manifest.version != item.version
                    {
                        return None;
                    }
                    let failure = native_mods::build_failures(std::slice::from_ref(item)).pop()?;
                    failed.insert(item.id.as_str(), failure);
                }
                _ => return None,
            }
        }
        if failed.is_empty() {
            return None;
        }
        // The compiler knows compiled crates; include enabled content-only dependents from
        // the actual package manifests too. None may be silently turned off.
        loop {
            let old = failed.len();
            for (id, pkg) in &self.before.packages {
                if !pkg.on || failed.contains_key(id.as_str()) {
                    continue;
                }
                if let Some((needs, _)) = pkg.manifest.dependencies.iter().find(|(dep, rule)| {
                    !rule.details().optional && failed.contains_key(dep.as_str())
                }) {
                    failed.insert(
                        id,
                        native_mods::BuildFailure {
                            id: id.clone(),
                            reason: trf("It needs {mod}, which didn't build.", &[("mod", needs)]),
                            log: String::new(),
                        },
                    );
                }
            }
            if failed.len() == old {
                break;
            }
        }
        Some(failed.into_values().collect())
    }

    /// Keep the exact failed installed bytes but prove the enabled survivors. Unlike a
    /// deliberate single-mod toggle, every disabled package participates in world impact.
    pub(super) fn after_compile_failure(
        self,
        index: &index::Index,
        failed: &[native_mods::BuildFailure],
        compiler: CandidateBuild<'_>,
        progress: impl Fn(native_mods::Step),
    ) -> Result<ReadyInstall, String> {
        let mut retention = Retention::default();
        for failure in failed {
            let pkg = self
                .before
                .packages
                .get(&failure.id)
                .filter(|pkg| pkg.on)
                .ok_or_else(unsettled)?;
            let Image::Sha256(digest) = Image::at(&self.stage.join(&pkg.relative))? else {
                return Err(unsettled());
            };
            retention.compile_failed.insert(
                failure.id.clone(),
                build::WaitingMod {
                    file: pkg.relative.clone(),
                    digest,
                    reason: failure.reason.clone(),
                    was_on: true,
                },
            );
        }
        let ids: Vec<_> = failed.iter().map(|failure| failure.id.as_str()).collect();
        crate::browser::mod_list::set_all_enabled(&self.stage, &ids, false)?;
        let after = self.proposed_inventory(index)?;
        self.seal(after, Vec::new(), retention, compiler, progress)
            .map_err(Into::into)
    }

    fn seal(
        self,
        after: switch::Inventory,
        sidecars: Vec<(PathBuf, Option<PathBuf>)>,
        retention: Retention<'_>,
        compiler: CandidateBuild<'_>,
        progress: impl Fn(native_mods::Step),
    ) -> Result<ReadyInstall, native_mods::Failed> {
        let waiting = build::waiting_packages(self.owner.home())?;
        let mut verified = chosen(&after);
        for (id, item) in waiting {
            if item.matches(&self.stage)? {
                if after.packages.get(&id).is_some_and(|pkg| pkg.on) {
                    return Err(unsettled().into());
                }
                // An unrelated Install neither removes nor tries to build the exact bytes
                // already disabled by Switch. A different replacement must be proved as usual.
                verified.remove(id.as_str());
            }
        }
        if !switch::fits(&verified) {
            return Err(tr("The proposed mod set has incompatible packages or missing dependencies. Nothing was installed.").to_owned().into());
        }
        let prior_failures = match &self.basis {
            Basis::Committed(prior) => Some(&prior.compile_failed),
            Basis::First(_) => None,
        };
        let mut excluded = BTreeSet::new();
        for (id, failure) in prior_failures
            .into_iter()
            .flatten()
            .chain(&retention.compile_failed)
        {
            if after.packages.get(id).is_some_and(|pkg| !pkg.on) && failure.matches(&self.stage)? {
                excluded.insert(id.as_str());
            }
        }
        if !excluded.is_empty() {
            // Off packages depending on a known failure cannot be proved in isolation either.
            loop {
                let old = excluded.len();
                for (&id, pkg) in &verified {
                    if !pkg.on
                        && pkg.manifest.dependencies.iter().any(|(dep, rule)| {
                            !rule.details().optional && excluded.contains(dep.as_str())
                        })
                    {
                        excluded.insert(id);
                    }
                }
                if excluded.len() == old {
                    break;
                }
            }
            verified.retain(|id, _| !excluded.contains(id));
        }
        let image =
            switch::build_candidate(self.base(), &self.stage, &compiler, &progress, &verified)?;
        let home = self.owner.home();
        let changed: Vec<_> = self
            .before
            .packages
            .iter()
            .filter(|(id, old)| {
                after.packages.get(*id).is_none_or(|new| {
                    old.on
                        && !retention.explicitly_off.contains(id)
                        && (!new.on || new.digest != old.digest)
                })
            })
            .map(|(id, old)| worlds::switch::Changed {
                id,
                title: &old.manifest.name,
                namespaces: &old.manifest.namespaces,
            })
            .collect();
        let (fence, question) = if changed.is_empty() {
            (None, worlds::SwitchQuestion::default())
        } else {
            let (fence, question) = worlds::switch::Fence::hold(home, &changed, None)?;
            (Some(fence), question)
        };
        if after.browser != self.before.browser {
            // Keep raw unrelated rows and foreign metadata on records we didn't change.
            // The planner's typed view is for choosing packages, not rewriting those rows.
            let mut browser = self.before.browser.clone();
            let rows = browser["entries"].as_array_mut().ok_or_else(unsettled)?;
            rows.retain(|row| {
                !self.before.packages.iter().any(|(id, old)| {
                    old.row
                        .as_ref()
                        .is_some_and(|entry| row["file"].as_str() == Some(entry.file.as_str()))
                        && after.packages.get(id).and_then(|new| new.row.as_ref())
                            != old.row.as_ref()
                })
            });
            for (id, pkg) in &after.packages {
                if let Some(row) = &pkg.row {
                    if self.before.packages.get(id).and_then(|p| p.row.as_ref()) != Some(row) {
                        rows.push(serde_json::to_value(row).map_err(|e| e.to_string())?);
                    }
                }
            }
            write(
                &self.stage.join(installed::MANIFEST),
                &serde_json::to_vec(&browser).map_err(|e| e.to_string())?,
            )?;
        }
        let candidate_inputs = build::inputs(&self.stage)?;
        let mut next = match &self.basis {
            Basis::Committed(prior) => prior.adopt(&image, candidate_inputs, &self.stage)?,
            Basis::First(binding) => {
                binding.first_generation(&image, &self.stage, candidate_inputs)?
            }
        };
        next.compile_failed.extend(retention.compile_failed);
        if build::inputs(home)? != self.input {
            return Err(unsettled().into());
        }
        let mut steps = switch::changes(home, &self.stage, &self.before, &chosen(&after))?;
        steps.extend(sidecars);
        self.owner.seal_staged_set(&next, &steps)?;
        Ok(ReadyInstall {
            install: self,
            next,
            cancellation: compiler.cancellation,
            fence,
            question,
        })
    }
}

/// An explicit player edit, not an instruction to find a new version or omit a dependent.
pub enum ModEdit {
    SetEnabled { ids: Vec<String>, enabled: bool },
    Remove(Vec<ModRemoval>),
}

pub struct ModRemoval {
    id: String,
    sha256: String,
    origin: RemovalOrigin,
}

enum RemovalOrigin {
    Registry(String),
    Github,
}

impl ModRemoval {
    pub fn registry(entry: &installed::Entry) -> Result<Self, String> {
        let id = sunrise::mod_id(entry)
            .filter(|_| entry.source == SourceId::Sunrise && entry.kind == Kind::Mod)
            .ok_or_else(unsettled)?;
        let sha256 = entry
            .checksum
            .strip_prefix("SHA-256:")
            .ok_or_else(unsettled)?;
        Ok(Self {
            id: id.into(),
            sha256: sha256.into(),
            origin: RemovalOrigin::Registry(entry.project.clone()),
        })
    }

    pub fn github(origin: &github_mods::Origin) -> Self {
        Self {
            id: origin.id.clone(),
            sha256: origin.sha256.clone(),
            origin: RemovalOrigin::Github,
        }
    }
}

pub struct PreparedModEdit {
    ready: ReadyInstall,
    index: String,
}

impl PreparedModEdit {
    pub fn question(&self) -> &worlds::SwitchQuestion {
        self.ready.question()
    }

    pub fn commit(self, approved: bool) -> Result<(), String> {
        self.ready.commit(approved, || {
            let (_, current) = sunrise::authoritative_index()?;
            if current != self.index {
                return Err(tr("The mod registry changed while the edit was being prepared. Nothing was changed.").into());
            }
            Ok(())
        })
    }
}

pub fn prepare_mod_edit(
    owner: Arc<Lease>,
    instance: &Instance,
    edit: ModEdit,
    progress: &dyn Fn(&str, f32),
    registry_consent: &dyn Fn(&native_mods::Consent) -> bool,
) -> Result<PreparedModEdit, String> {
    let (index, evidence) = sunrise::authoritative_index()?;
    let plan = StagedInstall::for_instance(owner, instance, progress)?;
    let ready = plan.edit(
        edit,
        &index,
        instance.native_profile,
        |step| progress(&step.label(), 0.99),
        Some(registry_consent),
    )?;
    Ok(PreparedModEdit {
        ready,
        index: evidence,
    })
}

impl StagedInstall {
    pub(in crate::browser::sunrise) fn edit(
        self,
        edit: ModEdit,
        index: &index::Index,
        profile: native_mods::Profile,
        progress: impl Fn(native_mods::Step),
        registry_consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
    ) -> Result<ReadyInstall, String> {
        let mut sidecars = Vec::new();
        let mut requested_off = Vec::new();
        match edit {
            ModEdit::SetEnabled { ids, enabled } => {
                if ids.is_empty() || ids.iter().any(|id| !self.before.packages.contains_key(id)) {
                    return Err(unsettled());
                }
                let refs: Vec<_> = ids.iter().map(String::as_str).collect();
                crate::browser::mod_list::set_all_enabled(&self.stage, &refs, enabled)?;
                if !enabled {
                    requested_off = ids;
                }
            }
            ModEdit::Remove(removals) => {
                if removals.is_empty() {
                    return Err(unsettled());
                }
                let mut removed = BTreeSet::new();
                let mut browser = self.before.browser.clone();
                let rows = browser["entries"].as_array_mut().ok_or_else(unsettled)?;
                let mut changed = false;
                for removal in &removals {
                    let pkg = self
                        .before
                        .packages
                        .get(&removal.id)
                        .ok_or_else(unsettled)?;
                    if !removed.insert(removal.id.as_str())
                        || !removal.sha256.eq_ignore_ascii_case(&pkg.digest)
                    {
                        return Err(tr("The selected mod changed before it could be removed. Nothing was changed.").into());
                    }
                    let file = self.stage.join(&pkg.relative);
                    match &removal.origin {
                        RemovalOrigin::Registry(project) => {
                            let row = pkg
                                .row
                                .as_ref()
                                .filter(|row| &row.project == project)
                                .ok_or_else(unsettled)?;
                            rows.retain(|entry| entry["file"].as_str() != Some(row.file.as_str()));
                            changed = true;
                        }
                        RemovalOrigin::Github => {
                            let origin_path = github_mods::origin_path(Path::new(""), &removal.id);
                            let path = self.stage.join(&origin_path);
                            let bytes = build::optional_bytes(&path)?.ok_or_else(unsettled)?;
                            let origin: github_mods::Origin =
                                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                            if origin.id != removal.id
                                || origin.sha256 != pkg.digest
                                || pkg.row.is_some()
                                || file != path.with_file_name(format!("{}.smod", removal.id))
                            {
                                return Err(unsettled());
                            }
                            fs::remove_file(&path).map_err(|e| at(&path, e))?;
                            sidecars.push((origin_path, None));
                        }
                    }
                    fs::remove_file(&file).map_err(|e| at(&file, e))?;
                }
                let ids: Vec<_> = removed.into_iter().collect();
                crate::browser::mod_list::set_all_enabled(&self.stage, &ids, false)?;
                if changed {
                    write(
                        &self.stage.join(installed::MANIFEST),
                        &serde_json::to_vec(&browser).map_err(|e| e.to_string())?,
                    )?;
                }
            }
        }
        let after = self.proposed_inventory(index)?;
        let compiler = CandidateBuild {
            profile,
            cancellation: native_mods::Cancellation::for_profile(self.owner.clone()),
            registry_consent,
        };
        self.seal(
            after,
            sidecars,
            Retention {
                explicitly_off: &requested_off,
                ..Retention::default()
            },
            compiler,
            progress,
        )
        .map_err(Into::into)
    }
}

/// Approval consumes the exact candidate that was built and shown, rather than retrying
/// the request. Dropping it (including closing the question) retires the private attempt.
pub(in crate::browser::sunrise) struct ReadyInstall {
    install: StagedInstall,
    next: Active,
    cancellation: native_mods::Cancellation,
    fence: Option<worlds::switch::Fence>,
    question: worlds::SwitchQuestion,
}

impl ReadyInstall {
    pub(super) fn image(&self) -> &str {
        self.next.executable()
    }

    pub fn question(&self) -> &worlds::SwitchQuestion {
        &self.question
    }

    pub(super) fn executable(&self) -> &str {
        self.next.executable()
    }

    pub fn commit(
        mut self,
        approved: bool,
        revalidate: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if !approved && self.question.needed() {
            return Err(self.question.lines().join("\n"));
        }
        let home = self.install.owner.home();
        revalidate()?;
        self.install.owner.publish_staged_set(|| {
            if build::inputs(home)? != self.install.input {
                return Err(unsettled());
            }
            if let Some(fence) = &self.fence {
                fence.revalidate(home)?;
            }
            match &self.install.basis {
                Basis::Committed(prior) => prior.verify_artifacts()?,
                Basis::First(binding) => binding.verify_artifact()?,
            }
            self.next.verify_artifacts()?;
            self.next.verify_resources()?;
            self.cancellation.publication_guard(self.next.executable())
        })?;
        self.install.published = true;
        Ok(())
    }
}

impl Drop for StagedInstall {
    fn drop(&mut self) {
        if !self.published {
            if let Err(error) = self.owner.recover_abandoned() {
                eprintln!("{error}");
            }
        }
    }
}

/// The verified contents of a private profile, without any live handle inside its tree.
pub(in crate::browser::sunrise) struct SealedProfile {
    active: Active,
    native_grant: Option<crate::backend::NativeGrant>,
}

/// Seal a new private profile before its name becomes visible. The caller keeps the lifecycle
/// gate and rechecks the artifact identities under the publication lock before rename.
pub(in crate::browser::sunrise) fn seal_private_profile(
    owner: Arc<Lease>,
    base: &str,
    profile: native_mods::Profile,
    progress: impl Fn(native_mods::Step),
    registry_consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
) -> Result<SealedProfile, native_mods::Failed> {
    let home = owner.home();
    let inventory = switch::Inventory::read(home)?;
    let chosen = chosen(&inventory);
    let target = Target::legacy(base)?;
    let compiler = CandidateBuild {
        profile,
        cancellation: native_mods::Cancellation::for_profile(owner.clone()),
        registry_consent,
    };
    let image = verify_candidate(target.base(), home, &compiler, &progress, &chosen)?;
    let binding = build::Bootstrap::bind(home, target)?;
    let active = binding.active(&image, home)?;
    build::publish(home, &active)?;
    Ok(SealedProfile {
        active,
        native_grant: compiler.cancellation.grant()?,
    })
}

pub(in crate::browser::sunrise) fn expose_private_profile(
    stage: &Path,
    destination: &Path,
    sealed: &SealedProfile,
) -> Result<(), String> {
    let _name = Lease::reserve_name(&destination.join("home"))?;
    let _publication = crate::run_leases::installation().map_err(native_mods::cache_error)?;
    let game_root = destination
        .parent()
        .and_then(Path::parent)
        .ok_or_else(unsettled)?;
    super::artifact_roots::register_root(game_root)?;
    sealed.active.verify_artifacts()?;
    sealed.active.verify_resources()?;
    durable::flush_tree(stage).map_err(|e| at(stage, e))?;
    let _permission =
        crate::native_status::publication_guard(sealed.active.executable(), sealed.native_grant)?;
    durable::move_new_directory(stage, destination).map_err(|e| at(destination, e))
}

pub(in crate::browser::sunrise) fn detach_private_profile(
    source: &Path,
    trash: &Path,
) -> Result<(), String> {
    let _publication = crate::run_leases::installation().map_err(native_mods::cache_error)?;
    durable::move_new_directory(source, trash).map_err(|e| at(source, e))
}

fn chosen(inventory: &switch::Inventory) -> BTreeMap<&str, &switch::Package> {
    inventory
        .packages
        .iter()
        .map(|(id, pkg)| (id.as_str(), pkg))
        .collect()
}

fn verify_candidate(
    base: &str,
    home: &Path,
    compiler: &CandidateBuild<'_>,
    progress: &impl Fn(native_mods::Step),
    chosen: &BTreeMap<&str, &switch::Package>,
) -> Result<String, native_mods::Failed> {
    if !switch::fits(chosen) {
        return Err(tr("The proposed mod set has incompatible packages or missing dependencies. Nothing was installed.").to_owned().into());
    }
    switch::build_candidate(base, home, compiler, progress, chosen)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| at(parent, e))?;
    }
    fs::write(path, bytes).map_err(|e| at(path, e))
}

fn at(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}
