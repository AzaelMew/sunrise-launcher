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

    pub fn commit(
        mut self,
        sidecars: &[&str],
        profile: native_mods::Profile,
        progress: impl Fn(native_mods::Step),
    ) -> Result<(), String> {
        let after = switch::Inventory::read(&self.stage)?;
        let included = chosen(&after);
        let cancellation = native_mods::Cancellation::for_profile(self.owner.clone());
        let image = verify_candidate(
            self.prior.base(),
            &self.stage,
            profile,
            &cancellation,
            &progress,
            &included,
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
        let fence = if changed.is_empty() {
            None
        } else {
            let (fence, question) =
                worlds::switch::Fence::hold(home, &changed, None, None, |_| None)?;
            if !question.affected.is_empty() {
                return Err(question.lines().join("\n"));
            }
            Some(fence)
        };
        let next = self
            .prior
            .adopt(&image, build::inputs(&self.stage)?, &self.stage)?;
        if build::inputs(home)? != self.input {
            return Err(unsettled());
        }
        let mut steps = switch::changes(home, &self.stage, &self.before, &included)?;
        for name in sidecars {
            steps.push((PathBuf::from(name), Some(self.stage.join(name))));
        }
        self.owner.seal_staged_set(&next, &steps)?;
        self.owner.publish_staged_set(|| {
            if let Some(fence) = &fence {
                fence.revalidate(home)?;
            }
            self.prior.verify_artifacts()?;
            next.verify_artifacts()?;
            cancellation.publication_guard(next.executable())
        })?;
        self.published = true;
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
