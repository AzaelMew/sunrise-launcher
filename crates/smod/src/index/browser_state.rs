//! Recover the in-game registry's pending publication before the launcher uses its manifest.
//! Mirrors `crates/sunrise-mod-package/src/index/browser_state.rs` in sunrise-org/sunrise.
use super::{MAX_FILE, MAX_INDEX, hex64, sha256_file, valid_id};
use crate::install_lock::lock_home;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MANIFEST: &str = "mod-browser.json";
pub const PENDING: &str = ".mod-browser.pending.json";
// Old and new can come from different indexes. Each row stores the version label twice,
// so allow two index budgets per row, plus space for fixed fields and the journal envelope.
const MAX_PENDING: u64 = 4 * MAX_INDEX + (4 << 10);

#[derive(Deserialize)]
struct Pending {
    id: String,
    file: String,
    old: Option<Value>,
    new: Value,
}

/// Settle an interrupted game install. The usual case has no pending file and does not wait
/// for a concurrent download. Writers still hold the home lock across read, modify and write.
pub fn reconcile(home: &Path) -> Result<(), String> {
    let path = home.join(PENDING);
    // The common case does not need to take the shared game/launcher lock.
    if path
        .symlink_metadata()
        .is_err_and(|e| e.kind() == ErrorKind::NotFound)
    {
        return Ok(());
    }
    let _lock = lock_home(home).map_err(|e| format!("{}: {e}", home.display()))?;
    reconcile_locked(home, &path, sync_parent)
}

fn reconcile_locked(
    home: &Path,
    path: &Path,
    sync: impl Fn(&Path) -> io::Result<()>,
) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    match path.symlink_metadata() {
        Ok(info) if !info.is_file() => return Err(error(&"not a regular file")),
        Ok(info) if info.len() > MAX_PENDING => return Err(error(&"pending entry too large")),
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(error(&e)),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(MAX_PENDING + 1).read_to_end(&mut bytes))
        .map_err(|e| error(&e))?;
    if bytes.len() as u64 > MAX_PENDING {
        return Err(error(&"pending entry too large"));
    }
    let pending: Pending = serde_json::from_slice(&bytes).map_err(|e| error(&e))?;
    let sha = pending.new["checksum"]
        .as_str()
        .and_then(|hash| hash.strip_prefix("SHA-256:"))
        .ok_or_else(|| error(&"missing SHA-256"))?;
    let valid_row = |row: &Value| owned_by(row, &pending.id) && row["file"] == pending.file;
    if !valid_id(&pending.id)
        || pending.file != format!("{}/{}.smod", crate::mod_list::DIR, pending.id)
        || !valid_row(&pending.new)
        || !pending.new["version"].is_string()
        || !pending.old.as_ref().is_none_or(valid_row)
    {
        return Err(error(&"invalid pending entry"));
    }
    hex64(sha).map_err(|e| error(&e))?;

    let target = home.join(&pending.file);
    let published = match target.symlink_metadata() {
        Ok(info) if info.is_file() && info.len() <= MAX_FILE => sha256_file(&target)? == sha,
        Ok(_) => false,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(format!("{}: {e}", target.display())),
    };
    if published {
        let manifest = home.join(MANIFEST);
        let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", manifest.display());
        // Do not round-trip through the launcher's typed rows: other sources and future fields
        // must survive recovery unchanged, and their file claims still count.
        let mut document: Value = match fs::read(&manifest) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| fail(&e))?,
            Err(e) if e.kind() == ErrorKind::NotFound => serde_json::json!({ "entries": [] }),
            Err(e) => return Err(fail(&e)),
        };
        let entries = document
            .get_mut("entries")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| fail(&"entries must be an array"))?;
        let mut list = Installed {
            entries: std::mem::take(entries),
        };
        let current = list.single(&pending.id)?.map(|i| &list.entries[i]);
        if list.taken_by(&pending.file, &pending.id).is_some() {
            return Err(error(&"another entry claims its file"));
        }
        if current == Some(&pending.new) {
            // The row may be visible before its rename is durable.
            sync(&manifest).map_err(|e| fail(&e))?;
        } else if current == pending.old.as_ref() {
            sync(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            list.record(pending.new);
            *entries = list.entries;
            let bytes = serde_json::to_vec_pretty(&document).map_err(|e| fail(&e))?;
            save(&manifest, &bytes).map_err(|e| fail(&e))?;
        } else {
            return Err(error(
                &"the project's entry changed since the install began",
            ));
        }
    }
    // Old or unrelated bytes keep the old row. Don't discard recovery data after an I/O error.
    match fs::remove_file(path) {
        Ok(()) => sync(path).map_err(|e| error(&e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(error(&e)),
    }
}

/// Whether `entry` is the registry's row for project `id`.
fn owned_by(entry: &Value, id: &str) -> bool {
    entry["source"] == "sunrise" && entry["project"] == id
}

/// A manifest file claim as a key that equal paths share: its components relative to the
/// home, `.` ones left out, and on Windows with `\` a separator, case ignored and trailing dots
/// and spaces dropped, as Win32 paths resolve them. `None` for a claim that can't be placed
/// without the filesystem (absolute, or with `..`), which may alias any file.
fn claim_key(path: &str) -> Option<Vec<String>> {
    let mut key = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                key.push(if cfg!(windows) {
                    name.trim_end_matches(['.', ' ']).to_ascii_lowercase()
                } else {
                    name.into_owned()
                });
            }
            _ => return None,
        }
    }
    Some(key)
}

