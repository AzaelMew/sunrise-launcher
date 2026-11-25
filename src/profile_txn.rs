//! A profile's exact local publication: stage, fence, move data, move the pointer, commit.
//! This layer doesn't select a build, inspect the network or decide what mods a player wants.
//! Existing launcher routes are not cut over to this record until their replacements can all
//! observe the same admission and committed generation.
mod admission;
mod commit;
pub(crate) mod durable;
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
const RETIRED: &str = "mod-browser/install-txn.retired.json";
pub(crate) const POINTER: &str = "mod-browser/active-build.json";
const STAGE_PREFIX: &str = ".install-txn-";
const VERSION: u64 = 2;
const BOOTSTRAP_VERSION: u64 = 1;

#[cfg(test)]
pub(crate) use admission::tear_next_commit_marker;
pub(crate) use admission::NameGate;
pub use admission::{Admission, Sealed, Staging};
pub(crate) use commit::Boundary;

/// Moving a managed profile can change its path but never the filesystem identity of a
/// directory referenced by an in-flight transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    volume: u64,
    inode: u64,
}

impl Identity {
    pub fn of(path: &Path) -> Result<Self, String> {
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

    fn file_at(path: &Path) -> Result<Option<Self>, String> {
        match fs::symlink_metadata(path) {
            Ok(meta) if regular(&meta) && meta.is_file() => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            _ => return Err(at(path, unsettled())),
        }
        let file = fs::File::open(path).map_err(|e| at(path, e))?;
        let meta = cap_std::fs::File::from_std(file)
            .metadata()
            .map_err(|e| at(path, e))?;
        Ok(Some(Self {
            volume: meta.dev(),
            inode: meta.ino(),
        }))
    }
}

fn regular(meta: &fs::Metadata) -> bool {
    !crate::native_cache::linked(meta)
}

fn unsettled() -> String {
    crate::i18n::tr(
        "Sunrise couldn't settle the profile's install transaction. The profile can't be used yet. Any recovery data was kept.",
    )
    .into()
}

fn at(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

fn profile_in_use() -> String {
    crate::i18n::tr(
        "This profile is in use. Close the game or wait for the other operation to finish.",
    )
    .to_owned()
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Install,
    Switch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum Image {
    Missing,
    File {
        sha256: [u8; 32],
        access: durable::Access,
    },
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
        let access = durable::Access::of(path, &file).map_err(|e| at(path, e))?;
        let mut hash = Sha256::new();
        io::copy(&mut file, &mut hash).map_err(|e| at(path, e))?;
        Ok(Self::File {
            sha256: hash.finalize().into(),
            access,
        })
    }

    fn digest(self) -> Option<[u8; 32]> {
        match self {
            Self::Missing => None,
            Self::File { sha256, .. } => Some(sha256),
        }
    }
}

/// First Play's durable binding, stored in the same journal as its pointer publish.
/// Selection and proof are versioned 0c payloads; the journal owns the stable tags and
/// frozen identities, while the artifact boundary validates those concrete payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    version: u64,
    pub base: String,
    pub image: String,
    pub profile: [u8; 16],
    pub inputs: [u8; 32],
    pub selection: serde_json::Value,
    pub proof: Option<serde_json::Value>,
}

impl Bootstrap {
    pub fn new(
        base: String,
        image: String,
        profile: [u8; 16],
        inputs: [u8; 32],
        selection: serde_json::Value,
    ) -> Self {
        Self {
            version: BOOTSTRAP_VERSION,
            base,
            image,
            profile,
            inputs,
            selection,
            proof: None,
        }
    }

    fn targets(&self) -> BTreeSet<String> {
        BTreeSet::from([self.base.clone(), self.image.clone()])
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u64,
    home: Identity,
    intent: Intent,
    targets: BTreeSet<String>,
    bootstrap: Option<Bootstrap>,
    operation: Option<Operation>,
    retirement: Option<Stage>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    id: Identity,
    name: String,
}

impl Stage {
    fn path(&self, home: &Path) -> PathBuf {
        home.join("mod-browser").join(&self.name)
    }

    fn validate(&self, owner: Identity) -> Result<(), String> {
        if owner.volume == self.id.volume
            && self.name.strip_prefix(STAGE_PREFIX).is_some_and(|tail| {
                !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_alphanumeric())
            })
        {
            Ok(())
        } else {
            Err(unsettled())
        }
    }

    fn verify(&self, home: &Path) -> Result<(), String> {
        if Identity::of(&self.path(home))? == self.id {
            Ok(())
        } else {
            Err(unsettled())
        }
    }

