//! Recover the in-game registry's pending publication before the launcher uses its manifest.
//! Mirrors `crates/sunrise-mod-package/src/index/browser_state.rs` in sunrise-org/sunrise.
use super::{MAX_FILE, MAX_INDEX, hex64, sha256_file, valid_id};
use crate::install_lock::lock_home;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;
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
    // Probe without taking the lock first: views also load the manifest on the UI thread.
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
    let ours = |row: &Value| row["source"] == "sunrise" && row["project"] == pending.id;
    let valid_row = |row: &Value| ours(row) && row["file"] == pending.file;
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
        // Compare parsed rows so formatting changes don't matter but metadata edits do.
        let current = entries.iter().find(|row| ours(row));
        if current == Some(&pending.new) {
            // The row may be visible before its rename is durable. This also handles a
            // completed Windows install whose sidecar deletion did not survive power loss.
            sync(&manifest).map_err(|e| fail(&e))?;
        } else if current == pending.old.as_ref()
            && !entries
                .iter()
                .any(|row| row["file"] == pending.file && !ours(row))
        {
            // The game could have died after the package rename, before the directory sync.
            sync(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            entries.retain(|row| !ours(row));
            entries.push(pending.new);
            let bytes = serde_json::to_vec_pretty(&document).map_err(|e| fail(&e))?;
            save(&manifest, &bytes).map_err(|e| fail(&e))?;
        }
    }
    // Old or unrelated bytes keep the old row. Don't discard recovery data after an I/O error.
    match fs::remove_file(path) {
        Ok(()) => sync(path).map_err(|e| error(&e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(error(&e)),
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