/// `mod-browser.json`, kept as JSON values so the launcher's entries this
/// game doesn't model (Modrinth resource packs, ported Java mods) are written
/// back as they were.
#[derive(Debug, Default)]
pub struct Installed {
    pub entries: Vec<Value>,
}

impl Installed {
    /// Reads the raw rows after settling any pending game publication.
    pub fn load(home: &Path) -> Result<Self, String> {
        let _lock = lock_home(home).map_err(|e| format!("{}: {e}", home.display()))?;
        reconcile(home)?;
        Self::read(home)
    }

    fn read(home: &Path) -> Result<Self, String> {
        let path = home.join(MANIFEST);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let value: Value =
            serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        let entries = value["entries"]
            .as_array()
            .ok_or_else(|| format!("{}: entries must be an array", path.display()))?
            .clone();
        Ok(Self { entries })
    }

    /// The registry's one entry for project `id`, if any. Several are refused: which one holds
    /// later edits is unknown, and replacing them would lose the others.
    pub fn single(&self, id: &str) -> Result<Option<usize>, String> {
        let mut rows = (0..self.entries.len()).filter(|&i| owned_by(&self.entries[i], id));
        match (rows.next(), rows.next()) {
            (_, Some(_)) => Err(format!("several entries in {MANIFEST} name {id}")),
            (first, None) => Ok(first),
        }
    }

    /// The title of another entry that owns `file` however its path is spelled, or whose path
    /// can't be placed ([`claim_key`]) and so may own it.
    pub fn taken_by(&self, file: &str, id: &str) -> Option<String> {
        let key = claim_key(file);
        self.entries
            .iter()
            .find(|e| {
                !owned_by(e, id)
                    && e["file"].as_str().is_some_and(|other| {
                        let other = claim_key(other);
                        other.is_none() || other == key
                    })
            })
            .map(|e| e["title"].as_str().unwrap_or("another mod").to_owned())
    }

    fn record(&mut self, entry: Value) {
        let id = entry["project"].clone();
        self.entries
            .retain(|e| !(e["source"] == "sunrise" && e["project"] == id));
        self.entries.push(entry);
    }

    /// Check ownership before a launcher install downloads or publishes a package.
    pub fn check_install(&self, id: &str, file: &str) -> Result<(), String> {
        self.single(id)?;
        if let Some(other) = self.taken_by(file, id) {
            return Err(format!("{file} belongs to {other}: remove it first."));
        }
        Ok(())
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let dir = fs::File::open(parent)?;
        #[cfg(not(target_os = "macos"))]
        dir.sync_all()?;
        // A directory has no data for macOS's regular-file F_FULLFSYNC barrier.
        #[cfg(target_os = "macos")]
        loop {
            use std::os::fd::AsRawFd;
            // SAFETY: the descriptor is valid while `dir` is open.
            if unsafe { libc::fsync(dir.as_raw_fd()) } == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    // The game uses a write-through move on Windows, where dirs cannot be fsynced.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    for _ in 0..16 {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary =
            path.with_file_name(format!(".mod-browser-{}-{id}.part", std::process::id()));
        let mut file = match fs::File::create_new(&temporary) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        return written
            .and_then(|()| replace(&temporary, path))
            .inspect_err(|_| {
                let _ = fs::remove_file(&temporary);
            });
    }
    Err(ErrorKind::AlreadyExists.into())
}

fn replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let wide = |path: &Path| -> io::Result<Vec<u16>> {
            let parent = path.parent().expect("file in the game home");
            let name = path.file_name().expect("a filename");
            Ok(parent
                .canonicalize()?
                .join(name)
                .as_os_str()
                .encode_wide()
                .chain([0])
                .collect())
        };
        let (from, to) = (wide(from)?, wide(to)?);
        // SAFETY: both buffers are NUL-terminated and live through this synchronous call.
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)?;
        sync_parent(to)?;
        // Flush the device after the directory metadata barrier on APFS.
        #[cfg(target_os = "macos")]
        fs::File::open(to)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
