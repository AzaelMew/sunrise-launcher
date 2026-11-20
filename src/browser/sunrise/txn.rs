//! One transaction record per profile: what the installs of a build changed, kept until the
//! build ends. A launcher that closes or crashes at any point leaves this file, and the next
//! start ([`recover`]) puts every file back as it was: what was added is removed, what was
//! replaced or removed comes back, the installed list and the enabled list included.
//!
//! The preimages are not estimated: they are the files the installer itself moves aside into a
//! staging folder under the publish lock, and the record names each step. An attempt goes
//! *pending* (written, before anything is published), *published* (in place, waiting for the
//! build) and then *done* ([`done`], the build worked) or *undone* ([`undo`]). The record is
//! written whole beside the old one and renamed over it; if it can't be written, nothing is
//! published. One that can't be undone stays, and the profile takes no new install until it is.
use crate::browser::installed::Approval;
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const FILE: &str = "mod-browser/install-txn.json";
const VERSION: u64 = 1;

/// One file the install puts in place, relative to the profile.
#[derive(Clone, Serialize, Deserialize)]
struct Step {
    to: PathBuf,
    /// Whether a file was there before: it moves to `backup` first.
    existed: bool,
    backup: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum State {
    /// Written before anything is published: the files may be half moved.
    Pending,
    /// In place, waiting for the build the install asked for.
    Published,
}

#[derive(Clone, Serialize, Deserialize)]
struct Attempt {
    id: String,
    state: State,
    /// The folder in the profile holding the exact preimages (the files moved aside).
    staging: PathBuf,
    steps: Vec<Step>,
    /// What the player approved removing with it, and whether they agreed to what worlds lose.
    approvals: Vec<Approval>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u64,
    /// The build tag the installs wait for, once it started.
    build: Option<String>,
    attempts: Vec<Attempt>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            version: VERSION,
            build: None,
            attempts: Vec::new(),
        }
    }
}

/// The record, or none. One that can't be read, or that another version made, is never deleted
/// or guessed at: it is an error that stops every change in the profile.
fn read(home: &Path) -> Result<Option<Record>, String> {
    let file = home.join(FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let kept = |why: String| {
        trf(
            "{file} is kept as it is and can't be used: {why}",
            &[("file", &file.display().to_string()), ("why", &why)],
        )
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| kept(e.to_string()))?;
    if value["version"].as_u64() != Some(VERSION) {
        return Err(kept(
            tr("another version of the launcher made it").to_string(),
        ));
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|e| kept(e.to_string()))
}

/// Writes `record` whole beside the old one, flushed, and renames it over: the old record stays
/// whole until the new one is complete. An empty one takes the file away.
fn write(home: &Path, record: &Record) -> Result<(), String> {
    let file = home.join(FILE);
    if record.attempts.is_empty() {
        return match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", file.display()))
            }
            _ => Ok(()),
        };
    }
    let dir = file.parent().expect("inside the profile");
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let next = file.with_extension("json.new");
    let text = serde_json::to_string(record).map_err(|e| e.to_string())?;
    let flushed = fs::File::create(&next).and_then(|mut out| {
        std::io::Write::write_all(&mut out, text.as_bytes())?;
        out.sync_all()
    });
    flushed.map_err(|e| format!("{}: {e}", next.display()))?;
    fs::rename(&next, &file).map_err(|e| format!("{}: {e}", file.display()))?;
    // The rename itself is flushed too, where the system lets a folder be opened.
    if let Ok(folder) = fs::File::open(dir) {
        let _ = folder.sync_all();
    }
    Ok(())
}

