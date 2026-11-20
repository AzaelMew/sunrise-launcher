//! Exact identity of an installed executable and the files it reads beside itself.
use super::{digest_file, plain_file, require_dir, validate_tag};
use crate::backend;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub(super) tag: String,
    executable: [u8; 32],
    runtime: [u8; 32],
}

impl Artifact {
    pub(super) fn capture(tag: &str) -> Result<Self, String> {
        validate_tag(tag)?;
        Self::capture_at(tag, &backend::version_dir(tag))
    }

    pub(super) fn capture_at(tag: &str, dir: &Path) -> Result<Self, String> {
        require_dir(dir)?;
        let complete = dir.join(".installed");
        if digest_file(&complete)?.is_none() {
            return Err(at(&complete, io::ErrorKind::NotFound.into()));
        }
        let path = dir.join(backend::CLIENT);
        let Some(executable) = digest_file(&path)? else {
            return Err(at(&path, io::ErrorKind::NotFound.into()));
        };
        let mut hash = Sha256::new();
        for file in [
            ".installed",
            backend::installation::ARCHIVE_ID,
            backend::GAME_BUILD_ARCHITECTURE,
            "sunrise-release.json",
            "research-lock.json",
            "native-mods.json",
        ] {
            item(&mut hash, file.as_bytes(), digest_file(&dir.join(file))?);
        }
        for name in [
            "native-data",
            "native-content",
            sunrise_launcher_smod::mod_list::NATIVE_DIR,
        ] {
            item(&mut hash, name.as_bytes(), tree(&dir.join(name), false)?);
        }
        for entry in entries(dir)? {
            let name = entry.file_name();
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(".dll")
                || lower.ends_with(".dylib")
                || lower.ends_with(".so")
                || lower.contains(".so.")
            {
                let path = entry.path();
                let image = digest_file(&path)?;
                if image.is_none() {
                    return Err(at(&path, io::ErrorKind::NotFound.into()));
                }
                item(&mut hash, name.as_encoded_bytes(), image);
            }
        }
        Ok(Self {
            tag: tag.into(),
            executable,
            runtime: hash.finalize().into(),
        })
    }

    pub(super) fn verify_program(&self, path: &Path) -> Result<(), String> {
        if digest_file(path)? != Some(self.executable) {
            return Err(super::unsettled());
        }
        Ok(())
    }
}

pub(super) fn tree(path: &Path, resources: bool) -> Result<Option<[u8; 32]>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && plain_file(&meta) => {
            let device = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    meta.dev()
                }
                #[cfg(not(unix))]
                {
                    0
                }
            };
            directory(path, 0, device, resources).map(Some)
        }
        Ok(_) => Err(at(path, io::ErrorKind::InvalidData.into())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(at(path, e)),
    }
}

fn directory(path: &Path, depth: usize, _device: u64, resources: bool) -> Result<[u8; 32], String> {
    if depth > 64 {
        return Err(at(path, io::ErrorKind::InvalidData.into()));
    }
    let mut hash = Sha256::new();
    for entry in entries(path)? {
        let name = entry.file_name();
        if depth == 0 && resources && super::resources::generated_cache(&name) {
            continue;
        }
        let entry_path = entry.path();
        let meta = fs::symlink_metadata(&entry_path).map_err(|e| at(&entry_path, e))?;
        if !plain_file(&meta) {
            return Err(at(&entry_path, io::ErrorKind::InvalidData.into()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if _device != meta.dev() {
                return Err(at(&entry_path, io::ErrorKind::InvalidData.into()));
            }
        }
        let image = if meta.is_dir() {
            hash.update(b"d");
            Some(directory(&entry_path, depth + 1, _device, resources)?)
        } else if meta.is_file() {
            hash.update(b"f");
            let image = digest_file(&entry_path)?;
            if image.is_none() {
                return Err(at(&entry_path, io::ErrorKind::NotFound.into()));
            }
            image
        } else {
            return Err(at(&entry_path, io::ErrorKind::InvalidData.into()));
        };
        item(&mut hash, name.as_encoded_bytes(), image);
    }
    Ok(hash.finalize().into())
}

fn entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let read = || fs::read_dir(path)?.collect::<io::Result<Vec<_>>>();
    let mut entries = read().map_err(|e| at(path, e))?;
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn item(hash: &mut Sha256, name: &[u8], image: Option<[u8; 32]>) {
    hash.update((name.len() as u64).to_le_bytes());
    hash.update(name);
    match image {
        None => hash.update([0]),
        Some(bytes) => {
            hash.update([1]);
            hash.update(bytes);
        }
    }
}

fn at(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_covers_library_native_payload_and_archive_but_not_logs() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        fs::write(dir.join(backend::CLIENT), b"game").unwrap();
        fs::write(dir.join(".installed"), []).unwrap();
        fs::create_dir(dir.join("native-data")).unwrap();
        let paths = [
            "native.dll",
            "native-data/data.json",
            backend::GAME_BUILD_ARCHITECTURE,
            backend::installation::ARCHIVE_ID,
        ];
        for path in paths {
            fs::write(dir.join(path), b"original").unwrap();
        }
        let capture = || Artifact::capture_at("tag", dir).unwrap();
        let sealed = capture();
        fs::write(dir.join("latest.log"), b"mutable game log").unwrap();
        assert_eq!(sealed, capture());
        for path in paths {
            fs::write(dir.join(path), b"different").unwrap();
            assert_ne!(sealed, capture(), "{path}");
            fs::write(dir.join(path), b"original").unwrap();
            assert_eq!(sealed, capture(), "{path}");
        }
        fs::write(dir.join("extra.so.1"), b"extra library").unwrap();
        assert_ne!(sealed, capture());
    }
}
