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
//!
//! The record's root names its kind ([`Kind`]): `install` now, and a version switch's attempt
//! later, never both in one file. A record of another kind, another version or that can't be read
//! blocks the profile and is never guessed at. [`begin`], [`done`], [`undo`] and the cleanup act
//! only on the attempt ids they are given, so a late call from an older build can't touch a newer
//! attempt. The files earlier development builds and the automatic updates left are reconciled
//! once per profile, under the same lock, before anything else ([`reconcile_legacy`]).
use crate::browser::installed::Approval;
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

mod durable;

const FILE: &str = "mod-browser/install-txn.json";
const VERSION: u64 = 1;

/// The live owner of one profile's install batch. The file is never removed: all launchers
/// must contend on the same inode, even when the record itself is replaced or absent.
pub struct Lease {
    home: PathBuf,
    file: fs::File,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // A concurrent fork can briefly inherit the handle before exec closes it. On normal
        // release unlock explicitly, rather than leaving it to the last inherited handle.
        let _ = self.file.unlock();
    }
}

impl Lease {
    pub fn try_acquire(home: &Path) -> Result<Option<Self>, String> {
        fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
        let path = home.join(".install-txn.lock");
        let fail = |e| format!("{}: {e}", path.display());
        let file = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(fail)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                home: home.to_path_buf(),
                file,
            })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(e)) => Err(fail(e)),
        }
    }

    pub fn acquire(home: &Path) -> Result<Self, String> {
        Self::try_acquire(home)?.ok_or_else(|| {
            tr("Another launcher is working on this profile. Try again when it finishes.")
                .to_string()
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// A freshly acquired lease can clean up an abandoned batch before doing new work.
    pub fn recover_abandoned(&self) -> Result<(), String> {
        let _lock = locked(&self.home)?;
        reconcile_legacy(&self.home)?;
        let ids: Vec<String> = read(&self.home)?
            .unwrap_or_default()
            .attempts
            .into_iter()
            .map(|a| a.id)
            .collect();
        undo(&self.home, &ids)
    }

    pub fn outstanding(&self) -> bool {
        outstanding(&self.home)
    }

    pub fn mark_build(&self, tag: &str) -> Result<Vec<String>, String> {
        mark_build(&self.home, tag)
    }

    pub fn undo(&self, ids: &[String]) -> Result<(), String> {
        undo(&self.home, ids)
    }

    pub fn done(&self, ids: &[String]) {
        done(&self.home, ids);
    }
}

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
    /// The build tag it waits for, once that build began.
    build: Option<String>,
}

/// What the record's attempts are of: one kind in a file.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Install,
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u64,
    kind: Kind,
    attempts: Vec<Attempt>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            version: VERSION,
            kind: Kind::Install,
            attempts: Vec::new(),
        }
    }
}

