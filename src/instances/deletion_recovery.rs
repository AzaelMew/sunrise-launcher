//! Use one record per detached folder. A Dock Quit can end the process without waiting for
//! the Trash worker, so the recovery location must already be on disk when the folder moves.
use super::{durable, RETIRED};
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

const RECORD: &str = ".pending-delete";
const OWNER: &str = ".delete-owner";

pub(super) fn record(retired: &Path, private: &Path, id: &str) -> io::Result<File> {
    let owner = File::create_new(private.join(OWNER))?;
    owner.lock()?;
    durable::replace(retired, &private.join(RECORD), id.as_bytes())?;
    Ok(owner)
}

pub(super) fn settled(private: &Path, owner: File) {
    // Retire the record before releasing the owner so a concurrent startup cannot report it.
    let _ = durable::retire(&private.join(RECORD));
    drop(owner);
    let _ = fs::remove_file(private.join(OWNER));
    let _ = fs::remove_dir(private);
}

pub struct InterruptedDeletion {
    folder: PathBuf,
}

impl InterruptedDeletion {
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Dismiss only the notice. The profile remains where the message said it was kept.
    pub fn acknowledge(self) {
        let parent = self
            .folder
            .parent()
            .expect("inside a private recovery folder");
        let _ = durable::retire(&parent.join(RECORD));
        let _ = fs::remove_file(parent.join(OWNER));
    }
}

pub fn interrupted_deletions() -> Vec<InterruptedDeletion> {
    interrupted_in(&crate::backend::game_home())
}

fn interrupted_in(game_home: &Path) -> Vec<InterruptedDeletion> {
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
            let owner = File::options()
                .read(true)
                .write(true)
                .open(private.join(OWNER))
                .ok()?;
            owner.try_lock().ok()?;
            let record = private.join(RECORD);
            let meta = fs::symlink_metadata(&record).ok()?;
            if !meta.is_file() || crate::native_cache::linked(&meta) {
                return None;
            }
            let id = fs::read_to_string(&record).ok()?;
            let path = Path::new(&id);
            if path.file_name()? != path.as_os_str() {
                return None;
            }
            let folder = private.join(path);
            if real_directory(&folder) {
                Some(InterruptedDeletion { folder })
            } else {
                // The process stopped before detachment, or after the folder reached Trash.
                settled(&private, owner);
                None
            }
        })
        .collect()
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
        let owner = record(&retired, &private, "cabin").unwrap();
        let folder = private.join("cabin");
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
        drop(record(&retired, &private, "cabin").unwrap());
        assert!(interrupted_in(home.path()).is_empty());
        assert!(!private.exists());
    }
}
