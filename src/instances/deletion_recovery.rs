//! Use one record per detached folder. A Dock Quit can end the process without waiting for
//! the Trash worker, so the recovery location must already be on disk when the folder moves.
use super::{durable, RETIRED};
use crate::browser::{safe_name, MAX_SAFE_NAME_BYTES};
use std::{
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
};

const RECORD: &str = ".pending-delete";
const OWNER: &str = ".delete-owner";

#[derive(Debug)]
pub struct InterruptedDeletion {
    folder: PathBuf,
    id: String,
    owner: File,
}

impl InterruptedDeletion {
    pub(super) fn record(retired: &Path, folder: PathBuf, id: String) -> io::Result<Self> {
        let private = folder.parent().expect("inside a private recovery folder");
        let owner = File::create_new(private.join(OWNER))?;
        owner.lock()?;
        durable::replace(retired, &private.join(RECORD), id.as_bytes())?;
        Ok(Self { folder, id, owner })
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn retained(self) -> Option<Self> {
        match fs::symlink_metadata(&self.folder) {
            Ok(meta) if meta.is_dir() && !crate::native_cache::linked(&meta) => Some(self),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // The process quit before detachment, the move was undone, or Trash completed.
                self.acknowledge();
                None
            }
            // An unreadable or unexpected path doesn't prove the folder is gone.
            _ => None,
        }
    }

    /// Dismiss only the notice. The profile remains where the message said it was kept.
    pub fn acknowledge(self) {
        let parent = self
            .folder
            .parent()
            .expect("inside a private recovery folder");
        // Keep the owner marker when retirement fails so the next start can find the record.
        if durable::retire(&parent.join(RECORD)).is_ok() {
            drop(self.owner);
            let _ = fs::remove_file(parent.join(OWNER));
            let _ = fs::remove_dir(parent);
        }
    }
}

pub fn interrupted_deletions() -> Vec<InterruptedDeletion> {
    interrupted_in(&crate::backend::game_home())
}

pub(super) fn interrupted_in(game_home: &Path) -> Vec<InterruptedDeletion> {
    let root = game_home.join(RETIRED);
    if !real_directory(&root) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let private = entry.path();
            if !real_directory(&private) {
                return None;
            }
            let owner = open_recovery_file(&private.join(OWNER), true)?;
            owner.try_lock().ok()?;
            let mut id = String::new();
            open_recovery_file(&private.join(RECORD), false)?
                .take((MAX_SAFE_NAME_BYTES + 1) as u64)
                .read_to_string(&mut id)
                .ok()?;
            let folder = private.join(safe_name(&id)?);
            InterruptedDeletion { folder, id, owner }.retained()
        })
        .collect()
}

fn open_recovery_file(path: &Path, write: bool) -> Option<File> {
    let regular = |meta: &fs::Metadata| meta.is_file() && !crate::native_cache::linked(meta);
    if !regular(&fs::symlink_metadata(path).ok()?) {
        return None;
    }
    let mut options = File::options();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).ok()?;
    regular(&file.metadata().ok()?).then_some(file)
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_dir() && !crate::native_cache::linked(&meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unfinished_detachment_is_reported_and_dismissal_keeps_its_worlds() {
        let home = tempfile::tempdir().unwrap();
        let retired = home.path().join(RETIRED);
        let private = retired.join("profile-move");
        fs::create_dir_all(&private).unwrap();
        let folder = private.join("cabin");
        let owner = InterruptedDeletion::record(&retired, folder.clone(), "cabin".into()).unwrap();
        let world = folder.join("saves/World/level.dat");
        fs::create_dir_all(world.parent().unwrap()).unwrap();
        fs::write(&world, b"keep").unwrap();
        assert!(
            interrupted_in(home.path()).is_empty(),
            "worker still owns the move"
        );
        drop(owner); // As when a native Quit ends the process without waiting for the worker.
        let mut notices = interrupted_in(home.path());
        assert_eq!(notices.len(), 1);
        let notice = notices.pop().unwrap();
        assert_eq!(notice.folder(), folder);
        notice.acknowledge();
        assert!(interrupted_in(home.path()).is_empty());
        assert_eq!(fs::read(world).unwrap(), b"keep");
    }

    #[test]
    fn a_record_without_a_detached_folder_does_not_claim_a_profile_was_retained() {
        let home = tempfile::tempdir().unwrap();
        let retired = home.path().join(RETIRED);
        let private = retired.join("profile-move");
        fs::create_dir_all(&private).unwrap();
        drop(InterruptedDeletion::record(&retired, private.join("cabin"), "cabin".into()).unwrap());
        assert!(interrupted_in(home.path()).is_empty());
        assert!(!private.exists());
    }

    #[test]
    fn an_unexpected_path_or_failed_retirement_leaves_the_recovery_markers() {
        let home = tempfile::tempdir().unwrap();
        let retired = home.path().join(RETIRED);
        let private = retired.join("profile-move");
        fs::create_dir_all(&private).unwrap();
        let folder = private.join("cabin");
        drop(InterruptedDeletion::record(&retired, folder.clone(), "cabin".into()).unwrap());
        fs::write(&folder, b"not a directory").unwrap();
        assert!(interrupted_in(home.path()).is_empty());
        assert!(private.join(RECORD).is_file());
        assert!(private.join(OWNER).is_file());
        fs::remove_file(&folder).unwrap();
        fs::create_dir(&folder).unwrap();
        let notice = interrupted_in(home.path()).pop().unwrap();
        let record = private.join(RECORD);
        #[cfg(windows)]
        {
            let blocked = record.with_extension("json.retired");
            fs::create_dir(&blocked).unwrap();
            fs::write(blocked.join("block"), b"keep").unwrap();
        }
        #[cfg(not(windows))]
        {
            fs::remove_file(&record).unwrap();
            fs::create_dir(&record).unwrap();
        }
        notice.acknowledge();
        assert!(record.exists());
        assert!(private.join(OWNER).is_file());
    }

    #[test]
    fn startup_ignores_invalid_names_oversized_records_and_non_regular_owners() {
        let home = tempfile::tempdir().unwrap();
        let private = home.path().join(RETIRED).join("profile-move");
        fs::create_dir_all(&private).unwrap();
        let record = private.join(RECORD);
        let owner = private.join(OWNER);
        fs::write(&owner, b"").unwrap();
        fs::create_dir(private.join(".hidden")).unwrap();
        let long = "x".repeat(MAX_SAFE_NAME_BYTES + 1);
        for bad in [".hidden", "../profile-move", long.as_str()] {
            fs::write(&record, bad).unwrap();
            assert!(interrupted_in(home.path()).is_empty(), "accepted {bad}");
            assert!(record.is_file(), "cleaned invalid record {bad}");
        }
        fs::create_dir(private.join("cabin")).unwrap();
        fs::write(&record, "cabin").unwrap();
        fs::remove_file(&owner).unwrap();
        fs::create_dir(&owner).unwrap();
        assert!(interrupted_in(home.path()).is_empty());
        #[cfg(unix)]
        {
            fs::remove_dir(&owner).unwrap();
            let real_owner = home.path().join("lock");
            fs::write(&real_owner, b"").unwrap();
            std::os::unix::fs::symlink(real_owner, &owner).unwrap();
            assert!(interrupted_in(home.path()).is_empty());
        }
    }
}
