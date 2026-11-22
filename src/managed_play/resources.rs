//! Managed Play reads the exact prepared tree saved with its generation. The ordinary
//! preparation cache remains available for a later Prepare, without changing an active game.
use super::{artifact, digest_file, optional_bytes, plain_file, require_dir, unsettled};
use crate::{backend, preparer, profile_txn::durable};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

const ROOT: &str = "launcher-prepared";
const EVIDENCE: &str = "data-file.json";
const CURRENT: &str = "current.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resources {
    primary: [u8; 32],
    accelerator: Option<[u8; 32]>,
}

impl Resources {
    pub(super) fn capture(
        base: &str,
        stage: &crate::profile_txn::Staging<'_>,
    ) -> Result<Self, String> {
        let root = backend::version_dir(base);
        let preparation = backend::prepare_lock(base)?;
        match Self::current(&root)? {
            Some(resources) => Ok(resources),
            None => Self::prepare(base, &preparation, Some(stage.handoff())),
        }
    }

    fn current(root: &Path) -> Result<Option<Self>, String> {
        Ok(optional_bytes(&receipt(root)?)?
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|resources: &Self| resources.path_at(root).is_dir()))
    }

    /// Finish the shared preparation under its existing lock, before a profile needs it.
    pub(crate) fn prepare(
        base: &str,
        preparation: &preparer::Preparation,
        profile: Option<crate::process::leases::ProfileHandoff<'_>>,
    ) -> Result<Self, String> {
        backend::need_base_prepared(base, Some(preparation))?;
        let handoff = preparation.handoff(profile);
        backend::complete_assets_staged(base, &handoff);
        let root = backend::version_dir(base);
        Self::seal(&root, &backend::resources(base), |resources| {
            let binary = preparer::binary(&root)?;
            match preparer::data_file(&binary, &root, resources, Some(&handoff)) {
                Ok(Some(true)) => digest_file(&resources.join(preparer::DATA_FILE))
                    .ok()
                    .flatten(),
                Ok(_) => None,
                Err(error) => {
                    eprintln!("{error}");
                    None
                }
            }
        })
    }

    pub(crate) fn invalidate(root: &Path) -> io::Result<()> {
        durable::retire(&receipt(root).map_err(io::Error::other)?)
    }

    pub(super) fn path(&self, base: &str) -> PathBuf {
        self.path_at(&backend::version_dir(base))
    }

    fn path_at(&self, root: &Path) -> PathBuf {
        parent(root).join(key(self.primary)).join("resources")
    }

    pub(super) fn verify(&self, base: &str) -> Result<(), String> {
        let root = backend::version_dir(base);
        let path = self.path(base);
        for dir in [
            &root,
            &root.join(".local"),
            &parent(&root),
            path.parent().expect("snapshot parent"),
        ] {
            require_dir(dir)?;
        }
        let actual = artifact::tree(&path, true)?;
        if actual != Some(self.primary) {
            return Err(setup_error(&path));
        }
        Ok(())
    }

    pub(super) fn allow_accelerator(&self, base: &str) -> bool {
        self.accelerator.is_some_and(|digest| {
            digest_file(&self.path(base).join(preparer::DATA_FILE))
                .is_ok_and(|found| found == Some(digest))
        })
    }

    // When the source is prepared, flush a private primary tree and publish it without
    // replacing anything. Build the optional cache against the final path: the game caches
    // the canonical resource root inside it. A kill after exposure but before its evidence
    // write leaves a usable primary tree and an untrusted accelerator, never the reverse.
    fn seal(
        root: &Path,
        source: &Path,
        accelerator: impl FnOnce(&Path) -> Option<[u8; 32]>,
    ) -> Result<Self, String> {
        let relative = source.strip_prefix(root).map_err(|_| setup_error(source))?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(setup_error(source));
        }
        for ancestor in source.ancestors().take_while(|path| *path != root) {
            require_dir(ancestor)?;
        }
        require_dir(root)?;
        let Some(primary) = artifact::tree(source, true)? else {
            return Err(setup_error(source));
        };
        let parent = parent(root);
        fs::create_dir_all(&parent).map_err(|e| at(&parent, e))?;
        require_dir(&parent)?;
        let snapshot = parent.join(key(primary));
        let resources = snapshot.join("resources");
        let evidence = snapshot.join(EVIDENCE);
        let exists = snapshot.exists();
        if exists {
            require_dir(&snapshot)?;
        }
        let accelerator = if resources.is_dir() {
            optional_bytes(&evidence)?.and_then(|bytes| serde_json::from_slice(&bytes).ok())
        } else {
            // A move from .local flushes both directories, including the new store's entry.
            let stage = tempfile::Builder::new()
                .prefix(".prepared-")
                .tempdir_in(root.join(".local"))
                .map_err(|error| at(root, error))?;
            let candidate = stage.path().join("resources");
            copy_tree(source, &candidate, true, 0)?;
            durable::flush_tree(&candidate).map_err(|e| at(&candidate, e))?;
            let (from, to) = if exists {
                (candidate.as_path(), &resources)
            } else {
                (stage.path(), &snapshot)
            };
            durable::move_new_directory(from, to).map_err(|e| at(to, e))?;
            let digest = accelerator(&resources);
            if let Some(digest) = digest {
                write_receipt(&evidence, &digest)?;
            }
            digest
        };
        let prepared = Self {
            primary,
            accelerator,
        };
        write_receipt(&parent.join(CURRENT), &prepared)?;
        Ok(prepared)
    }
}

