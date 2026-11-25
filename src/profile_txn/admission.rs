//! Admission to a profile, and the stage an admitted profile opens and seals.
use super::*;
use std::time::{Duration, Instant};

/// An admitted profile. The owner file must be stable and never removed, including across
/// detach/recreate; managed names pass a path outside the directory they can move. Its inherited
/// file description outlives a launcher killed while a game child still holds it.
pub struct Admission {
    home: PathBuf,
    identity: Identity,
    pub(super) gate: NameGate,
}

/// An exclusive claim on a stable name, including before its first directory is visible.
pub(crate) struct NameGate {
    pub(super) file: fs::File,
    #[cfg(unix)]
    handed_off: std::sync::atomic::AtomicBool,
}

#[cfg(unix)]
impl Drop for NameGate {
    fn drop(&mut self) {
        if !*self.handed_off.get_mut() {
            // An unrelated concurrent fork may temporarily hold a copy until exec.
            // It was never given admission. Only an intentional handoff needs close-only.
            let _ = self.file.unlock();
        }
    }
}
impl NameGate {
    pub(crate) fn open(gate: &Path) -> Result<Self, String> {
        Self::try_open(gate)?.ok_or_else(profile_in_use)
    }

    /// `None` means the gate is held, not that the profile is missing or invalid.
    fn try_open(gate: &Path) -> Result<Option<Self>, String> {
        match fs::symlink_metadata(gate) {
            Ok(meta) if regular(&meta) && meta.is_file() => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            _ => return Err(at(gate, unsettled())),
        }
        let mut open = fs::File::options();
        open.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.share_mode(0); // The Windows share exclusion follows inherited handles.
        }
        let file = match open.open(gate) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(None),
            Err(error) => return Err(at(gate, error)),
        };
        #[cfg(unix)]
        match file.try_lock() {
            Ok(()) => (),
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => return Err(at(gate, e)),
        }
        Ok(Some(Self {
            file,
            #[cfg(unix)]
            handed_off: std::sync::atomic::AtomicBool::new(false),
        }))
    }
}

impl Admission {
    /// Recovery can retire or restore references. Managed callers fence cache deletion
    /// around admission; no cache guard is held by this lease while a worker runs.
    pub fn open(home: &Path, external_owner: Option<&Path>) -> Result<Self, String> {
        Self::open_fenced(home, external_owner, |_| Ok(()))
    }

    /// Acquire the caller's reference guards after the profile, never the reverse.
    pub(crate) fn open_fenced<G>(
        home: &Path,
        external_owner: Option<&Path>,
        fence: impl FnOnce(Identity) -> Result<G, String>,
    ) -> Result<Self, String> {
        let lease = Self::claim(home, external_owner)?;
        let _fence = fence(lease.identity)?;
        lease.recover()?;
        Ok(lease)
    }

    pub(crate) fn claim(home: &Path, external_owner: Option<&Path>) -> Result<Self, String> {
        Self::try_claim(home, external_owner)?.ok_or_else(profile_in_use)
    }

    /// After a confirmed game exit, the child's inherited gate can still be closing.
    /// Only completion waits; a new Play or profile change still reports contention immediately.
    pub(crate) fn claim_after_exit(
        home: &Path,
        external_owner: Option<&Path>,
    ) -> Result<Self, String> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if let Some(profile) = Self::try_claim(home, external_owner)? {
                return Ok(profile);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(profile_in_use());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }

    fn try_claim(home: &Path, external_owner: Option<&Path>) -> Result<Option<Self>, String> {
        let identity = Identity::of(home)?;
        let gate = match external_owner {
            Some(path) => NameGate::try_open(path)?,
            None => NameGate::try_open(&home.join(".install-txn.lock"))?,
        };
        let Some(gate) = gate else {
            return Ok(None);
        };
        if Identity::of(home)? != identity {
            return Err(unsettled());
        }
        Ok(Some(Self {
            home: home.into(),
            identity,
            gate,
        }))
    }