/// Before anything is published: writes the attempt that moves `changes` (where each file goes)
/// into the record as pending, its replaced files waiting in the staging folder `staged`, which
/// sits in the profile. Returns its id. If this fails nothing was changed, and the install stops.
pub(super) fn begin(
    home: &Path,
    staged: &Path,
    changes: &[PathBuf],
    approvals: &[Approval],
) -> Result<String, String> {
    let mut record = read(home)?.unwrap_or_default();
    if record.attempts.iter().any(|a| a.state == State::Pending) {
        return Err(unsettled());
    }
    let relative = |path: &Path| {
        path.strip_prefix(home).map(Path::to_path_buf).map_err(|_| {
            trf(
                "{path} is outside the profile",
                &[("path", &path.display().to_string())],
            )
        })
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
    let id = relative(staged)?.display().to_string();
    record.attempts.push(Attempt {
        id: id.clone(),
        state: State::Pending,
        staging: relative(staged)?,
        steps,
        approvals: approvals.to_vec(),
    });
    write(home, &record)?;
    Ok(id)
}

/// For tests of callers: an attempt begun and left cut off.
#[cfg(test)]
pub(super) fn begin_for_test(home: &Path, staged: &Path, changes: &[PathBuf]) {
    begin(home, staged, changes, &[]).unwrap();
}

fn unsettled() -> String {
    tr("An earlier install is still being undone. Restart the launcher and try again.").to_string()
}

/// The files of `attempt` are put back, newest step first; true when all are. The staging
/// folder goes only then.
fn restore(home: &Path, attempt: &Attempt) -> bool {
    let mut undone = true;
    for step in attempt.steps.iter().rev() {
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
        let _ = fs::remove_dir_all(home.join(&attempt.staging));
    }
    undone
}

/// The attempt `id` was undone on the spot (its publish failed and the files are back): it
/// leaves the record.
pub(super) fn abandon(home: &Path, id: &str) -> Result<(), String> {
    let mut record = read(home)?.unwrap_or_default();
    record.attempts.retain(|a| a.id != id);
    write(home, &record)
}

/// An install failed with `message` after (or while) its files moved: the attempt is undone on
/// the spot. When that works the message is returned as it is; when it doesn't, the record and
/// the staging folder (the backups) stay, and the message says to restart.
pub(super) fn rollback(
    home: &Path,
    id: &str,
    staged: tempfile::TempDir,
    message: String,
) -> String {
    let attempt = read(home)
        .ok()
        .flatten()
        .and_then(|r| r.attempts.into_iter().find(|a| a.id == id));
    match attempt {
        Some(a) if restore(home, &a) => match abandon(home, id) {
            Ok(()) => message,
            Err(e) => format!("{message}; {e}"),
        },
        _ => {
            let _ = staged.keep();
            format!(
                "{message}. {}",
                tr("Putting the files back failed too: restart the launcher to try again.")
            )
        }
    }
}

/// The install's files are in place. The attempt is published: it stays, with its staging
/// folder, while a build is outstanding (`brings_native`, or an earlier attempt still waits for
/// one); otherwise the install is done and the record goes. If the record can't be written the
/// files are put back and the error is returned.
pub(super) fn commit(
    home: &Path,
    id: &str,
    staged: tempfile::TempDir,
    brings_native: bool,
) -> Result<(), String> {
    let result = read(home).and_then(|record| {
        let mut record = record.unwrap_or_default();
        if let Some(a) = record.attempts.iter_mut().find(|a| a.id == id) {
            a.state = State::Published;
        }
        let waits = brings_native || record.attempts.len() > 1;
        if !waits {
            record.attempts.clear();
        }
        write(home, &record).map(|()| waits)
    });
    match result {
        Ok(true) => {
            // The backups are the record's preimages: they outlive this install.
            let _ = staged.keep();
            Ok(())
        }
        Ok(false) => Ok(()),
        Err(e) => Err(rollback(home, id, staged, e)),
    }
}

/// With the profile locked: puts back every attempt, newest first, each leaving the record as
/// it is restored. One that can't be restored keeps the record (and its staging folder) and
/// returns the error; the profile takes no new install until it is.
pub fn undo(home: &Path) -> Result<(), String> {
    let mut record = read(home)?.unwrap_or_default();
    while let Some(attempt) = record.attempts.last() {
        if !restore(home, attempt) {
            return Err(unsettled());
        }
        record.attempts.pop();
        write(home, &record)?;
    }
    Ok(())
}

/// Undoes only the attempts that were cut off before they were published: what an install
/// does first, so a half-moved set of files never has another install on top of it. Published
/// attempts wait for their build and stay.
pub fn settle(home: &Path) -> Result<(), String> {
    let mut record = read(home)?.unwrap_or_default();
    while let Some(attempt) = record.attempts.last().filter(|a| a.state == State::Pending) {
        if !restore(home, attempt) {
            return Err(unsettled());
        }
        record.attempts.pop();
        write(home, &record)?;
    }
    Ok(())
}

/// At start: undoes whatever an earlier launcher left, published installs whose build never
/// ended included. True when nothing is left to undo.
pub fn recover(home: &Path) -> bool {
    match undo(home) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

/// The build for the outstanding installs began: its tag is kept with them.
pub fn mark_build(home: &Path, tag: &str) {
    if let Ok(Some(mut record)) = read(home) {
        record.build = Some(tag.to_owned());
        let _ = write(home, &record);
    }
}

/// The build worked: the installs are done, and their backups go.
pub fn done(home: &Path) {
    if let Ok(Some(record)) = read(home) {
        for attempt in &record.attempts {
            let _ = fs::remove_dir_all(home.join(&attempt.staging));
        }
        let _ = write(home, &Record::default());
    }
}

/// Whether installs of this profile wait for a build, or a restore is still owed.
pub fn outstanding(home: &Path) -> bool {
    read(home).map_or(true, |r| r.is_some_and(|r| !r.attempts.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staging(home: &Path) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".installed-mods-")
            .tempdir_in(home)
            .unwrap()
    }

    /// A profile with an old dependency and an old list, and an attempt that will replace the
    /// dependency, replace the list and add a mod.
    struct Scene {
        home: tempfile::TempDir,
        dep: PathBuf,
        list: PathBuf,
        added: PathBuf,
    }

    fn scene() -> Scene {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        fs::create_dir_all(h.join("sunrise-mods")).unwrap();
        let (dep, list, added) = (
            h.join("sunrise-mods/flywheel.smod"),
            h.join("mod-browser.json"),
            h.join("sunrise-mods/lumen.smod"),
        );
        fs::write(&dep, "flywheel 0.3").unwrap();
        fs::write(&list, "old list").unwrap();
        Scene {
            home,
            dep,
            list,
            added,
        }
    }

    impl Scene {
        fn home(&self) -> &Path {
            self.home.path()
        }

        fn begin(&self, staged: &tempfile::TempDir) -> String {
            begin(
                self.home(),
                staged.path(),
                &[self.dep.clone(), self.list.clone(), self.added.clone()],
                &[],
            )
            .unwrap()
        }

        /// What publishing does, as far as `steps` of it went.
        fn publish(&self, staged: &tempfile::TempDir, steps: usize) {
            let s = staged.path();
            if steps > 0 {
                fs::rename(&self.dep, s.join("old-0")).unwrap();
                fs::write(&self.dep, "flywheel 0.4").unwrap();
            }
            if steps > 1 {
                fs::rename(&self.list, s.join("old-1")).unwrap();
                fs::write(&self.list, "new list").unwrap();
            }
            if steps > 2 {
                fs::write(&self.added, "lumen").unwrap();
            }
        }

        fn is_as_it_was(&self) {
            assert_eq!(fs::read_to_string(&self.dep).unwrap(), "flywheel 0.3");
            assert_eq!(fs::read_to_string(&self.list).unwrap(), "old list");
            assert!(!self.added.exists(), "what the install added is removed");
        }
    }

    #[test]
    fn a_crash_at_any_step_leaves_the_profile_as_it_was_at_the_next_start() {
        for steps in 0..=3 {
            let scene = scene();
            let staged = staging(scene.home());
            let kept = staged.path().to_path_buf();
            scene.begin(&staged);
            scene.publish(&staged, steps);
            // The launcher is gone: nothing in memory, the staging folder is not dropped.
            let _ = staged.keep();
            assert!(recover(scene.home()), "after {steps} steps");
            scene.is_as_it_was();
            assert!(!kept.exists() && !scene.home().join(FILE).exists());
            // Nothing left to do: another start changes nothing.
            assert!(recover(scene.home()));
            scene.is_as_it_was();
        }
    }

    #[test]
    fn a_published_install_waits_for_its_build_and_a_crash_before_the_build_ends_undoes_it() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        let id = scene.begin(&staged);
        scene.publish(&staged, 3);
        commit(scene.home(), &id, staged, true).unwrap();
        assert!(kept.is_dir(), "the preimages wait with the record");
        assert!(scene.home().join(FILE).is_file());
        // The build never ended. The dependency comes back, it is not deleted as if added.
        assert!(recover(scene.home()));
        scene.is_as_it_was();
    }

    #[test]
    fn a_build_that_worked_clears_the_record_and_the_backups() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        let id = scene.begin(&staged);
        scene.publish(&staged, 3);
        commit(scene.home(), &id, staged, true).unwrap();
        done(scene.home());
        assert!(!kept.exists() && !scene.home().join(FILE).exists());
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
    }

    #[test]
    fn a_plain_install_behind_a_native_one_goes_back_with_it_after_a_crash() {
        let scene = scene();
        let first = staging(scene.home());
        let id = scene.begin(&first);
        scene.publish(&first, 3);
        commit(scene.home(), &id, first, true).unwrap();
        // A plain install queued behind it, finished, with nothing to build: its record stays
        // while the native one waits, so a crash returns the profile to one consistent state.
        let second = staging(scene.home());
        let plain = scene.home().join("sunrise-mods/plain.smod");
        let id = begin(
            scene.home(),
            second.path(),
            &[scene.list.clone(), plain.clone()],
            &[],
        )
        .unwrap();
        fs::rename(&scene.list, second.path().join("old-0")).unwrap();
        fs::write(&scene.list, "newer list").unwrap();
        fs::write(&plain, "plain").unwrap();
        commit(scene.home(), &id, second, false).unwrap();
        assert!(scene.home().join(FILE).is_file());
        assert!(recover(scene.home()));
        scene.is_as_it_was();
        assert!(!plain.exists());
    }

    #[test]
    fn a_plain_install_with_nothing_waiting_leaves_no_record() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        let id = scene.begin(&staged);
        scene.publish(&staged, 3);
        commit(scene.home(), &id, staged, false).unwrap();
        assert!(!kept.exists() && !scene.home().join(FILE).exists());
    }

    #[test]
    fn a_restore_that_fails_keeps_the_record_and_the_backups_and_stops_the_next_install() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        scene.begin(&staged);
        scene.publish(&staged, 1);
        let _ = staged.keep();
        // Something else took the place of the dependency: the old file can't go back.
        fs::remove_file(&scene.dep).unwrap();
        fs::create_dir_all(scene.dep.join("x")).unwrap();
        assert!(!recover(scene.home()));
        assert!(kept.join("old-0").is_file(), "the backups stay");
        assert!(scene.home().join(FILE).is_file(), "and so does the record");
        let next = staging(scene.home());
        let refused = begin(scene.home(), next.path(), &[scene.dep.clone()], &[]).unwrap_err();
        assert!(refused.contains("still being undone"), "{refused}");
        assert!(settle(scene.home()).is_err());
        // Cleared: the restore works, and the profile takes installs again.
        fs::remove_dir_all(&scene.dep).unwrap();
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
        assert!(begin(scene.home(), next.path(), &[scene.dep.clone()], &[]).is_ok());
    }

    #[test]
    fn a_record_that_cannot_be_written_publishes_nothing() {
        let scene = scene();
        // The record's place is a folder: it can be neither read nor replaced.
        fs::create_dir_all(scene.home().join(FILE).join("x")).unwrap();
        let staged = staging(scene.home());
        assert!(begin(scene.home(), staged.path(), &[scene.dep.clone()], &[]).is_err());
        scene.publish(&staged, 0);
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
    }

    #[test]
    fn a_record_of_another_version_is_refused_and_never_deleted() {
        let scene = scene();
        let file = scene.home().join(FILE);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = r#"{"version": 2, "attempts": []}"#;
        fs::write(&file, text).unwrap();
        let staged = staging(scene.home());
        assert!(begin(scene.home(), staged.path(), &[scene.dep.clone()], &[]).is_err());
        assert!(!recover(scene.home()));
        assert!(settle(scene.home()).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
        // Unreadable text is kept too.
        fs::write(&file, "{").unwrap();
        assert!(!recover(scene.home()));
        assert_eq!(fs::read_to_string(&file).unwrap(), "{");
    }

    #[test]
    fn a_failed_publish_is_undone_on_the_spot_and_leaves_no_record() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        let id = scene.begin(&staged);
        scene.publish(&staged, 2);
        let message = rollback(scene.home(), &id, staged, "Cannot install it".into());
        assert_eq!(message, "Cannot install it");
        scene.is_as_it_was();
        assert!(!kept.exists() && !scene.home().join(FILE).exists());
    }

    #[test]
    fn a_publish_whose_rollback_fails_says_to_restart_and_keeps_its_backups() {
        let scene = scene();
        let staged = staging(scene.home());
        let kept = staged.path().to_path_buf();
        let id = scene.begin(&staged);
        scene.publish(&staged, 1);
        fs::remove_file(&scene.dep).unwrap();
        fs::create_dir_all(scene.dep.join("x")).unwrap();
        let message = rollback(scene.home(), &id, staged, "Cannot install it".into());
        assert!(message.contains("restart the launcher"), "{message}");
        assert!(kept.join("old-0").is_file());
        assert!(scene.home().join(FILE).is_file());
    }
}