/// Whether a build has sealed this batch; later installs and removals must wait.
pub(in crate::browser) fn has_build(home: &Path) -> Result<bool, String> {
    Ok(read(home)?.is_some_and(|r| r.attempts.iter().any(|a| a.build.is_some())))
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

/// Writes the full record before replacing the old one. An empty one retires the live file.
/// A failed persistence barrier is reported, even if the namespace change already happened.
fn write(home: &Path, record: &Record) -> Result<(), String> {
    let file = home.join(FILE);
    let persisted = if record.attempts.is_empty() {
        durable::retire(&file)
    } else {
        let text = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        durable::replace(home, &file, &text)
    };
    persisted.map_err(|e| format!("{}: {e}", file.display()))
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
    if record
        .attempts
        .iter()
        .any(|a| a.state == State::Pending || a.build.is_some())
    {
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
    // An attempt id is only ever begun once.
    if record.attempts.iter().any(|a| a.id == id) {
        return Err(unsettled());
    }
    record.attempts.push(Attempt {
        id: id.clone(),
        state: State::Pending,
        staging: relative(staged)?,
        steps,
        approvals: approvals.to_vec(),
        build: None,
    });
    write(home, &record)?;
    Ok(id)
}

/// For tests of callers: an attempt begun and left cut off.
#[cfg(test)]
pub(super) fn begin_for_test(home: &Path, staged: &Path, changes: &[PathBuf]) {
    begin(home, staged, changes, &[]).unwrap();
}

pub(in crate::browser) fn unsettled() -> String {
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

/// The profile's lock, for every change of the record: it holds across threads and across
/// launcher processes (`lock_home`).
fn locked(home: &Path) -> Result<crate::browser::HomeLockGuard, String> {
    crate::browser::lock_home(home).map_err(|e| format!("{}: {e}", home.display()))
}

/// The profiles whose legacy files this process has reconciled.
static RECONCILED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// For tests of a start: the launcher begins again knowing nothing of what it reconciled.
#[cfg(test)]
pub(super) fn restart() {
    RECONCILED.lock().unwrap().clear();
}

/// Once per profile in this process, under its lock and before the record is used: the files
/// that earlier launchers left are reconciled, and nothing recovers them on its own afterwards.
/// The install journal of a development build is undone like an attempt cut off before it was
/// published; the automatic updates' kept files are put back or dropped (`auto::recover_kept`);
/// the undo record of a development build is not understood any more and blocks the profile,
/// kept as it is.
fn reconcile_legacy(home: &Path) -> Result<(), String> {
    let key = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if RECONCILED.lock().is_ok_and(|done| done.contains(&key)) {
        return Ok(());
    }
    let kept = |file: &Path, why: String| {
        trf(
            "{file} is kept as it is and can't be used: {why}",
            &[("file", &file.display().to_string()), ("why", &why)],
        )
    };
    let undo_record = home.join("mod-browser/build-undo.json");
    if undo_record.exists() {
        return Err(kept(
            &undo_record,
            tr("a development build of the launcher made it").to_string(),
        ));
    }
    let journal = home.join("mod-browser/install-journal.json");
    if journal.exists() {
        let steps: Vec<Step> = fs::read_to_string(&journal)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            .map_err(|e| kept(&journal, e))?;
        // Its staging folder is the one its backups are in, a folder of the profile's own.
        let staging = steps
            .first()
            .and_then(|step| step.backup.parent())
            .map(Path::to_path_buf);
        if let Some(staging) = staging {
            let ours = staging.components().count() == 1
                && staging
                    .to_str()
                    .is_some_and(|name| name.starts_with(".installed-mods-"));
            if !ours {
                return Err(kept(
                    &journal,
                    tr("its backups are not where the launcher keeps them").to_string(),
                ));
            }
            let attempt = Attempt {
                id: staging.display().to_string(),
                state: State::Pending,
                staging,
                steps,
                approvals: Vec::new(),
                build: None,
            };
            if !restore(home, &attempt) {
                return Err(unsettled());
            }
        }
        fs::remove_file(&journal).map_err(|e| format!("{}: {e}", journal.display()))?;
    }
    super::auto::recover_kept(home);
    if let Ok(mut done) = RECONCILED.lock() {
        done.push(key);
    }
    Ok(())
}

/// Puts back the attempts `ids` names, newest first, each leaving the record as it is
/// restored; the others stay as they are. An attempt that a later one (not named) was made on
/// top of is not undone, and one that can't be restored keeps the record (and its staging
/// folder) and returns the error: the profile takes no new install until it is.
fn undo(home: &Path, ids: &[String]) -> Result<(), String> {
    let _lock = locked(home)?;
    reconcile_legacy(home)?;
    let mut record = read(home)?.unwrap_or_default();
    let named = |a: &Attempt| ids.contains(&a.id);
    if let Some(first) = record.attempts.iter().position(named) {
        if record.attempts[first..].iter().any(|a| !named(a)) {
            return Err(unsettled());
        }
    }
    while let Some(attempt) = record.attempts.last().filter(|a| named(a)) {
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
pub(super) fn settle(home: &Path) -> Result<(), String> {
    let _lease = Lease::acquire(home)?;
    settle_owned(home)
}

/// Repairs a partial publish; the install entry point already holds the owner lease.
pub(super) fn settle_owned(home: &Path) -> Result<(), String> {
    let _lock = locked(home)?;
    reconcile_legacy(home)?;
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
    let result = Lease::try_acquire(home).and_then(|lease| match lease {
        Some(lease) => lease.recover_abandoned().map(|()| true),
        None => Ok(false),
    });
    match result {
        Ok(recovered) => recovered,
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

/// A build for the installs that wait began: its tag is kept with each, and their ids are
/// returned for the build's end to act on ([`done`], [`undo`]) and nothing else.
fn mark_build(home: &Path, tag: &str) -> Result<Vec<String>, String> {
    let _lock = locked(home)?;
    let Some(mut record) = read(home)? else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    let mut changed = false;
    for attempt in &mut record.attempts {
        if attempt.state != State::Published
            || attempt.build.as_deref().is_some_and(|base| base != tag)
        {
            return Err(unsettled());
        }
        changed |= attempt.build.is_none();
        attempt.build.get_or_insert_with(|| tag.to_owned());
        ids.push(attempt.id.clone());
    }
    if changed {
        write(home, &record)?;
    }
    Ok(ids)
}

/// The build worked: the installs `ids` names are done, and their backups go. A later attempt
/// stays.
fn done(home: &Path, ids: &[String]) {
    let Ok(_lock) = locked(home) else {
        return;
    };
    if let Ok(Some(mut record)) = read(home) {
        record.attempts.retain(|attempt| {
            let finished = ids.contains(&attempt.id);
            if finished {
                let _ = fs::remove_dir_all(home.join(&attempt.staging));
            }
            !finished
        });
        let _ = write(home, &record);
    }
}

/// Whether installs of this profile wait for a build, or a restore is still owed.
fn outstanding(home: &Path) -> bool {
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
        done(scene.home(), &[id]);
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
        let refused = begin(
            scene.home(),
            next.path(),
            std::slice::from_ref(&scene.dep),
            &[],
        )
        .unwrap_err();
        assert!(refused.contains("still being undone"), "{refused}");
        assert!(settle(scene.home()).is_err());
        // Cleared: the restore works, and the profile takes installs again.
        fs::remove_dir_all(&scene.dep).unwrap();
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
        assert!(begin(
            scene.home(),
            next.path(),
            std::slice::from_ref(&scene.dep),
            &[]
        )
        .is_ok());
    }

    #[test]
    fn a_record_that_cannot_be_written_publishes_nothing() {
        let scene = scene();
        // The record's place is a folder: it can be neither read nor replaced.
        fs::create_dir_all(scene.home().join(FILE).join("x")).unwrap();
        let staged = staging(scene.home());
        assert!(begin(
            scene.home(),
            staged.path(),
            std::slice::from_ref(&scene.dep),
            &[]
        )
        .is_err());
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
        assert!(begin(
            scene.home(),
            staged.path(),
            std::slice::from_ref(&scene.dep),
            &[]
        )
        .is_err());
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

    /// A published native install, left waiting for its build: its id.
    fn waiting(scene: &Scene) -> String {
        let staged = staging(scene.home());
        let id = scene.begin(&staged);
        scene.publish(&staged, 3);
        commit(scene.home(), &id, staged, true).unwrap();
        id
    }

    #[test]
    fn a_build_seals_the_ordered_attempts_until_it_is_reaped() {
        let scene = scene();
        let lease = Lease::acquire(scene.home()).unwrap();
        let first = waiting(&scene);
        let build = lease.mark_build("tag").unwrap();
        assert_eq!(build, std::slice::from_ref(&first));
        let second = staging(scene.home());
        let newer = scene.home().join("sunrise-mods/newer.smod");
        let begin_second = || {
            begin(
                scene.home(),
                second.path(),
                std::slice::from_ref(&newer),
                &[],
            )
        };
        assert!(begin_second().is_err());
        assert_eq!(
            lease.mark_build("tag").unwrap(),
            build,
            "retry uses the same batch"
        );
        assert!(lease.mark_build("other").is_err());
        lease.done(&build);
        let newer_id = begin_second().unwrap();
        fs::write(&newer, "newer").unwrap();
        commit(scene.home(), &newer_id, second, true).unwrap();
        // A late callback for the first batch cannot touch the next batch.
        lease.done(&build);
        lease.undo(&build).unwrap();
        assert!(lease.outstanding() && newer.is_file());
        lease.undo(&[newer_id]).unwrap();
        assert!(!newer.exists() && !lease.outstanding());
    }

    // Invoked by the cross-process test. Wait on stdin as a build would wait on an answer.
    #[test]
    fn live_owner_process() {
        let Some(home) = std::env::var_os("SUNRISE_TEST_INSTALL_OWNER") else {
            return;
        };
        let home = Path::new(&home);
        let lease = Lease::acquire(home).unwrap();
        let staged = staging(home);
        let file = home.join("sunrise-mods/added.smod");
        let id = begin(home, staged.path(), std::slice::from_ref(&file), &[]).unwrap();
        fs::write(file, b"new mod").unwrap();
        commit(home, &id, staged, true).unwrap();
        lease.mark_build("tag").unwrap();
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        writeln!(out, "owner ready").unwrap();
        out.flush().unwrap();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).unwrap();
        assert!(answer.is_empty());
    }

    #[test]
    #[ignore = "daily"]
    fn startup_skips_another_live_owner_and_recovers_after_it_is_killed() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let scene = scene();
        let home = scene.home();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "browser::sunrise::txn::tests::live_owner_process",
                "--nocapture",
            ])
            .env("SUNRISE_TEST_INSTALL_OWNER", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let ready = BufReader::new(output)
                .lines()
                .any(|line| line.is_ok_and(|l| l == "owner ready"));
            let _ = tx.send(ready);
        });
        let ready = rx.recv_timeout(std::time::Duration::from_secs(10)) == Ok(true);
        let file = home.join("sunrise-mods/added.smod");
        let skipped = ready && !recover(home) && file.is_file() && home.join(FILE).is_file();
        let refused = ready && matches!(Lease::try_acquire(home), Ok(None));
        let killed = child.kill();
        let status = child.wait().unwrap();
        reader.join().unwrap();
        assert!(
            ready,
            "the first launcher published its install and waited for an answer"
        );
        assert!(
            skipped && refused,
            "startup did not touch the live batch or acquire its lease"
        );
        killed.unwrap();
        assert!(!status.success());
        assert!(recover(home));
        assert!(!file.exists() && !home.join(FILE).exists());
    }

    #[test]
    fn an_attempt_is_begun_once_and_a_record_of_another_kind_blocks_the_profile() {
        let scene = scene();
        let staged = staging(scene.home());
        let id = scene.begin(&staged);
        assert!(begin(
            scene.home(),
            staged.path(),
            std::slice::from_ref(&scene.dep),
            &[]
        )
        .is_err());
        abandon(scene.home(), &id).unwrap();
        // A switch's record (a later kind) or a newer layout is neither guessed at nor removed.
        let file = scene.home().join(FILE);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        for text in [
            r#"{"version":1,"kind":"switch","attempts":[]}"#,
            r#"{"version":2,"kind":"install","attempts":[]}"#,
            r#"{"version":1,"attempts":[]}"#,
        ] {
            fs::write(&file, text).unwrap();
            let staged = staging(scene.home());
            assert!(begin(
                scene.home(),
                staged.path(),
                std::slice::from_ref(&scene.dep),
                &[]
            )
            .is_err());
            assert!(!recover(scene.home()));
            assert!(outstanding(scene.home()));
            assert!(mark_build(scene.home(), "tag").is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
        }
    }

    #[test]
    fn the_files_earlier_builds_left_are_reconciled_once_under_the_same_gate() {
        let scene = scene();
        // An install journal of a development build, cut off after replacing the dependency.
        let staged = staging(scene.home());
        let name = staged.path().file_name().unwrap().to_owned();
        let _ = staged.keep();
        let backup = Path::new(&name).join("old-0");
        fs::rename(&scene.dep, scene.home().join(&backup)).unwrap();
        fs::write(&scene.dep, "flywheel 0.4").unwrap();
        let journal = scene.home().join("mod-browser/install-journal.json");
        fs::create_dir_all(journal.parent().unwrap()).unwrap();
        let steps = serde_json::json!([{ "to": "sunrise-mods/flywheel.smod", "existed": true, "backup": backup }]);
        fs::write(&journal, steps.to_string()).unwrap();
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
        assert!(!journal.exists() && !scene.home().join(&name).exists());
        // A journal that points outside the launcher's staging folders is kept and blocks.
        let other = tempfile::tempdir().unwrap();
        let blocked = other.path();
        fs::create_dir_all(blocked.join("mod-browser")).unwrap();
        let text = r#"[{"to":"a","existed":true,"backup":"somewhere/old-0"}]"#;
        fs::write(blocked.join("mod-browser/install-journal.json"), text).unwrap();
        assert!(settle(blocked).is_err());
        assert_eq!(
            fs::read_to_string(blocked.join("mod-browser/install-journal.json")).unwrap(),
            text
        );
        // An undo record of a development build is not understood: kept, blocking.
        let third = tempfile::tempdir().unwrap();
        fs::create_dir_all(third.path().join("mod-browser")).unwrap();
        fs::write(third.path().join("mod-browser/build-undo.json"), "{}").unwrap();
        assert!(settle(third.path()).is_err());
        assert!(third.path().join("mod-browser/build-undo.json").is_file());
    }
}