    /// Give a game child the gate of this already-admitted profile. The game spawn
    /// must carry its cache claims and this profile gate in the same handoff.
    pub fn handoff(&self) -> crate::process::leases::ProfileHandoff<'_> {
        crate::process::leases::ProfileHandoff {
            file: &self.gate.file,
            #[cfg(unix)]
            handed_off: &self.gate.handed_off,
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub(crate) fn identity(&self) -> Identity {
        self.identity
    }

    /// Reserve the candidate artifacts before exposing them. Hold the reference guard
    /// while creating the reservation, not while compiling the private candidate.
    pub fn reserve(
        &mut self,
        intent: Intent,
        targets: BTreeSet<String>,
    ) -> Result<Staging<'_>, String> {
        if read(&self.home)?.is_some() {
            return Err(unsettled());
        }
        self.stage(Record::new(self.identity, intent, targets, None))
    }

    pub(crate) fn has_bootstrap(&self) -> Result<bool, String> {
        match read(&self.home)? {
            Some(record) => record.pending_bootstrap().map(|_| true),
            None => Ok(false),
        }
    }

    /// Release a failed first Play's choice only when nothing was published. A sealed
    /// operation still needs journal recovery; an unsealed stage can still have a worker.
    pub(crate) fn release_failed_bootstrap(&self) -> Result<(), String> {
        let Some(mut record) = read(&self.home)? else {
            return Ok(());
        };
        if record.bootstrap.is_none()
            || record
                .operation
                .as_ref()
                .is_some_and(|op| op.ready.is_some())
        {
            return Ok(());
        }
        if pointer_bytes(&self.home)?.is_some() {
            return Err(unsettled());
        }
        if record.operation.is_some() {
            record.bootstrap = None;
            write(&self.home, &record)
        } else {
            record.clean_binding_stage(&self.home)?;
            let path = self.home.join(RECORD);
            durable::retire(&path).map_err(|e| at(&path, e))
        }
    }

    /// Resolve first Play once. A pending binding, with its exact inputs and proof, wins
    /// over a subsequent change to the release feed or profile defaults.
    pub fn bind_bootstrap<G>(
        &mut self,
        choose: impl FnOnce() -> Result<Bootstrap, String>,
        authorize: impl FnOnce(&Bootstrap) -> Result<G, String>,
    ) -> Result<Bootstrap, String> {
        if let Some(record) = read(&self.home)? {
            return record.pending_bootstrap().cloned();
        }
        if pointer_bytes(&self.home)?.is_some() {
            return Err(unsettled());
        }
        let binding = choose()?;
        // Acquire cache publication exclusion only after resolving the candidate. The
        // caller rechecks that exact candidate under its guard before it becomes a reference.
        let _reference_guard = authorize(&binding)?;
        let record = Record::new(
            self.identity,
            Intent::Install,
            binding.targets(),
            Some(binding.clone()),
        );
        record.verify_home(&self.home)?;
        write(&self.home, &record)?;
        Ok(binding)
    }

    /// Persist the accepted archive proof before consuming archive bytes, without rebinding.
    /// Existing proofs are returned verbatim; the caller must decode and verify their schema.
    pub fn prove_bootstrap(
        &mut self,
        prove: impl FnOnce(&Bootstrap) -> Result<serde_json::Value, String>,
    ) -> Result<Bootstrap, String> {
        let mut record = read(&self.home)?.ok_or_else(unsettled)?;
        let binding = record.pending_bootstrap()?.clone();
        if binding.proof.is_some() {
            return Ok(binding);
        }
        let proof = prove(&binding)?;
        let binding = Bootstrap {
            proof: Some(proof),
            ..binding
        };
        record.bootstrap = Some(binding.clone());
        write(&self.home, &record)?;
        Ok(binding)
    }

