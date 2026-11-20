//! First Play freezes the candidate and its proof before downloading or preparing anything.
use super::{at, inputs, scan_packages, unsettled, Active, Artifact, ReferencePublication};
use crate::{
    backend::{
        self,
        installation::{GameDownload, GameProof},
    },
    native_mods,
    process::leases::ProfileHandoff,
    profile_txn::{self, Admission, Bootstrap, Staging},
};
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};
use sunrise_launcher_smod::install_lock::{lock_home, HomeLockGuard};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Selection {
    Cached { base: Artifact, preferred: Artifact },
    Download { archive: GameDownload },
}

/// The candidate resolved on the first admitted Play. A committed pointer or an
/// interrupted binding wins over this chooser on every later Play.
pub struct FirstBuild {
    base: String,
    image: String,
    selection: Selection,
}

impl FirstBuild {
    pub fn installed(image: &str) -> Result<Self, String> {
        let base = if native_mods::is_native(image) {
            let data = native_mods::about(image).ok_or_else(unsettled)?;
            if data.target != native_mods::Target::Client || native_mods::is_native(&data.base) {
                return Err(unsettled());
            }
            data.base
        } else {
            image.to_owned()
        };
        let primary = Artifact::capture(&base)?;
        let preferred = if base == image {
            primary.clone()
        } else {
            Artifact::capture(image)?
        };
        Ok(Self {
            base,
            image: image.to_owned(),
            selection: Selection::Cached {
                base: primary,
                preferred,
            },
        })
    }

    pub fn download(release: &backend::Release) -> Result<Self, String> {
        let archive = GameDownload::select(release)?;
        Ok(Self {
            base: release.tag.clone(),
            image: release.tag.clone(),
            selection: Selection::Download { archive },
        })
    }
}

struct Bound {
    base: String,
    image: String,
    profile: [u8; 16],
    inputs: [u8; 32],
    selection: Selection,
    proof: Option<GameProof>,
}

impl Bound {
    fn from_record(record: Bootstrap) -> Result<Self, String> {
        let selection = serde_json::from_value(record.selection).map_err(|e| e.to_string())?;
        let proof = match &selection {
            Selection::Cached { base, preferred } => {
                if record.base != base.tag
                    || record.image != preferred.tag
                    || record.proof.is_some()
                {
                    return Err(unsettled());
                }
                super::validate_tag(&base.tag)?;
                if preferred.tag != base.tag {
                    super::validate_tag(&preferred.tag)?;
                } else if base != preferred {
                    return Err(unsettled());
                }
                None
            }
            Selection::Download { archive } => {
                if record.base != record.image {
                    return Err(unsettled());
                }
                archive.validate(&record.base)?;
                record.proof.map(GameProof::saved).transpose()?
            }
        };
        Ok(Self {
            base: record.base,
            image: record.image,
            profile: record.profile,
            inputs: record.inputs,
            selection,
            proof,
        })
    }

    fn verify_image(&self) -> Result<(), String> {
        match &self.selection {
            Selection::Cached { base, preferred } => {
                if Artifact::capture(&base.tag)? == *base
                    && (base == preferred || Artifact::capture(&preferred.tag)? == *preferred)
                {
                    return Ok(());
                }
            }
            Selection::Download { archive } => {
                if self
                    .proof
                    .as_ref()
                    .is_some_and(|proof| archive.is_installed(&self.base, proof))
                {
                    return Ok(());
                }
            }
        }
        Err(unsettled())
    }
}

pub struct Profile(Admission);

impl Profile {
    /// Acquire profile, then game manifest, then cache publication. Cache scans never
    /// take either profile lock. Keep the same admission through the game handoff.
    pub fn open(home: &Path, external_owner: Option<&Path>) -> Result<Self, String> {
        let profile = Admission::open_fenced(home, external_owner, || {
            let manifest = game_lock(home)?;
            let references = ReferencePublication::begin(home)?;
            Ok((manifest, references))
        })?;
        let _manifest = game_lock(home)?;
        crate::browser::installed::recover_game_install(home)?;
        Ok(Self(profile))
    }

    pub fn home(&self) -> &Path {
        self.0.home()
    }

    pub fn active(&self) -> Result<Option<Active>, String> {
        profile_txn::references(self.home())?
            .pointer
            .as_deref()
            .map(Active::parse)
            .transpose()
    }

