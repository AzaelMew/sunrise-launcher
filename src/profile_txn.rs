//! Exclusive ownership of an existing profile, including an optional inherited child handle.

#[doc(hidden)]
pub mod durable;
#[doc(hidden)]
pub mod progress;

use cap_fs_ext::MetadataExt;
use cap_std::{ambient_authority, fs::Dir};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Moving a managed profile can change its path but never the filesystem identity of a
/// directory referenced by an in-flight transaction.
#[derive(Clone, Copy, PartialEq, Eq)]
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
}

#[cfg(test)]
const RECORD: &str = "mod-browser/install-txn.json";

#[cfg(test)]
mod tests;
