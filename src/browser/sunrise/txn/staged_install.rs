//! Exact same-base installs use the same private preparation and durable publication as Switch.
use super::{build, durable, switch, unsettled, Active, Lease, Target};
use crate::{browser::installed, i18n::tr, native_mods, worlds};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher_smod::mod_list;

/// The profile stays admitted while a complete candidate and optional sidecars are prepared.
pub(in crate::browser::sunrise) struct StagedInstall {
    owner: Arc<Lease>,
    prior: Active,
    input: [u8; 32],
    before: switch::Inventory,
    stage: PathBuf,
    published: bool,
}

impl StagedInstall {
    pub fn begin(owner: Arc<Lease>, prior: Active) -> Result<Self, String> {
        prior.verify_artifacts()?;
        let before = switch::Inventory::read(owner.home())?;
        let input = build::inputs(owner.home())?;
        let root = owner.reserve_install_set(&prior)?;
        let stage = root.join("candidate");
        let plan = Self {
            owner,
            prior,
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
        Ok(plan)
    }

    pub fn stage(&self) -> &Path {
        &self.stage
    }

    pub fn prepare(
        self,
        sidecars: &[&str],
        profile: native_mods::Profile,
        progress: impl Fn(native_mods::Step),
    ) -> Result<ReadyInstall, String> {
        let after = switch::Inventory::read(&self.stage)?;
        let waiting = build::waiting_packages(self.owner.home())?;
        let mut verified = chosen(&after);
        for (id, item) in waiting {
            if item.matches(&self.stage)? {
                if after.packages.get(&id).is_some_and(|pkg| pkg.on) {
                    return Err(unsettled());
                }
                // An unrelated Install neither removes nor tries to build the exact bytes
                // already disabled by Switch. A different replacement must be proved as usual.
                verified.remove(id.as_str());
            }
        }
        let cancellation = native_mods::Cancellation::for_profile(self.owner.clone());
        let image = verify_candidate(
            self.prior.base(),
            &self.stage,
            profile,
            &cancellation,
            &progress,
            &verified,
        )?;
        let home = self.owner.home();
        let changed: Vec<_> = self
            .before
            .packages
            .iter()
            .filter(|(id, old)| {
                old.on
                    && after
                        .packages
                        .get(*id)
                        .is_none_or(|new| !new.on || new.digest != old.digest)
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
        let next = self
            .prior
            .adopt(&image, build::inputs(&self.stage)?, &self.stage)?;
        if build::inputs(home)? != self.input {
            return Err(unsettled());
        }
        let mut steps = switch::changes(home, &self.stage, &self.before, &chosen(&after))?;
        for name in sidecars {
            steps.push((PathBuf::from(name), Some(self.stage.join(name))));
        }
        self.owner.seal_staged_set(&next, &steps)?;
        Ok(ReadyInstall {
            install: self,
            next,
            cancellation,
            fence,
            question,
        })
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
    pub fn question(&self) -> &worlds::SwitchQuestion {
        &self.question
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
            self.install.prior.verify_artifacts()?;
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
) -> Result<SealedProfile, String> {
    let home = owner.home();
    let inventory = switch::Inventory::read(home)?;
    let chosen = chosen(&inventory);
    let target = Target::legacy(base)?;
    let cancellation = native_mods::Cancellation::for_profile(owner.clone());
    let image = verify_candidate(
        target.base(),
        home,
        profile,
        &cancellation,
        &progress,
        &chosen,
    )?;
    let binding = build::Bootstrap::bind(home, target)?;
    let active = binding.active(&image, home)?;
    build::publish(home, &active)?;
    Ok(SealedProfile {
        active,
        native_grant: cancellation.grant()?,
    })
}

pub(in crate::browser::sunrise) fn expose_private_profile(
    stage: &Path,
    destination: &Path,
    sealed: &SealedProfile,
) -> Result<(), String> {
    let _publication = crate::run_leases::installation().map_err(native_mods::cache_error)?;
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
    profile: native_mods::Profile,
    cancellation: &native_mods::Cancellation,
    progress: &impl Fn(native_mods::Step),
    chosen: &BTreeMap<&str, &switch::Package>,
) -> Result<String, String> {
    if !switch::fits(chosen) {
        return Err(tr("The proposed mod set has incompatible packages or missing dependencies. Nothing was installed.").into());
    }
    switch::build_candidate(base, home, profile, cancellation, progress, chosen)
        .map_err(String::from)
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