    pub fn handoff(&self) -> ProfileHandoff<'_> {
        self.0.handoff()
    }

    pub fn begin_first(
        &mut self,
        choose: impl FnOnce() -> Result<FirstBuild, String>,
        progress: impl Fn(f32),
    ) -> Result<FirstAttempt<'_>, String> {
        let home = self.home().to_path_buf();
        let mut admitted = None;
        let initial = {
            let _manifest = game_lock(&home)?;
            scan_packages(&home)?;
            let epoch = inputs(&home, None)?;
            let initial = self.0.bind_bootstrap(
                || {
                    let choice = choose()?;
                    let mut profile = [0; 16];
                    while profile == [0; 16] {
                        getrandom::fill(&mut profile).map_err(|e| e.to_string())?;
                    }
                    let selection =
                        serde_json::to_value(&choice.selection).map_err(|e| e.to_string())?;
                    Ok(Bootstrap::new(
                        choice.base,
                        choice.image,
                        profile,
                        epoch,
                        selection,
                    ))
                },
                |record| {
                    let guard = ReferencePublication::begin(&home)?;
                    let bound = Bound::from_record(record.clone())?;
                    if matches!(bound.selection, Selection::Cached { .. }) {
                        bound.verify_image()?;
                    }
                    admitted = Some(bound);
                    Ok(guard)
                },
            )?;
            if initial.inputs != epoch {
                return Err(unsettled());
            }
            initial
        };
        let mut bound = admitted.map_or_else(|| Bound::from_record(initial), Ok)?;
        if let Selection::Download { archive } = &bound.selection {
            if bound.proof.is_none() {
                let proof = archive.prove(&bound.base)?;
                let _references = ReferencePublication::begin(&home)?;
                self.0
                    .prove_bootstrap(|_| serde_json::to_value(&proof).map_err(|e| e.to_string()))?;
                bound.proof = Some(proof);
            }
            archive.install(
                &bound.base,
                bound.proof.as_ref().expect("saved download proof"),
                progress,
            )?;
        }
        let stage = {
            let _manifest = game_lock(&home)?;
            let _references = ReferencePublication::begin(&home)?;
            if inputs(&home, None)? != bound.inputs {
                return Err(unsettled());
            }
            bound.verify_image()?;
            self.0.resume_bootstrap()?
        };
        Ok(FirstAttempt { home, bound, stage })
    }
}

pub struct FirstAttempt<'a> {
    home: std::path::PathBuf,
    bound: Bound,
    stage: Staging<'a>,
}

impl FirstAttempt<'_> {
    pub fn base(&self) -> &str {
        &self.bound.base
    }
    pub fn preferred_image(&self) -> &str {
        &self.bound.image
    }
    pub fn handoff(&self) -> ProfileHandoff<'_> {
        self.stage.handoff()
    }

    pub fn prepare(&self, log: &dyn Fn(String)) -> Result<(), String> {
        let base = self.base();
        if backend::check_prepared(base) {
            return Ok(());
        }
        let _prepare = backend::prepare_lock(base)?;
        if backend::check_prepared(base) {
            return Ok(());
        }
        let minecraft = backend::Settings::load()
            .minecraft_dir
            .unwrap_or_else(|| crate::preparer::minecraft_dir(None));
        let (command, minecraft) =
            backend::preparer(base, &minecraft, backend::MinecraftFrom::Install, log)?;
        backend::sounds_staged(base, &minecraft, log, Some(self.handoff()));
        self.run_prepare(command, log)
    }

    fn run_prepare(&self, command: Command, log: &dyn Fn(String)) -> Result<(), String> {
        crate::preparer::run_staged(command, self.handoff(), log)?;
        backend::forget_prepared();
        backend::need_prepared(self.base())
    }

    pub fn exact_image(
        &self,
        mode: native_mods::Profile,
        cancellation: &native_mods::Cancellation,
        progress: impl Fn(native_mods::Step),
        consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
    ) -> Result<String, native_mods::Failed> {
        image_for_stage(
            self.base(),
            self.preferred_image(),
            &self.home,
            &self.stage,
            mode,
            cancellation,
            progress,
            consent,
        )
    }

    /// No failed or partially compiled output enters a generation. Keep the worker's
    /// original native permission revision through the last check and publication.
    pub fn commit(
        self,
        image: &str,
        cancellation: &native_mods::Cancellation,
    ) -> Result<Active, String> {
        if native_mods::is_native(image) {
            cancellation.admit_native_work().map_err(String::from)?;
        }
        let _manifest = game_lock(&self.home)?;
        let active = Active::capture(
            self.bound.profile,
            &self.bound.base,
            image,
            &self.home,
            self.bound.inputs,
            None,
        )?;
        let _references = ReferencePublication::begin(&self.home)?;
        self.bound.verify_image()?;
        let pointer = serde_json::to_vec(&active).map_err(|e| e.to_string())?;
        self.stage.seal(pointer, &[])?.publish(|| {
            active.verify_artifacts()?;
            if inputs(&self.home, None)? != self.bound.inputs {
                return Err(unsettled());
            }
            cancellation.publication_guard()
        })?;
        Ok(active)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn image_for_stage(
    base: &str,
    preferred: &str,
    home: &Path,
    stage: &Staging<'_>,
    mode: native_mods::Profile,
    cancellation: &native_mods::Cancellation,
    progress: impl Fn(native_mods::Step),
    consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
) -> Result<String, native_mods::Failed> {
    if super::verify_included(base, preferred, home).is_ok() {
        if native_mods::is_native(preferred) {
            cancellation.admit_native_work()?;
        }
        return Ok(preferred.to_owned());
    }
    if crate::native_status::enabled(home).is_empty() {
        return Ok(base.to_owned());
    }
    native_mods::build_exact_client(base, home, mode, stage, cancellation, progress, consent)
}

pub(super) fn game_lock(home: &Path) -> Result<HomeLockGuard, String> {
    lock_home(home).map_err(|e| at(home, e))
}

#[cfg(test)]
mod tests;