pub(super) fn generated_cache(name: &OsStr) -> bool {
    name == preparer::DATA_FILE
        || name.to_str().is_some_and(|name| {
            name.strip_prefix("sunrise-data.bin.")
                .and_then(|name| name.strip_suffix(".tmp"))
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|c| c.is_ascii_digit()))
        })
}

fn parent(root: &Path) -> PathBuf {
    root.join(".local").join(ROOT)
}
fn receipt(root: &Path) -> Result<PathBuf, String> {
    let parent = parent(root);
    if parent.exists() {
        for dir in [root, &root.join(".local"), &parent] {
            require_dir(dir)?;
        }
    }
    Ok(parent.join(CURRENT))
}
// These receipts only cache selection or accelerator evidence. A crash can lose them; the
// primary tree is already durable and a profile's independent digest is checked at cutoff.
fn write_receipt(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().expect("snapshot directory"))
        .map_err(|e| at(path, e))?;
    serde_json::to_writer(&mut staged, value).map_err(|e| e.to_string())?;
    staged
        .persist(path)
        .map(|_| ())
        .map_err(|e| at(path, e.error))
}
fn key(digest: [u8; 32]) -> String {
    format!("{:x}", sha2::digest::Output::<Sha256>::from(digest))
}

pub(super) fn copy_tree(
    source: &Path,
    dest: &Path,
    skip_accelerator: bool,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err(setup_error(source));
    }
    require_dir(source)?;
    fs::create_dir(dest).map_err(|e| at(dest, e))?;
    for entry in fs::read_dir(source).map_err(|e| at(source, e))? {
        let entry = entry.map_err(|e| at(source, e))?;
        let name = entry.file_name();
        if skip_accelerator && generated_cache(&name) {
            continue;
        }
        let src = entry.path();
        let dst = dest.join(name);
        let meta = fs::symlink_metadata(&src).map_err(|e| at(&src, e))?;
        if !plain_file(&meta) {
            return Err(setup_error(&src));
        }
        if meta.is_dir() {
            copy_tree(&src, &dst, false, depth + 1)?;
        } else if meta.is_file() {
            preparer::clone_or_copy(&src, &dst).map_err(|e| at(&dst, e))?;
        } else {
            return Err(setup_error(&src));
        }
    }
    Ok(())
}

fn setup_error(path: &Path) -> String {
    format!("{}: {}", path.display(), unsettled())
}
fn at(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_seal_a_prepared_tree_outside_the_install() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("version");
        let source = temp.path().join("prepared");
        fs::create_dir_all(root.join(".local")).unwrap();
        fs::create_dir(&source).unwrap();
        fs::write(source.join("sunrise-resources.json"), "{}").unwrap();
        for source in [source, root.join("../prepared")] {
            assert!(Resources::seal(&root, &source, |_| None).is_err());
        }
        assert!(!parent(&root).exists());
    }

    #[test]
    fn missing_snapshot_uses_the_same_seal_path_as_first_preparation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source = root.join(".local/prepared");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("sunrise-resources.json"), "{}").unwrap();
        let prepared = Resources::seal(root, &source, |_| None).unwrap();
        let path = prepared.path_at(root);
        for missing in [&path, path.parent().unwrap()] {
            fs::remove_dir_all(missing).unwrap();
            assert_eq!(Resources::current(root).unwrap(), None);
            assert_eq!(Resources::seal(root, &source, |_| None).unwrap(), prepared);
            assert_eq!(Resources::current(root).unwrap().as_ref(), Some(&prepared));
        }
    }

    #[test]
    fn prepared_snapshot_does_not_read_a_cache_from_a_different_tree() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let source = root.join(".local/prepared");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("sunrise-resources.json"), "{}").unwrap();
        fs::write(source.join("blocks.json"), "{\"old\": true}").unwrap();
        fs::write(source.join(preparer::DATA_FILE), b"untrusted").unwrap();
        let prepared = Resources::seal(root, &source, |dir| {
            let data = dir.join(preparer::DATA_FILE);
            assert!(!data.exists());
            fs::write(&data, b"generated from the snapshot").unwrap();
            digest_file(&data).unwrap()
        })
        .unwrap();
        let snapshot = parent(root).join(key(prepared.primary)).join("resources");
        assert_eq!(
            fs::read_to_string(snapshot.join("blocks.json")).unwrap(),
            "{\"old\": true}"
        );
        assert_eq!(
            Resources::seal(root, &source, |_| panic!("snapshot reused")).unwrap(),
            prepared
        );
        fs::write(source.join("blocks.json"), "{\"new\": true}").unwrap();
        assert_eq!(Resources::current(root).unwrap().as_ref(), Some(&prepared));
        fs::write(receipt(root).unwrap(), b"partial").unwrap();
        assert_eq!(Resources::current(root).unwrap(), None);
        Resources::invalidate(root).unwrap();
        assert_eq!(Resources::current(root).unwrap(), None);
        let changed = Resources::seal(root, &source, |_| None).unwrap();
        assert_ne!(prepared.primary, changed.primary);
        assert_eq!(Resources::current(root).unwrap(), Some(changed));
        fs::write(snapshot.join(preparer::DATA_FILE), b"different cache").unwrap();
        fs::write(snapshot.join("sunrise-data.bin.123.tmp"), b"pending cache").unwrap();
        let primary = || artifact::tree(&snapshot, true).unwrap();
        assert_eq!(primary(), Some(prepared.primary));
        fs::write(snapshot.join("blocks.json"), b"different primary bytes").unwrap();
        assert_ne!(primary(), Some(prepared.primary));
    }
}