    fn clean(&self, home: &Path) -> Result<(), String> {
        let path = self.path(home);
        if !absent(&path)? {
            self.verify(home)?;
            fs::remove_dir_all(&path).map_err(|e| at(&path, e))?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    stage: Stage,
    before: Image,
    previous_pointer: Option<Vec<u8>>,
    ready: Option<Ready>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    directories: BTreeMap<PathBuf, Identity>,
    files: Vec<Step>,
    next_pointer: Option<Vec<u8>>,
    pointer_after: Image,
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
        self.directories.len() + self.files.len() + usize::from(self.next_pointer.is_some())
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
        if self.steps() == 0
            || self
                .directories
                .iter()
                .any(|(name, id)| !mod_root(name) || id.volume != op.stage.id.volume)
            || self.files.iter().any(|step| {
                !valid_destination(&step.name)
                    || !names.insert((step.parent, destination_key(&step.name)))
                    || step.parent.volume != op.stage.id.volume
            })
            || match &self.next_pointer {
                Some(bytes) => {
                    bytes.is_empty()
                        || self.pointer_after.digest() != Some(Sha256::digest(bytes).into())
                }
                None => self.pointer_after != op.before,
            }
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
    if let Some(operation) = record.as_ref().and_then(|record| record.operation.as_ref()) {
        operation.stage.verify(home)?;
    }
    Ok(record)
}

fn clean_retired(home: &Path) -> Result<(), String> {
    if let Some(record) = read_at(home, RETIRED)? {
        let marker = home.join(RETIRED);
        durable::establish_retirement(home, &marker).map_err(|e| at(&marker, e))?;
        remove_retired(home, &marker, &record.operation()?.stage)?;
    }
    Ok(())
}

fn remove_retired(home: &Path, marker: &Path, stage: &Stage) -> Result<(), String> {
    stage.clean(home)?;
    durable::retire(marker).map_err(|e| at(marker, e))
}

/// Read reference evidence under the caller's cache-reference guard, without acquiring
/// profile admission. Journal shape, staging identity and pointer file kinds are checked by
/// the same parser used for recovery. 0c parses the exact pointer bytes as its `Active` type.
pub struct References {
    pub targets: BTreeSet<String>,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    pub pointer: Option<Vec<u8>>,
}

pub fn references(home: &Path) -> Result<References, String> {
    let active = read(home)?;
    // A retirement rename can be visible before its durability barrier. Conservatively
    // keep these references until admission can enforce the barrier and remove the record.
    let retired = read_at(home, RETIRED)?;
    if active.is_some() && retired.is_some() {
        return Err(unsettled());
    }
    let mut refs = References {
        targets: BTreeSet::new(),
        before: None,
        after: None,
        pointer: pointer_bytes(home)?,
    };
    if let Some(record) = active.or(retired) {
        refs.targets = record.targets;
        if let Some(operation) = record.operation {
            refs.before = operation.previous_pointer;
            refs.after = operation.ready.and_then(|ready| ready.next_pointer);
            if refs.pointer != refs.before && (refs.after.is_none() || refs.pointer != refs.after) {
                return Err(unsettled());
            }
        } else if refs.pointer.is_some() {
            return Err(unsettled());
        }
    }
    Ok(refs)
}

/// The copy is still private and has never had a transaction or a live generation.
pub(crate) fn write_copy_pointer(home: &Path, bytes: &[u8]) -> Result<(), String> {
    record_dir(home)?;
    let path = home.join(POINTER);
    durable::replace(home, &path, bytes).map_err(|e| at(&path, e))
}

pub fn recovery_pending(home: &Path) -> Result<bool, String> {
    for name in [RECORD, RETIRED] {
        let path = home.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(at(&path, error)),
        }
    }
    Ok(false)
}

/// A display snapshot is not permission to act. An interrupted attempt is still
/// presented as the last settled generation until an admitter finishes recovery.
pub(crate) fn display_pointer(home: &Path) -> Result<Option<Vec<u8>>, String> {
    let pending = read_at(home, RECORD)?;
    if let Some(operation) = pending.and_then(|record| record.operation) {
        return Ok(operation.previous_pointer);
    }
    pointer_bytes(home)
}

fn pointer_bytes(home: &Path) -> Result<Option<Vec<u8>>, String> {
    record_dir(home)?;
    let file = home.join(POINTER);
    match fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_file() && regular(&meta) => {
            fs::read(&file).map(Some).map_err(|e| at(&file, e))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        _ => Err(at(&file, unsettled())),
    }
}

fn write(home: &Path, record: &Record) -> Result<(), String> {
    let path = home.join(RECORD);
    let bytes = serde_json::to_vec(record).map_err(|e| at(&path, e))?;
    durable::replace(home, &path, &bytes).map_err(|e| at(&path, e))
}

fn installed_manifest() -> &'static str {
    sunrise_launcher_smod::index::browser_state::MANIFEST
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
        None => first == FILE || first == installed_manifest(),
        Some(Component::Normal(file)) if first == "mod-browser" => {
            matches!(file.to_str(), Some("pack.json" | "pack.spack"))
        }
        Some(Component::Normal(file)) if first == DIR || first == NATIVE_DIR => {
            use sunrise_launcher_smod::index::{browser_state::portable_ascii_component, valid_id};
            let Some(name) = file.to_str() else {
                return false;
            };
            valid_smod_leaf(name)
                || (portable_ascii_component(name)
                    && name.strip_suffix(".origin.json").is_some_and(valid_id))
        }
        _ => false,
    }
}