    /// Stage an attempt on the same binding. Reopening after a failed prepare or publish
    /// cannot silently select a different tag, variant, profile identity, inputs or proof.
    pub fn resume_bootstrap(&mut self) -> Result<Staging<'_>, String> {
        let record = read(&self.home)?.ok_or_else(unsettled)?;
        record.pending_bootstrap()?;
        record.clean_binding_stage(&self.home)?;
        self.stage(record)
    }

    fn stage(&mut self, mut record: Record) -> Result<Staging<'_>, String> {
        clean_retired(&self.home)?;
        let root = record_dir(&self.home)?;
        fs::create_dir_all(&root).map_err(|e| at(&root, e))?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(STAGE_PREFIX).rand_bytes(24);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let stage = builder.tempdir_in(&root).map_err(|e| at(&root, e))?;
        let pointer = self.home.join(POINTER);
        record.operation = Some(Operation {
            stage: Stage {
                id: Identity::of(stage.path())?,
                name: stage
                    .path()
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or_else(unsettled)?
                    .to_owned(),
            },
            before: Image::at(&pointer)?,
            previous_pointer: pointer_bytes(&self.home)?,
            ready: None,
        });
        record.retirement = None;
        record.verify_home(&self.home)?;
        if let Some(binding) = record.undo_binding()? {
            let saved = stage.path().join("binding-after-undo");
            let bytes = serde_json::to_vec(&binding).map_err(|e| at(&saved, e))?;
            durable::replace(stage.path(), &saved, &bytes).map_err(|e| at(&saved, e))?;
        }
        // A reported sync failure may still leave the record visible. Keep its directory.
        let path = stage.keep();
        write(&self.home, &record)?;
        Ok(Staging {
            lease: self,
            path,
            record,
        })
    }

    fn recover(&self) -> Result<(), String> {
        if let Some(record) = read(&self.home)? {
            record.complete(&self.home, |_| {}, || {})?;
        } else {
            // No mutation is in flight. A retained cleanup record cannot block Play.
            let _ = clean_retired(&self.home);
        }
        Ok(())
    }
}

fn snapshot(source: &Path, copy: &Path, image: Image) -> Result<(), String> {
    if let Image::File { access, .. } = image {
        let mut src = fs::File::open(source).map_err(|e| at(source, e))?;
        let mut dst = fs::File::options()
            .create_new(true)
            .write(true)
            .open(copy)
            .map_err(|e| at(copy, e))?;
        io::copy(&mut src, &mut dst)
            .and_then(|_| access.apply(&dst))
            .and_then(|_| durable::flush(&dst))
            .map_err(|e| at(copy, e))?;
        drop(dst);
        if Image::at(copy)? != image {
            return Err(at(copy, unsettled()));
        }
        durable::sync_restored(copy, copy, false).map_err(|e| at(copy, e))?;
    }
    Ok(())
}

pub struct Staging<'a> {
    lease: &'a mut Admission,
    path: PathBuf,
    pub(super) record: Record,
}

impl<'a> Staging<'a> {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn resumes_bootstrap(&self) -> bool {
        self.record.bootstrap.is_some()
    }

