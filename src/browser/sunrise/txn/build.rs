//! The published Play generation and the binding retained while admitting an old profile.
use super::{durable, unsettled, Image, Lease};
use crate::{backend, native_mods, native_status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) const FILE: &str = "mod-browser/active-build.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    base: String,
    preferred_image: String,
}

impl Target {
    /// A legacy profile may already point at a derived image. Preserve that exact choice.
    pub fn legacy(tag: &str) -> Result<Self, String> {
        validate_tag(tag)?;
        let base = if native_mods::is_native(tag) {
            native_mods::about(tag).ok_or_else(unsettled)?.base
        } else {
            tag.to_owned()
        };
        validate_tag(&base)?;
        Ok(Self {
            base,
            preferred_image: tag.into(),
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn preferred_image(&self) -> &str {
        &self.preferred_image
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        validate_tag(&self.base)?;
        validate_tag(&self.preferred_image)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bootstrap {
    pub(super) target: Target,
    pub(super) profile: [u8; 16],
    inputs: [u8; 32],
}

impl Bootstrap {
    pub(super) fn bind(home: &Path, target: Target) -> Result<Self, String> {
        let mut profile = [0; 16];
        getrandom::fill(&mut profile).map_err(|e| e.to_string())?;
        Ok(Self {
            target,
            profile,
            inputs: inputs(home)?,
        })
    }

    pub(super) fn verify(&self, home: &Path) -> Result<(), String> {
        if inputs(home)? != self.inputs {
            return Err(crate::i18n::tr("The profile changed while its first build was being prepared. Nothing was switched.").into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn test_active(&self) -> Active {
        let base = Artifact {
            tag: self.target.base.clone(),
            executable: [1; 32],
            metadata: [Image::Missing, Image::Missing, Image::Missing],
        };
        Active {
            version: 1,
            profile: self.profile,
            generation: 1,
            executable: base.clone(),
            base,
            inputs: self.inputs,
            receipts: Vec::new(),
            waiting: Default::default(),
        }
    }

    pub(super) fn active(&self, image: &str, home: &Path) -> Result<Active, String> {
        self.verify(home)?;
        Active::capture(
            self.profile,
            1,
            &self.target.base,
            image,
            self.inputs,
            Vec::new(),
            home,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    tag: String,
    executable: [u8; 32],
    metadata: [Image; 3],
}

impl Artifact {
    fn capture(tag: &str) -> Result<Self, String> {
        validate_tag(tag)?;
        let dir = backend::version_dir(tag);
        require_dir(&dir)?;
        let path = dir.join(backend::CLIENT);
        let Image::Sha256(executable) = Image::at(&path)? else {
            return Err(format!("{}: {}", path.display(), io::ErrorKind::NotFound));
        };
        Ok(Self {
            tag: tag.into(),
            executable,
            metadata: [
                Image::at(&dir.join("sunrise-release.json"))?,
                Image::at(&dir.join("native-mods.json"))?,
                Image::at(&dir.join("research-lock.json"))?,
            ],
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Active {
    version: u32,
    pub(super) profile: [u8; 16],
    pub(super) generation: u64,
    base: Artifact,
    executable: Artifact,
    pub(super) inputs: [u8; 32],
    receipts: Vec<Receipt>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    waiting: std::collections::BTreeMap<String, WaitingMod>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitingMod {
    pub(super) file: PathBuf,
    pub(super) digest: [u8; 32],
    pub reason: String,
    pub(in crate::browser::sunrise) was_on: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchEffects {
    pub waiting_for_update: Vec<(String, String)>,
    pub removed: Vec<(String, String)>,
}

impl SwitchEffects {
    pub fn is_empty(&self) -> bool {
        self.waiting_for_update.is_empty() && self.removed.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    id: [u8; 16],
    effects: SwitchEffects,
}

impl Receipt {
    pub fn id(&self) -> [u8; 16] {
        self.id
    }
    pub fn effects(&self) -> &SwitchEffects {
        &self.effects
    }
}

impl Active {
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }
    fn capture(
        profile: [u8; 16],
        generation: u64,
        base: &str,
        image: &str,
        inputs: [u8; 32],
        receipts: Vec<Receipt>,
        home: &Path,
    ) -> Result<Self, String> {
        verify_included(base, image, home)?;
        if self::inputs(home)? != inputs {
            return Err(unsettled());
        }
        let base = Artifact::capture(base)?;
        let executable = if image == base.tag {
            base.clone()
        } else {
            Artifact::capture(image)?
        };
        Ok(Self {
            version: 1,
            profile,
            generation,
            base,
            executable,
            inputs,
            receipts,
            waiting: Default::default(),
        })
    }

    pub(super) fn adopt(&self, image: &str, inputs: [u8; 32], home: &Path) -> Result<Self, String> {
        let generation = self.generation.checked_add(1).ok_or_else(unsettled)?;
        let mut next = Self::capture(
            self.profile,
            generation,
            self.base(),
            image,
            inputs,
            self.receipts.clone(),
            home,
        )?;
        if next.base != self.base {
            return Err(unsettled());
        }
        for (id, item) in &self.waiting {
            if item.matches(home)? {
                next.waiting.insert(id.clone(), item.clone());
            }
        }
        next.require_waiting_off(home)?;
        Ok(next)
    }

    pub(super) fn switched(
        &self,
        base: &str,
        executable: &str,
        staged: &Path,
        effects: SwitchEffects,
        waiting: std::collections::BTreeMap<String, WaitingMod>,
    ) -> Result<Self, String> {
        let mut receipts = self.receipts.clone();
        if !effects.is_empty() {
            let mut id = [0; 16];
            getrandom::fill(&mut id).map_err(|e| e.to_string())?;
            receipts.push(Receipt { id, effects });
        }
        let mut next = Self::capture(
            self.profile,
            self.generation.checked_add(1).ok_or_else(unsettled)?,
            base,
            executable,
            inputs(staged)?,
            receipts,
            staged,
        )?;
        next.waiting = waiting;
        next.require_waiting_off(staged)?;
        Ok(next)
    }

    pub(super) fn require_waiting_off(&self, home: &Path) -> Result<(), String> {
        for (id, item) in &self.waiting {
            if crate::browser::mod_list::is_enabled(home, id) && item.matches(home)? {
                return Err(crate::i18n::trf(
                    "{mod} needs an update from its author before it can be turned on.",
                    &[("mod", id)],
                ));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn test_receipt(mut self, sequence: u8) -> Self {
        self.receipts.push(Receipt {
            id: [sequence; 16],
            effects: SwitchEffects {
                waiting_for_update: self
                    .waiting
                    .keys()
                    .map(|id| (id.clone(), "no fit".into()))
                    .collect(),
                removed: vec![(format!("mod-{sequence}"), format!("reason-{sequence}"))],
            },
        });
        self
    }

    #[cfg(test)]
    pub(super) fn test_waiting(mut self, id: &str, item: WaitingMod) -> Self {
        self.waiting.insert(id.to_owned(), item);
        self
    }

    #[cfg(test)]
    pub(super) fn test_switch(&self, tag: &str) -> Self {
        let mut active = self.clone();
        active.base.tag = tag.to_owned();
        active.executable.tag = tag.to_owned();
        active.generation += 1;
        active
    }

    pub fn base(&self) -> &str {
        &self.base.tag
    }

    pub(super) fn same_base_artifact(&self, other: &Self) -> bool {
        self.base == other.base
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn executable(&self) -> &str {
        &self.executable.tag
    }

    pub fn verify_artifacts(&self) -> Result<(), String> {
        if Artifact::capture(self.base())? != self.base
            || (self.executable != self.base
                && Artifact::capture(self.executable())? != self.executable)
        {
            return Err(crate::i18n::tr(
                "The profile's saved game build does not match the files on disk.",
            )
            .into());
        }
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.generation == 0 || self.profile == [0; 16] {
            return Err(unsettled());
        }
        let mut receipts = std::collections::HashSet::new();
        if self.receipts.iter().any(|receipt| {
            receipt.id == [0; 16] || !receipts.insert(receipt.id) || receipt.effects.is_empty()
        }) {
            return Err(unsettled());
        }
        for (id, mod_) in &self.waiting {
            if !sunrise_launcher_smod::index::valid_id(id)
                || !super::switch_record::valid_package_destination(&mod_.file)
            {
                return Err(unsettled());
            }
        }
        validate_tag(self.base())?;
        validate_tag(self.executable())
    }
}

pub(super) fn verify_included(base: &str, image: &str, home: &Path) -> Result<(), String> {
    if image != base
        && native_mods::about(image)
            .is_none_or(|about| about.base != base || !about.left_out.is_empty())
    {
        return Err(unsettled());
    }
    let included = native_status::included(image)?;
    native_status::check_compiled_profile(&included, home)?;
    if native_status::enabled(home).len() != included.len() {
        return Err(crate::i18n::tr("This native build doesn't say exactly which mods it includes. Rebuild the mods or choose another game build.").into());
    }
    Ok(())
}

pub(super) fn verify_packages(home: &Path) -> Result<(), String> {
    use sunrise_launcher_smod::{manifest, mod_list};
    let mut ids = std::collections::HashSet::new();
    for directory in [mod_list::DIR, mod_list::NATIVE_DIR] {
        let root = home.join(directory);
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", root.display())),
        };
        for entry in entries {
            let path = entry
                .map_err(|e| format!("{}: {e}", root.display()))?
                .path();
            if path.extension().is_none_or(|ext| ext != "smod") {
                continue;
            }
            let package =
                manifest::Package::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if (directory == mod_list::NATIVE_DIR) != package.manifest.native.is_some()
                || !ids.insert(package.manifest.id)
            {
                return Err(unsettled());
            }
        }
    }
    Ok(())
}

pub(super) fn validate_tag(tag: &str) -> Result<(), String> {
    if tag.is_empty()
        || !tag.as_bytes()[0].is_ascii_alphanumeric()
        || tag.ends_with('.')
        || !tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+@".contains(&b))
    {
        return Err(unsettled());
    }
    Ok(())
}

pub(super) fn read(home: &Path) -> Result<Option<Active>, String> {
    let path = home.join(FILE);
    let Some(bytes) = optional_bytes(&path)? else {
        return Ok(None);
    };
    parse(&bytes)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub(super) fn parse(bytes: &[u8]) -> Result<Active, String> {
    let active: Active = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    active.validate()?;
    Ok(active)
}

pub(super) fn publish(home: &Path, active: &Active) -> Result<(), String> {
    let path = home.join(FILE);
    let bytes = serde_json::to_vec(active).map_err(|e| e.to_string())?;
    durable::replace(home, &path, &bytes).map_err(|e| format!("{}: {e}", path.display()))
}

// An advisory label for lists only. Launches and mutations must use an admitted lease.
pub fn displayed_image(home: &Path) -> Option<String> {
    read(home)
        .ok()
        .flatten()
        .map(|active| active.executable.tag)
}

impl WaitingMod {
    fn matches(&self, home: &Path) -> Result<bool, String> {
        Ok(Image::at(&home.join(&self.file))? == Image::Sha256(self.digest))
    }

    /// The newest candidate the registry can offer that is not the exact disabled bytes.
    /// The install still has to verify its dependencies and, for native code, build it.
    pub fn replacement<'a>(
        &self,
        index: &sunrise_launcher_smod::index::Index,
        project: &'a sunrise_launcher_smod::index::Project,
        game: &str,
    ) -> Option<&'a sunrise_launcher_smod::index::Version> {
        project.versions.iter().find(|v| {
            crate::browser::sunrise::fits_game(v, game)
                && !self.same_package(v)
                && index.revoked(&project.id, &v.file.sha256).is_none()
        })
    }

    pub(in crate::browser::sunrise) fn same_package(
        &self,
        version: &sunrise_launcher_smod::index::Version,
    ) -> bool {
        version.file.sha256.eq_ignore_ascii_case(&format!(
            "{:x}",
            sha2::digest::Output::<Sha256>::from(self.digest)
        ))
    }
}

pub(in crate::browser::sunrise) fn waiting_packages(
    home: &Path,
) -> Result<std::collections::BTreeMap<String, WaitingMod>, String> {
    let Some(active) = read(home)? else {
        return Ok(Default::default());
    };
    let mut waiting = std::collections::BTreeMap::new();
    for (id, item) in active.waiting {
        if item.matches(home)? {
            waiting.insert(id, item);
        }
    }
    Ok(waiting)
}

/// Installed display only. Admission reads and validates the same file independently.
pub fn waiting_for_update(home: &Path) -> std::collections::BTreeMap<String, WaitingMod> {
    waiting_packages(home).unwrap_or_default()
}

pub fn active_stamp(home: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let meta = fs::metadata(home.join(FILE)).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

pub(crate) fn require_current_update(home: &Path, ids: &[&str]) -> Result<(), String> {
    let Some(active) = read(home)? else {
        return Ok(());
    };
    for id in ids {
        if let Some(item) = active.waiting.get(*id) {
            if item.matches(home)? {
                return Err(crate::i18n::trf(
                    "{mod} needs an update from its author before it can be turned on.",
                    &[("mod", id)],
                ));
            }
        }
    }
    Ok(())
}

impl Lease {
    /// A delayed acknowledgement owns no authority over any other receipt or generation.
    pub fn acknowledge_switch(&self, id: [u8; 16]) -> Result<bool, String> {
        let _lock = super::locked(self.home())?;
        if super::read(self.home())?.is_some() {
            return Err(unsettled());
        }
        let Some(mut active) = self.active_build()? else {
            return Ok(false);
        };
        let before = active.receipts.len();
        active.receipts.retain(|receipt| receipt.id != id);
        if active.receipts.len() == before {
            return Ok(false);
        }
        let _publication =
            crate::run_leases::installation().map_err(crate::native_mods::cache_error)?;
        publish(self.home(), &active)?;
        Ok(true)
    }

    pub fn active_build(&self) -> Result<Option<Active>, String> {
        read(self.home())
    }
}

pub(super) fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let fail = |e| format!("{}: {e}", path.display());
    match fs::symlink_metadata(path) {
        Ok(meta) if regular(&meta) && meta.is_file() => fs::read(path).map(Some).map_err(fail),
        Ok(_) => Err(fail(io::Error::from(io::ErrorKind::InvalidData))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(fail(e)),
    }
}

pub(super) fn regular(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            == 0
    }
    #[cfg(not(windows))]
    !meta.file_type().is_symlink()
}

pub(super) fn require_dir(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.is_dir() && regular(&meta) {
        Ok(())
    } else {
        Err(format!(
            "{}: {}",
            path.display(),
            io::ErrorKind::InvalidData
        ))
    }
}

/// Take the identity from the actual bytes, not a display/cache manifest. Order and file
/// boundaries are explicit; a rebuild cannot substitute a changed package at the same path.
pub(super) fn inputs(home: &Path) -> Result<[u8; 32], String> {
    fn item(hash: &mut Sha256, root: &Path, rel: &str) -> Result<(), String> {
        hash.update((rel.len() as u64).to_le_bytes());
        hash.update(rel.as_bytes());
        let path = root.join(rel);
        let io_error = |e| format!("{}: {e}", path.display());
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) if regular(&meta) => meta,
            Ok(_) => return Err(io_error(io::Error::from(io::ErrorKind::InvalidData))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                hash.update(b"-");
                return Ok(());
            }
            Err(e) => return Err(io_error(e)),
        };
        if meta.is_dir() {
            hash.update(b"[");
            let mut names: Vec<_> = fs::read_dir(&path)
                .map_err(io_error)?
                .map(|e| e.map(|e| e.file_name()))
                .collect::<Result<_, _>>()
                .map_err(io_error)?;
            names.sort_unstable();
            for name in names {
                let name = name.to_str().ok_or_else(unsettled)?;
                item(hash, root, &format!("{rel}/{name}"))?;
            }
            hash.update(b"]");
        } else if meta.is_file() {
            let Image::Sha256(digest) = Image::at(&path)? else {
                return Err(unsettled());
            };
            hash.update(b"f");
            hash.update(digest);
        } else {
            return Err(io_error(io::Error::from(io::ErrorKind::InvalidData)));
        }
        Ok(())
    }

    use sunrise_launcher_smod::mod_list::{DIR, FILE, NATIVE_DIR};
    let mut hash = Sha256::new();
    for rel in [DIR, NATIVE_DIR, FILE, crate::browser::installed::MANIFEST] {
        item(&mut hash, home, rel)?;
    }
    Ok(hash.finalize().into())
}
