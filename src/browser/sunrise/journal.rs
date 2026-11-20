//! The undo list of an install that is moving files into the profile. A failure inside the
//! launcher is undone on the spot; a launcher that closes in the middle leaves this file, and
//! the next start ([`recover`]) puts every file back as it was: what the install added is
//! removed, what it replaced comes back, the installed list included.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const FILE: &str = "mod-browser/install-journal.json";

/// One file the install puts in place, relative to the profile.
#[derive(Serialize, Deserialize)]
struct Step {
    to: PathBuf,
    /// Whether a file was there before: it moves to `backup` first.
    existed: bool,
    backup: PathBuf,
}

/// Writes the list for `changes` (where each file goes), whose replaced files wait in the
/// staging folder `staged`, which sits in the profile.
pub(super) fn begin(home: &Path, staged: &Path, changes: &[PathBuf]) -> Result<(), String> {
    let relative = |path: &Path| {
        path.strip_prefix(home)
            .map(Path::to_path_buf)
            .map_err(|_| format!("{} is outside the profile", path.display()))
    };
    let steps = changes
        .iter()
        .enumerate()
        .map(|(i, to)| {
            Ok(Step {
                to: relative(to)?,
                existed: fs::symlink_metadata(to).is_ok(),
                backup: relative(&staged.join(format!("old-{i}")))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let file = home.join(FILE);
    let text = serde_json::to_string(&steps).map_err(|e| e.to_string())?;
    fs::create_dir_all(file.parent().expect("inside the profile"))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    // Never over a list that is still waiting to be undone: it is the only record of how.
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    std::io::Write::write_all(&mut out, text.as_bytes())
        .map_err(|e| format!("{}: {e}", file.display()))
}

/// The install ended, or was undone: the list is no longer needed. It must go, or the next
/// start would undo an install that is done.
pub(super) fn finish(home: &Path) -> Result<(), String> {
    match fs::remove_file(home.join(FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", home.join(FILE).display()))
        }
        _ => Ok(()),
    }
}

/// The install's last steps, with its files in place and the list still kept: if `after` fails,
/// or the list can't be cleared, the files are put back and the error is returned. Only a list
/// that is gone means the install is done. The staging folder `staged` holds the backups the
/// list points at: it goes with a finished install or a completed undo, and stays (for the
/// next start's [`recover`]) when putting the files back failed.
pub(super) fn commit(
    home: &Path,
    staged: tempfile::TempDir,
    after: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let result = after().and_then(|()| finish(home));
    let Err(e) = result else {
        return Ok(());
    };
    if recover(home) {
        Err(e)
    } else {
        let _ = staged.keep();
        Err(format!(
            "{e}. Putting the files back failed too: restart the launcher to try again."
        ))
    }
}

/// With the profile locked: undoes an install that was left half done, at start or before the
/// next install. Files are put back in reverse order. A step that fails keeps the list for the
/// next try. True when nothing is left to undo.
pub fn recover(home: &Path) -> bool {
    let Ok(text) = fs::read_to_string(home.join(FILE)) else {
        return true;
    };
    let Ok(steps) = serde_json::from_str::<Vec<Step>>(&text) else {
        return finish(home).is_ok();
    };
    let mut undone = true;
    for step in steps.iter().rev() {
        let (to, backup) = (home.join(&step.to), home.join(&step.backup));
        let result = match (step.existed, backup.is_file()) {
            // Moved aside, and the new file may or may not have come: the old one returns.
            (true, true) => fs::rename(&backup, &to),
            // Never reached: the old file is still there.
            (true, false) => Ok(()),
            (false, _) if to.is_file() => fs::remove_file(&to),
            (false, _) => Ok(()),
        };
        if let Err(e) = result {
            eprintln!("Couldn't undo {}: {e}", to.display());
            undone = false;
        }
    }
    if undone {
        // The staging folder with whatever is left in it.
        if let Some(dir) = steps
            .first()
            .and_then(|s| home.join(&s.backup).parent().map(Path::to_path_buf))
        {
            let _ = fs::remove_dir_all(dir);
        }
        return finish(home).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_that_closed_mid_install_leaves_the_profile_as_it_was() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let staged = home.join(".installed-mods-x");
        fs::create_dir_all(&staged).unwrap();
        let (list, updated, added, untouched) = (
            home.join("mod-browser.json"),
            home.join("sunrise-mods/dep.smod"),
            home.join("sunrise-mods/new.smod"),
            home.join("sunrise-mods/later.smod"),
        );
        fs::create_dir_all(home.join("sunrise-mods")).unwrap();
        for (path, text) in [
            (&list, "old list"),
            (&updated, "old dep"),
            (&untouched, "old later"),
        ] {
            fs::write(path, text).unwrap();
        }
        begin(
            home,
            &staged,
            &[
                list.clone(),
                updated.clone(),
                added.clone(),
                untouched.clone(),
            ],
        )
        .unwrap();
        // The install got as far as the new file: the list and the dependency moved aside and
        // were replaced, and the added file is in place. The last one was never reached.
        fs::rename(&list, staged.join("old-0")).unwrap();
        fs::write(&list, "new list").unwrap();
        fs::rename(&updated, staged.join("old-1")).unwrap();
        fs::write(&updated, "new dep").unwrap();
        fs::write(&added, "new").unwrap();

        recover(home);

        assert_eq!(fs::read_to_string(&list).unwrap(), "old list");
        assert_eq!(fs::read_to_string(&updated).unwrap(), "old dep");
        assert!(!added.exists(), "what the install added is removed");
        assert_eq!(fs::read_to_string(&untouched).unwrap(), "old later");
        assert!(!staged.exists() && !home.join(FILE).exists());
        // Nothing left to do: another start changes nothing.
        recover(home);
        assert_eq!(fs::read_to_string(&list).unwrap(), "old list");
    }

    #[test]
    fn an_install_that_finished_leaves_no_list() {
        let home = tempfile::tempdir().unwrap();
        let staged = home.path().join(".installed-mods-x");
        fs::create_dir_all(&staged).unwrap();
        begin(home.path(), &staged, &[home.path().join("a.smod")]).unwrap();
        assert!(home.path().join(FILE).is_file());
        finish(home.path()).unwrap();
        assert!(!home.path().join(FILE).exists());
    }

    #[test]
    fn a_list_still_waiting_is_never_overwritten_by_the_next_install() {
        let home = tempfile::tempdir().unwrap();
        let staged = home.path().join(".installed-mods-x");
        fs::create_dir_all(&staged).unwrap();
        begin(home.path(), &staged, &[home.path().join("a.smod")]).unwrap();
        let first = fs::read_to_string(home.path().join(FILE)).unwrap();
        assert!(home.path().join(FILE).exists());
        assert!(begin(home.path(), &staged, &[home.path().join("b.smod")]).is_err());
        assert_eq!(fs::read_to_string(home.path().join(FILE)).unwrap(), first);
    }

    #[test]
    fn a_failure_after_the_files_are_in_place_puts_them_back_and_clears_the_list() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let (old, new) = (home.join("dep.smod"), home.join("new.smod"));
        fs::write(&old, "old dep").unwrap();
        begin(home, staged.path(), &[old.clone(), new.clone()]).unwrap();
        fs::rename(&old, staged.path().join("old-0")).unwrap();
        fs::write(&old, "new dep").unwrap();
        fs::write(&new, "new").unwrap();

        let error = commit(home, staged, || Err("copy failed".into())).unwrap_err();

        assert_eq!(error, "copy failed");
        assert_eq!(fs::read_to_string(&old).unwrap(), "old dep");
        assert!(!new.exists() && !home.join(FILE).exists());
    }

    #[test]
    fn a_commit_that_went_through_keeps_the_files_and_clears_the_list() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let new = home.join("new.smod");
        begin(home, staged.path(), std::slice::from_ref(&new)).unwrap();
        fs::write(&new, "new").unwrap();
        commit(home, staged, || Ok(())).unwrap();
        assert!(new.is_file() && !home.join(FILE).exists());
    }

    #[test]
    fn a_list_that_cannot_be_cleared_undoes_the_install_instead_of_leaving_it_behind() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let new = home.join("new.smod");
        begin(home, staged.path(), std::slice::from_ref(&new)).unwrap();
        fs::write(&new, "new").unwrap();
        // The list is a folder now, which remove_file can't take away: not clearable.
        fs::remove_file(home.join(FILE)).unwrap();
        fs::create_dir_all(home.join(FILE).join("x")).unwrap();
        assert!(commit(home, staged, || Ok(())).is_err());
    }

    #[test]
    fn backups_the_list_points_at_stay_when_the_files_could_not_be_put_back() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let backups = staged.path().to_path_buf();
        let old = home.join("dep.smod");
        fs::write(&old, "old dep").unwrap();
        begin(home, staged.path(), std::slice::from_ref(&old)).unwrap();
        fs::rename(&old, backups.join("old-0")).unwrap();
        // Something else took the place: the old file can't go back there.
        fs::create_dir_all(old.join("x")).unwrap();
        let error = commit(home, staged, || Err("copy failed".into())).unwrap_err();
        assert!(error.contains("Putting the files back failed"), "{error}");
        assert!(
            backups.join("old-0").is_file(),
            "the next start still has them"
        );
        assert!(home.join(FILE).is_file());
    }

    #[test]
    fn a_finished_install_and_a_completed_undo_leave_no_staging_folder() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let new = home.join("new.smod");
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let backups = staged.path().to_path_buf();
        begin(home, staged.path(), std::slice::from_ref(&new)).unwrap();
        fs::write(&new, "new").unwrap();
        commit(home, staged, || Ok(())).unwrap();
        assert!(!backups.exists());
        let staged = tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap();
        let backups = staged.path().to_path_buf();
        begin(home, staged.path(), std::slice::from_ref(&new)).unwrap();
        assert!(commit(home, staged, || Err("no".into())).is_err());
        assert!(!backups.exists());
    }
}