    /// Borrow this stage's existing admission for a preparation or native worker. The
    /// worker may outlive the launcher, but only the launcher may seal proved output.
    pub fn handoff(&self) -> crate::process::leases::ProfileHandoff<'_> {
        self.lease.handoff()
    }

    /// The journal may have acquired candidate references since this stage was opened,
    /// but any other change means the stage no longer authorizes this operation.
    fn live_record(&self) -> Result<Record, String> {
        let home = self.lease.home();
        let mut live = read(home)?.ok_or_else(unsettled)?;
        if !self.record.targets.is_subset(&live.targets) {
            return Err(unsettled());
        }
        let targets = std::mem::replace(&mut live.targets, self.record.targets.clone());
        if live.digest()? != self.record.digest()? {
            return Err(unsettled());
        }
        live.targets = targets;
        live.operation()?.stage.verify(home)?;
        Ok(live)
    }

    /// Hold the cache publication lock. Protect a candidate tag before it can be used
    /// or published in the build cache, including while the profile pointer still names the old build.
    pub(crate) fn retain_target(&self, tag: &str) -> Result<(), String> {
        let home = self.lease.home();
        let mut live = self.live_record()?;
        if live.targets.insert(tag.to_owned()) {
            live.verify_home(home)?;
            write(home, &live)?;
        }
        Ok(())
    }

    pub(crate) fn seal_pointer_replacing(
        self,
        previous: &[u8],
        next: Vec<u8>,
    ) -> Result<Sealed<'a>, String> {
        if self.record.operation()?.previous_pointer.as_deref() != Some(previous) {
            return Err(unsettled());
        }
        self.seal(next, &[])
    }

    /// Bind exact preimages and candidates under the caller's reference guard, while only
    /// private files exist. A `None` source removes a file. Accept only launcher-managed names.
    pub fn seal(
        self,
        pointer_bytes: Vec<u8>,
        files: &[(PathBuf, Option<PathBuf>)],
    ) -> Result<Sealed<'a>, String> {
        self.seal_files(Some(pointer_bytes), files)
    }

    /// A files-only edit does not create or replace the active image pointer.
    pub(crate) fn seal_files(
        mut self,
        pointer_bytes: Option<Vec<u8>>,
        files: &[(PathBuf, Option<PathBuf>)],
    ) -> Result<Sealed<'a>, String> {
        self.record = self.live_record()?;
        let home = self.lease.home();
        let stage = &self.path;
        let record = &mut self.record;
        let pointer = home.join(POINTER);
        let before = record.operation()?.before;
        if Image::at(&pointer)? != before {
            return Err(at(&pointer, unsettled()));
        }
        let older = stage.join("pointer-old");
        snapshot(&pointer, &older, before)?;
        let pointer_after = if let Some(bytes) = &pointer_bytes {
            let newer = stage.join("pointer-new");
            durable::replace(stage, &newer, bytes).map_err(|e| at(&newer, e))?;
            Image::at(&newer)?
        } else {
            before
        };
        let mut plan = Ready {
            directories: BTreeMap::new(),
            files: Vec::new(),
            next_pointer: pointer_bytes,
            pointer_after,
        };
        let mut entries = BTreeSet::new();
        for (i, (name, source)) in files.iter().enumerate() {
            if !valid_destination(name) {
                return Err(unsettled());
            }
            let live = home.join(name);
            let parent_name = name.parent().ok_or_else(unsettled)?;
            let parent_path = home.join(parent_name);
            let parent = match fs::symlink_metadata(&parent_path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound && mod_root(parent_name) => {
                    if let Some(&id) = plan.directories.get(parent_name) {
                        id
                    } else {
                        let dir = stage.join(parent_name);
                        fs::create_dir(&dir).map_err(|e| at(&dir, e))?;
                        let id = Identity::of(&dir)?;
                        plan.directories.insert(parent_name.into(), id);
                        id
                    }
                }
                _ => Identity::of(&parent_path)?,
            };
            unique_entry(&live, &mut entries)?;
            let before = Image::at(&live)?;
            snapshot(&live, &stage.join(format!("old-{i}")), before)?;
            let after = if let Some(source) = source {
                let relative = source
                    .strip_prefix(stage)
                    .map_err(|_| at(source, unsettled()))?;
                let mut inside = stage.clone();
                let mut components = relative.components().peekable();
                while let Some(part) = components.next() {
                    let Component::Normal(name) = part else {
                        return Err(at(source, unsettled()));
                    };
                    inside.push(name);
                    if components.peek().is_some() && Identity::of(&inside)?.volume != parent.volume
                    {
                        return Err(at(source, unsettled()));
                    }
                }
                let image = Image::at(source)?;
                if image == Image::Missing {
                    return Err(at(source, unsettled()));
                }
                snapshot(source, &stage.join(format!("new-{i}")), image)?;
                image
            } else {
                Image::Missing
            };
            plan.files.push(Step {
                name: name.clone(),
                parent,
                before,
                after,
            });
        }
        plan.validate(record.operation()?)?;
        record.operation.as_mut().ok_or_else(unsettled)?.ready = Some(plan);
        progress::Slots::create(stage, record.digest()?).map_err(|e| at(stage, e))?;
        write(home, record)?;
        Ok(Sealed(self))
    }
}

pub struct Sealed<'a>(pub(super) Staging<'a>);

impl Sealed<'_> {
    /// Final external checks can return a short consent guard, held until the publishing
    /// frontier is recorded. A caller's cache/reference guard must span this whole call.
    /// No network calls or approvals occur after cutoff.
    pub fn publish<G>(self, cutoff: impl FnOnce() -> Result<G, String>) -> Result<(), String> {
        self.publish_observed(cutoff, |_, _| Ok(()))
    }

    pub(crate) fn publish_observed<G>(
        self,
        cutoff: impl FnOnce() -> Result<G, String>,
        boundary: impl FnMut(usize, Boundary) -> Result<(), String>,
    ) -> Result<(), String> {
        let home = self.0.lease.home();
        let record = read(home)?.ok_or_else(unsettled)?;
        if record.digest()? != self.0.record.digest()? {
            return Err(unsettled());
        }
        let result = record.publish_at(home, cutoff, boundary);
        if record.complete(home, |_| {}, || {})? {
            Ok(())
        } else {
            result
        }
    }
}

#[cfg(test)]
pub(crate) fn tear_next_commit_marker() {
    progress::fail_marker(progress::MarkerFault::TornWrite);
}
