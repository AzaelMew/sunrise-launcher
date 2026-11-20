//! A profile's exact local publication: stage, fence, move data, move the pointer, commit.
//! This layer doesn't select a build, inspect the network or decide what mods a player wants.
//! Existing launcher routes are not cut over to this record until their replacements can all
//! observe the same admission and committed generation.
mod durable;
mod progress;

use cap_fs_ext::MetadataExt;
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

const RECORD: &str = "mod-browser/install-txn.json";
const POINTER: &str = "mod-browser/active-build.json";
const STAGE_PREFIX: &str = ".install-txn-";
const VERSION: u64 = 2;

/// Moving a managed profile can change its path but never the filesystem identity of a
/// directory referenced by an in-flight transaction.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    volume: u64,
    inode: u64,
}

impl Identity {
    fn of(path: &Path) -> Result<Self, String> {
        if !fs::symlink_metadata(path).is_ok_and(|m| regular(&m) && m.is_dir()) {
            return Err(at(path, unsettled()));
        }
        let meta = Dir::open_ambient_dir(path, ambient_authority())
            .and_then(|dir| dir.dir_metadata())
            .map_err(|e| at(path, e))?;
        Ok(Self {
            volume: meta.dev(),
            inode: meta.ino(),
        })
    }
}

fn regular(meta: &fs::Metadata) -> bool {
    !crate::native_cache::linked(meta)
}

fn unsettled() -> String {
    crate::i18n::tr(
        "An earlier install couldn't be settled. Its transaction was kept. Nothing was changed.",
    )
    .into()
}