/// The portable `.smod` leaf shared by the journal and typed receipts in both mod roots.
pub(crate) fn valid_smod_leaf(name: &str) -> bool {
    // Before either leaf exists, neither native identity nor a Unicode fold
    // can rule out filesystem-specific aliases.
    sunrise_launcher_smod::index::browser_state::portable_ascii_component(name)
        && name.ends_with(".smod")
        && !name.starts_with('.')
        && name.len() <= 200
        && !name.contains('\x7f')
}

fn destination_key(path: &Path) -> String {
    // All admitted leaves are portable ASCII. Keep the same key even on a
    // case-sensitive volume so moving a profile cannot introduce an alias.
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("validated destination")
        .to_ascii_lowercase()
}

fn unique_entry(path: &Path, entries: &mut BTreeSet<Identity>) -> Result<(), String> {
    // Existing files also need a filesystem identity check. Two distinct spellings can
    // address the same entry through names the portable key doesn't know (or hard links).
    if Identity::file_at(path)?.is_some_and(|identity| !entries.insert(identity)) {
        return Err(at(path, unsettled()));
    }
    Ok(())
}

fn target(home: &Path, file: &Step) -> Result<PathBuf, String> {
    let path = home.join(&file.name);
    if Identity::of(path.parent().ok_or_else(unsettled)?)? != file.parent {
        return Err(at(&path, unsettled()));
    }
    Ok(path)
}

impl Record {
    fn new(
        home: Identity,
        intent: Intent,
        targets: BTreeSet<String>,
        bootstrap: Option<Bootstrap>,
    ) -> Self {
        Self {
            version: VERSION,
            home,
            intent,
            targets,
            bootstrap,
            operation: None,
            retirement: None,
        }
    }

    fn operation(&self) -> Result<&Operation, String> {
        self.operation.as_ref().ok_or_else(unsettled)
    }

    fn pending_bootstrap(&self) -> Result<&Bootstrap, String> {
        if self.operation.is_some() {
            return Err(unsettled());
        }
        self.bootstrap.as_ref().ok_or_else(unsettled)
    }

    fn undo_binding(&self) -> Result<Option<Self>, String> {
        self.bootstrap
            .as_ref()
            .map(|bootstrap| {
                let mut pending = Self::new(
                    self.home,
                    Intent::Install,
                    bootstrap.targets(),
                    Some(bootstrap.clone()),
                );
                pending.retirement = Some(self.operation()?.stage.clone());
                Ok(pending)
            })
            .transpose()
    }

    fn preallocated_binding(stage: &Path, binding: &Self) -> Result<PathBuf, String> {
        let saved = stage.join("binding-after-undo");
        if Image::at(&saved)?.digest() != Some(binding.digest()?) {
            return Err(at(&saved, unsettled()));
        }
        Ok(saved)
    }

    fn clean_binding_stage(&self, home: &Path) -> Result<(), String> {
        if let Some(stage) = &self.retirement {
            let record = home.join(RECORD);
            durable::establish_retirement(home, &record).map_err(|e| at(&record, e))?;
            stage.clean(home)?;
        }
        Ok(())
    }

    fn verify_home(&self, home: &Path) -> Result<(), String> {
        if self.home != Identity::of(home)?
            || (self.bootstrap.is_none() && self.operation.is_none())
            || (self.operation.is_some() && self.retirement.is_some())
            || self.targets.iter().any(|tag| {
                !tag.as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                    || tag.ends_with('.')
                    || !tag
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.+@".contains(&b))
            })
        {
            return Err(unsettled());
        }
        if let Some(binding) = &self.bootstrap {
            if !matches!(self.intent, Intent::Install)
                || binding.version != BOOTSTRAP_VERSION
                || if self.operation.is_some() {
                    !binding.targets().is_subset(&self.targets)
                } else {
                    binding.targets() != self.targets
                }
                || self
                    .operation
                    .as_ref()
                    .is_some_and(|op| op.before != Image::Missing)
            {
                return Err(unsettled());
            }
        }
        if let Some(stage) = &self.retirement {
            stage.validate(self.home)?;
        }
        if let Some(op) = &self.operation {
            op.stage.validate(self.home)?;
            let before = op
                .previous_pointer
                .as_ref()
                .map(|bytes| Sha256::digest(bytes).into());
            if before != op.before.digest() {
                return Err(unsettled());
            }
            if let Some(plan) = &op.ready {
                plan.validate(op)?;
            }
        }
        Ok(())
    }

    fn digest(&self) -> Result<[u8; 32], String> {
        Ok(Sha256::digest(serde_json::to_vec(self).map_err(|e| e.to_string())?).into())
    }
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