fn at(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

/// An admitted profile. The owner file must be stable and never removed, including across
/// detach/recreate; managed names pass a path outside the directory they can move. Its inherited
/// file description outlives a launcher killed while a game child still holds it.
pub struct Admission {
    home: PathBuf,
    identity: Identity,
    file: fs::File,
    #[cfg(unix)]
    transferred: std::sync::atomic::AtomicBool,
}

#[cfg(unix)]
impl Drop for Admission {
    fn drop(&mut self) {
        if !*self.transferred.get_mut() {
            let _ = self.file.unlock();
        }
    }
}

impl Admission {
    pub fn open(home: &Path, external_owner: Option<&Path>) -> Result<Self, String> {
        let lease = Self::claim(home, external_owner)?;
        lease.recover()?;
        Ok(lease)
    }

    fn claim(home: &Path, external_owner: Option<&Path>) -> Result<Self, String> {
        let gate = external_owner
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join(".install-txn.lock"));
        let identity = Identity::of(home)?;
        match fs::symlink_metadata(&gate) {
            Ok(meta) if regular(&meta) && meta.is_file() => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            _ => return Err(at(&gate, unsettled())),
        }
        let busy = || {
            crate::i18n::tr(
                "This profile is in use. Close the game or wait for the other operation to finish.",
            )
            .to_owned()
        };
        let mut open = fs::File::options();
        open.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.share_mode(0); // The Windows share exclusion follows inherited handles.
        }
        let file = match open.open(&gate) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Err(busy()),
            Err(error) => return Err(at(&gate, error)),
        };
        #[cfg(unix)]
        match file.try_lock() {
            Ok(()) => (),
            Err(fs::TryLockError::WouldBlock) => return Err(busy()),
            Err(fs::TryLockError::Error(e)) => return Err(at(&gate, e)),
        }
        if Identity::of(home)? != identity {
            return Err(unsettled());
        }
        Ok(Self {
            home: home.into(),
            identity,
            file,
            #[cfg(unix)]
            transferred: std::sync::atomic::AtomicBool::new(false),
        })
    }

    #[cfg(unix)]
    pub fn inherit(&self, command: &mut std::process::Command) -> io::Result<()> {
        crate::process::leases::inherit_profile(command, &self.file)?;
        self.transferred
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    #[cfg(windows)]
    pub fn inherited_handle(&self) -> &fs::File {
        &self.file
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Build privately in the returned directory. It is reserved durably before a worker sees it.
    /// Only the owning operation uses that path; worker-owned paths must be idle before seal.
    pub fn reserve(&mut self, intent: Intent) -> Result<Staging<'_>, String> {
        #[cfg(unix)]
        if *self.transferred.get_mut() {
            return Err(unsettled());
        }
        if read(&self.home)?.is_some() || Identity::of(&self.home)? != self.identity {
            return Err(unsettled());
        }
        let root = record_dir(&self.home)?;
        fs::create_dir_all(&root).map_err(|e| at(&root, e))?;
        let stage = tempfile::Builder::new()
            .prefix(STAGE_PREFIX)
            .rand_bytes(24)
            .tempdir_in(&root)
            .map_err(|e| at(&root, e))?;
        let operation = Operation {
            home: self.identity,
            stage_id: Identity::of(stage.path())?,
            stage_name: stage
                .path()
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(unsettled)?
                .into(),
            before: Image::at(&self.home.join(POINTER))?,
            ready: None,
        };
        if operation.home.volume != operation.stage_id.volume {
            return Err(unsettled());
        }
        // A reported sync failure may still leave the record visible. Keep its directory.
        let path = stage.keep();
        let record = Record {
            version: VERSION,
            intent,
            operation,
        };
        write(&self.home, &record)?;
        Ok(Staging {
            lease: self,
            path,
            record,
        })
    }

    fn recover(&self) -> Result<(), String> {
        if let Some(record) = read(&self.home)? {
            record.complete(&self.home, |_| {})?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Install,
    Switch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Image {
    Missing,
    Sha256([u8; 32]),
}

impl Image {
    fn at(path: &Path) -> Result<Self, String> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::Missing),
            Err(e) => return Err(at(path, e)),
        };
        if !meta.is_file() || !regular(&meta) {
            return Err(at(path, unsettled()));
        }
        let mut file = fs::File::open(path).map_err(|e| at(path, e))?;
        let mut hash = Sha256::new();
        io::copy(&mut file, &mut hash).map_err(|e| at(path, e))?;
        Ok(Self::Sha256(hash.finalize().into()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u64,
    intent: Intent,
    operation: Operation,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    home: Identity,
    stage_id: Identity,
    stage_name: String,
    before: Image,
    ready: Option<Ready>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    directories: BTreeMap<PathBuf, Identity>,
    files: Vec<Step>,
    // Keep immutable exact pointer bytes (not just a tag or a digest) for reference readers.
    previous_pointer: Option<Vec<u8>>,
    next_pointer: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    name: PathBuf,
    parent: Identity,
    before: Image,
    after: Image,
}

impl Ready {
    fn steps(&self) -> usize {
        self.directories.len() + self.files.len() + 1
    }

    fn directory(
        &self,
        home: &Path,
        stage: &Path,
        index: usize,
    ) -> Result<(PathBuf, PathBuf, Identity), String> {
        let (name, id) = self.directories.iter().nth(index).ok_or_else(unsettled)?;
        Ok((stage.join(name), home.join(name), *id))
    }

    fn validate(&self, op: &Operation) -> Result<(), String> {
        let mut names = BTreeSet::new();
        if self
            .directories
            .iter()
            .any(|(name, id)| !mod_root(name) || id.volume != op.home.volume)
            || self.files.iter().any(|step| {
                !valid_destination(&step.name)
                    || !names.insert(destination_key(&step.name))
                    || step.parent.volume != op.home.volume
            })
            || self
                .previous_pointer
                .as_ref()
                .map_or(Image::Missing, |bytes| {
                    Image::Sha256(Sha256::digest(bytes).into())
                })
                != op.before
            || self.next_pointer.is_empty()
        {
            return Err(unsettled());
        }
        Ok(())
    }
}

fn record_dir(home: &Path) -> Result<PathBuf, String> {
    let root = home.join("mod-browser");
    match fs::symlink_metadata(&root) {
        Ok(meta) if meta.is_dir() && regular(&meta) => Ok(root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(root),
        _ => Err(at(&root, unsettled())),
    }
}

fn read_at(home: &Path, name: &str) -> Result<Option<Record>, String> {
    record_dir(home)?;
    let path = home.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && regular(&meta) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        _ => return Err(at(&path, unsettled())),
    }
    let bytes = fs::read(&path).map_err(|e| at(&path, e))?;
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| at(&path, e))?;
    if record.version != VERSION {
        return Err(at(&path, unsettled()));
    }
    record.verify_home(home)?;
    Ok(Some(record))
}

fn read(home: &Path) -> Result<Option<Record>, String> {
    let record = read_at(home, RECORD)?;
    if let Some(record) = &record {
        record.verify_stage(home)?;
    }
    Ok(record)
}

fn write(home: &Path, record: &Record) -> Result<(), String> {
    let path = home.join(RECORD);
    let bytes = serde_json::to_vec(record).map_err(|e| at(&path, e))?;
    durable::replace(home, &path, &bytes).map_err(|e| at(&path, e))
}

fn snapshot(source: &Path, copy: &Path, image: Image) -> Result<(), String> {
    if image != Image::Missing {
        let mut src = fs::File::open(source).map_err(|e| at(source, e))?;
        let mut dst = fs::File::options()
            .create_new(true)
            .write(true)
            .open(copy)
            .map_err(|e| at(copy, e))?;
        io::copy(&mut src, &mut dst)
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
    record: Record,
}

impl<'a> Staging<'a> {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Bind exact preimages and candidates while only private files exist. A `None` source
    /// removes a file. Accept only launcher-managed package/list/sidecar names.
    pub fn seal(
        mut self,
        pointer_bytes: Vec<u8>,
        files: &[(PathBuf, Option<PathBuf>)],
    ) -> Result<Sealed<'a>, String> {
        let home = self.lease.home();
        let stage = &self.path;
        self.record.verify_home(home)?;
        self.record.verify_stage(home)?;
        let pointer = home.join(POINTER);
        let before = self.record.operation.before;
        if Image::at(&pointer)? != before {
            return Err(at(&pointer, unsettled()));
        }
        let older = stage.join("pointer-old");
        snapshot(&pointer, &older, before)?;
        let previous_pointer = if before == Image::Missing {
            None
        } else {
            Some(fs::read(&older).map_err(|e| at(&older, e))?)
        };
        let newer = stage.join("pointer-new");
        durable::replace(stage, &newer, &pointer_bytes).map_err(|e| at(&newer, e))?;
        let mut plan = Ready {
            directories: BTreeMap::new(),
            files: Vec::new(),
            previous_pointer,
            next_pointer: pointer_bytes,
        };
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
        plan.validate(&self.record.operation)?;
        self.record.operation.ready = Some(plan);
        progress::Slots::create(stage, self.record.digest()?).map_err(|e| at(stage, e))?;
        write(home, &self.record)?;
        Ok(Sealed(self))
    }
}

pub struct Sealed<'a>(Staging<'a>);

impl Sealed<'_> {
    /// Final external checks can return a short consent guard, held until the publishing
    /// frontier is recorded. A caller's cache/reference guard must span this whole call.
    /// No network calls or approvals occur after cutoff.
    pub fn publish<G>(self, cutoff: impl FnOnce() -> Result<G, String>) -> Result<(), String> {
        let home = self.0.lease.home();
        let record = read(home)?.ok_or_else(unsettled)?;
        if record.digest()? != self.0.record.digest()? {
            return Err(unsettled());
        }
        let result = record.publish_at(home, cutoff, |_, _| Ok(()));
        if record.complete(home, |_| {})? {
            Ok(())
        } else {
            result
        }
    }
}

fn mod_root(path: &Path) -> bool {
    use sunrise_launcher_smod::mod_list::{DIR, NATIVE_DIR};
    path.as_os_str() == DIR || path.as_os_str() == NATIVE_DIR
}

fn valid_destination(path: &Path) -> bool {
    let mut parts = path.components();
    let (Some(Component::Normal(first)), second, None) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    use sunrise_launcher_smod::mod_list::{DIR, FILE, NATIVE_DIR};
    match second {
        None => first == FILE || first == crate::browser::installed::MANIFEST,
        Some(Component::Normal(file)) => {
            (first == "mod-browser" && matches!(file.to_str(), Some("pack.json" | "pack.spack")))
                || ((first == DIR || first == NATIVE_DIR)
                    && file.to_str().is_some_and(|name| {
                        (name.ends_with(".smod") && crate::browser::safe_name(name).is_some())
                            || (first == NATIVE_DIR
                                && name
                                    .strip_suffix(".origin.json")
                                    .is_some_and(sunrise_launcher_smod::index::valid_id))
                    }))
        }
        _ => false,
    }
}

fn destination_key(path: &Path) -> String {
    let path = path.to_string_lossy();
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase()
    } else {
        path.into_owned()
    }
}

fn target(home: &Path, file: &Step) -> Result<PathBuf, String> {
    let path = home.join(&file.name);
    if Identity::of(path.parent().ok_or_else(unsettled)?)? != file.parent {
        return Err(at(&path, unsettled()));
    }
    Ok(path)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Boundary {
    BeforeRename,
    Renamed,
    Durable,
    BeforeMarker,
    WrittenMarker,
    DurableMarker,
}

impl Record {
    fn stage(&self, home: &Path) -> PathBuf {
        home.join("mod-browser").join(&self.operation.stage_name)
    }

    fn verify_home(&self, home: &Path) -> Result<(), String> {
        let op = &self.operation;
        if !op
            .stage_name
            .strip_prefix(STAGE_PREFIX)
            .is_some_and(|tail| !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
            || Identity::of(home)? != op.home
            || op.home.volume != op.stage_id.volume
        {
            return Err(unsettled());
        }
        if let Some(plan) = &op.ready {
            plan.validate(op)?;
        }
        Ok(())
    }

    fn verify_stage(&self, home: &Path) -> Result<(), String> {
        if Identity::of(&self.stage(home))? != self.operation.stage_id {
            return Err(unsettled());
        }
        Ok(())
    }

    fn digest(&self) -> Result<[u8; 32], String> {
        Ok(Sha256::digest(serde_json::to_vec(self).map_err(|e| e.to_string())?).into())
    }

    fn publish_at<G>(
        &self,
        home: &Path,
        cutoff: impl FnOnce() -> Result<G, String>,
        mut boundary: impl FnMut(usize, Boundary) -> Result<(), String>,
    ) -> Result<(), String> {
        self.verify_home(home)?;
        self.verify_stage(home)?;
        let stage = self.stage(home);
        let op = &self.operation;
        let plan = op.ready.as_ref().ok_or_else(unsettled)?;
        let pointer = home.join(POINTER);
        let next = Image::Sha256(Sha256::digest(&plan.next_pointer).into());
        if Image::at(&pointer)? != op.before
            || Image::at(&stage.join("pointer-old"))? != op.before
            || Image::at(&stage.join("pointer-new"))? != next
        {
            return Err(at(&pointer, unsettled()));
        }
        let mut progress = progress::Slots::open(&stage, self.digest()?, plan.steps())?;
        if progress.current().phase != progress::Phase::Ready {
            return Err(unsettled());
        }
        let directories = plan.directories.len();
        for i in 0..directories {
            let (src, dst, id) = plan.directory(home, &stage, i)?;
            if !absent(&dst)? || Identity::of(&src)? != id {
                return Err(at(&dst, unsettled()));
            }
        }
        for (i, file) in plan.files.iter().enumerate() {
            let name = if plan
                .directories
                .contains_key(file.name.parent().ok_or_else(unsettled)?)
            {
                if file.before != Image::Missing {
                    return Err(unsettled());
                }
                home.join(&file.name)
            } else {
                target(home, file)?
            };
            if Image::at(&name)? != file.before
                || Image::at(&stage.join(format!("old-{i}")))? != file.before
                || Image::at(&stage.join(format!("new-{i}")))? != file.after
            {
                return Err(at(&name, unsettled()));
            }
        }
        let mut permission = Some(cutoff()?);
        for i in 0..plan.steps() {
            progress.advance(progress::Phase::Publishing, i + 1)?;
            drop(permission.take());
            boundary(i, Boundary::BeforeRename)?;
            let mut fault = Ok(());
            let after_rename = || {
                fault = boundary(i, Boundary::Renamed);
            };
            if i == plan.steps() - 1 {
                durable::restore_move_at(&stage.join("pointer-new"), &pointer, after_rename)
                    .map_err(|e| at(&pointer, e))?;
            } else if i < directories {
                let (src, dst, id) = plan.directory(home, &stage, i)?;
                if Identity::of(&src)? != id {
                    return Err(at(&src, unsettled()));
                }
                durable::move_new_directory_at(&src, &dst, after_rename)
                    .map_err(|e| at(&dst, e))?;
            } else {
                let slot = i - directories;
                let file = &plan.files[slot];
                let dst = target(home, file)?;
                match (file.before, file.after) {
                    (Image::Missing, Image::Missing) => (),
                    (_, Image::Missing) => durable::restore_move_at(
                        &dst,
                        &stage.join(format!("removed-{slot}")),
                        after_rename,
                    )
                    .map_err(|e| at(&dst, e))?,
                    _ => durable::restore_move_at(
                        &stage.join(format!("new-{slot}")),
                        &dst,
                        after_rename,
                    )
                    .map_err(|e| at(&dst, e))?,
                }
            }
            fault?;
            boundary(i, Boundary::Durable)?;
        }
        boundary(plan.steps(), Boundary::BeforeMarker)?;
        progress.advance_at(progress::Phase::Committed, plan.steps(), || {
            boundary(plan.steps(), Boundary::WrittenMarker)
        })?;
        boundary(plan.steps(), Boundary::DurableMarker)
    }

    fn complete(&self, home: &Path, mut restored: impl FnMut(usize)) -> Result<bool, String> {
        use progress::{Phase, Slots};
        self.verify_home(home)?;
        self.verify_stage(home)?;
        let stage = self.stage(home);
        let pointer = home.join(POINTER);
        let op = &self.operation;
        let mut committed = false;
        if let Some(plan) = &op.ready {
            let next = Image::Sha256(Sha256::digest(&plan.next_pointer).into());
            let mut slots = Slots::open(&stage, self.digest()?, plan.steps())?;
            match slots.current().phase {
                Phase::Ready => {
                    if Image::at(&pointer)? != op.before {
                        return Err(at(&pointer, unsettled()));
                    }
                }
                Phase::Committed => {
                    slots.sync()?; // The last marker can be visible after a reported sync failure.
                    committed = true;
                    if Image::at(&pointer)? != next {
                        return Err(at(&pointer, unsettled()));
                    }
                    for i in 0..plan.directories.len() {
                        let (_, dst, id) = plan.directory(home, &stage, i)?;
                        if Identity::of(&dst)? != id {
                            return Err(at(&dst, unsettled()));
                        }
                    }
                    for file in &plan.files {
                        let dst = target(home, file)?;
                        if Image::at(&dst)? != file.after {
                            return Err(at(&dst, unsettled()));
                        }
                    }
                }
                Phase::Publishing | Phase::Undoing => {
                    if slots.current().phase == Phase::Publishing {
                        slots.advance(Phase::Undoing, slots.current().frontier)?;
                    }
                    let directories = plan.directories.len();
                    while slots.current().frontier > 0 {
                        let step = slots.current().frontier - 1;
                        if step == plan.steps() - 1 {
                            restore(
                                &pointer,
                                &stage.join("pointer-old"),
                                &stage.join("pointer-discarded"),
                                op.before,
                                next,
                            )?;
                        } else if step < directories {
                            let (src, dst, id) = plan.directory(home, &stage, step)?;
                            if absent(&dst)? {
                                if Identity::of(&src)? != id {
                                    return Err(at(&src, unsettled()));
                                }
                                durable::sync_restored(&dst, &src, true)
                                    .map_err(|e| at(&dst, e))?;
                            } else {
                                if Identity::of(&dst)? != id
                                    || fs::read_dir(&dst)
                                        .map_err(|e| at(&dst, e))?
                                        .next()
                                        .is_some()
                                {
                                    return Err(at(&dst, unsettled()));
                                }
                                durable::move_new_directory(&dst, &src).map_err(|e| at(&dst, e))?;
                            }
                        } else {
                            let index = step - directories;
                            let spec = &plan.files[index];
                            let dst = target(home, spec)?;
                            restore(
                                &dst,
                                &stage.join(format!("old-{index}")),
                                &stage.join(format!("discarded-{index}")),
                                spec.before,
                                spec.after,
                            )?;
                        }
                        restored(step);
                        slots.advance(Phase::Undoing, step)?;
                    }
                    if Image::at(&pointer)? != op.before {
                        return Err(at(&pointer, unsettled()));
                    }
                }
            }
        } else if Image::at(&pointer)? != op.before {
            return Err(at(&pointer, unsettled()));
        }
        let file = home.join(RECORD);
        durable::retire(&file).map_err(|e| at(&file, e))?;
        let _ = fs::remove_dir_all(stage);
        Ok(committed)
    }
}

fn restore(
    dst: &Path,
    backup: &Path,
    discarded: &Path,
    before: Image,
    after: Image,
) -> Result<(), String> {
    let live = Image::at(dst)?;
    if live != before {
        if live != after || Image::at(backup)? != before {
            return Err(at(dst, unsettled()));
        }
        if before == Image::Missing {
            durable::restore_move(dst, discarded)
        } else {
            durable::restore_move(backup, dst)
        }
        .map_err(|e| at(dst, e))?;
    }
    durable::sync_restored(dst, backup, before == Image::Missing).map_err(|e| at(dst, e))
}

fn absent(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(at(path, e)),
        Ok(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests;
