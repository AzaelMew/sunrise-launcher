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
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod durable;

const FILE: &str = "mod-browser/install-txn.json";
const VERSION: u64 = 1;

/// The live owner of one profile's install batch. The file is never removed: all launchers
/// must contend on the same inode, even when the record itself is replaced or absent.
pub struct Lease {
    home: PathBuf,
    file: fs::File,
    #[cfg(unix)]
    transferred: std::sync::atomic::AtomicBool,
}

#[cfg(unix)]
impl Drop for Lease {
    fn drop(&mut self) {
        // A concurrent fork can briefly inherit the handle before exec closes it. On normal
        // release unlock explicitly, rather than leaving it to the last inherited handle.
        if !*self.transferred.get_mut() {
            let _ = self.file.unlock();
        }
    }
}

impl Lease {
    pub fn try_acquire(home: &Path) -> Result<Option<Self>, String> {
        fs::create_dir_all(home).map_err(|e| format!("{}: {e}", home.display()))?;
        let path = home.join(".install-txn.lock");
        let fail = |e| format!("{}: {e}", path.display());
        let mut open = fs::File::options();
        open.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Sharing exclusion is owned by the kernel file object, including inherited
            // duplicates. A byte-range lock would die with the opening process.
            open.share_mode(0);
        }
        let file = match open.open(&path) {
            Ok(file) => file,
            #[cfg(windows)]
            Err(e) if matches!(e.raw_os_error(), Some(32 | 33)) => return Ok(None),
            Err(e) => return Err(fail(e)),
        };
        #[cfg(unix)]
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => return Err(fail(e)),
        }
        Ok(Some(Self {
            home: home.to_path_buf(),
            file,
            #[cfg(unix)]
            transferred: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    #[cfg(unix)]
    pub(crate) fn inherit(&self, command: &mut std::process::Command) -> io::Result<()> {
        crate::process::leases::inherit_profile(command, &self.file)?;
        self.transferred
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) fn file(&self) -> &fs::File {
        &self.file
    }

    pub fn acquire(home: &Path) -> Result<Self, String> {
        Self::try_acquire(home)?.ok_or_else(|| {
            tr("Another launcher is working on this profile. Try again when it finishes.")
                .to_string()
        })
    }

    /// Admit a new operation: claim the profile, then settle any transaction whose last owner
    /// has gone. Returning the lease couples the admission result to the operation's lifetime.
    pub fn admit(home: &Path) -> Result<Self, String> {
        let lease = Self::acquire(home)?;
        lease.recover_abandoned()?;
        Ok(lease)
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// A freshly acquired lease can clean up an abandoned batch before doing new work.
    pub fn recover_abandoned(&self) -> Result<(), String> {
        let _lock = locked(&self.home)?;
        let mut record = read(&self.home)?.unwrap_or_default();
        reconcile_legacy(&self.home)?;
        while !record.attempts.is_empty() {
            undo_last(&self.home, &mut record)?;
        }
        Ok(())
    }

    pub(in crate::browser) fn finish_standalone(&self) -> Result<(), String> {
        let _lock = locked(&self.home)?;
        let ids: Vec<_> = read(&self.home)?
            .unwrap_or_default()
            .attempts
            .into_iter()
            .map(|attempt| attempt.id)
            .collect();
        self.done(&ids)
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

    pub fn done(&self, ids: &[String]) -> Result<(), String> {
        done(&self.home, ids, write)
    }
}

/// One file the install puts in place, relative to the profile.
#[derive(Clone, Serialize, Deserialize)]
struct Step {
    to: PathBuf,
    before: Image,
    after: Image,
    backup: PathBuf,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Image {
    Missing,
    Sha256([u8; 32]),
}

impl Image {
    fn at(path: &Path) -> Result<Self, String> {
        let inspect = || -> io::Result<Self> {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => return Err(io::Error::from(io::ErrorKind::InvalidData)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::Missing),
                Err(e) => return Err(e),
            }
            let mut hash = Sha256::new();
            io::copy(&mut fs::File::open(path)?, &mut hash)?;
            Ok(Self::Sha256(hash.finalize().into()))
        };
        inspect().map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[derive(Deserialize)]
struct LegacyStep {
    to: PathBuf,
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
    /// Reverse steps still owed. The current step is `undo - 1`, until its barrier and the
    /// record update both finish. The source phase distinguishes a half-published step.
    undo: Option<usize>,
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
    let record: Record = serde_json::from_value(value).map_err(|e| kept(e.to_string()))?;
    use std::path::Component;
    let inside = |path: &Path| {
        !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
    };
    if record.attempts.iter().enumerate().any(|(i, a)| {
        !inside(&a.staging)
            || a.staging.components().count() != 1
            || !a.id.starts_with(".installed-mods-")
            || a.staging.as_os_str() != a.id.as_str()
            || a.undo
                .is_some_and(|next| next > a.steps.len() || i + 1 != record.attempts.len())
            || a.steps.iter().enumerate().any(|(n, step)| {
                !inside(&step.to) || step.backup != a.staging.join(format!("old-{n}"))
            })
    }) {
        return Err(kept(unsettled()));
    }
    Ok(Some(record))
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
    changes: &[(Option<PathBuf>, PathBuf)],
    approvals: &[Approval],
) -> Result<String, String> {
    let mut record = read(home)?.unwrap_or_default();
    if record
        .attempts
        .iter()
        .any(|a| a.state == State::Pending || a.build.is_some() || a.undo.is_some())
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
        .map(|(i, (from, to))| {
            let after = match from {
                Some(from) => match Image::at(from)? {
                    Image::Missing => {
                        return Err(format!("{}: {}", from.display(), io::ErrorKind::NotFound))
                    }
                    image => image,
                },
                None => Image::Missing,
            };
            Ok(Step {
                before: Image::at(to)?,
                after,
                to: relative(to)?,
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
        undo: None,
    });
    write(home, &record)?;
    Ok(id)
}

pub(in crate::browser) fn unsettled() -> String {
    tr("An earlier install is still being undone. Restart the launcher and try again.").to_string()
}

/// Restore a single recorded step. A resumed frontier accepts exact restored bytes (including
/// recorded absence), not the absence of a backup. Only Pending may also be between the two
/// publication renames: no target yet, but an exact backup.
fn restore_step(home: &Path, attempt: &Attempt, index: usize) -> Result<(), String> {
    let step = attempt.steps.get(index).ok_or_else(unsettled)?;
    let to = home.join(&step.to);
    let image = Image::at(&to)?;
    let backup = home.join(&step.backup);
    if image == step.before {
        return durable::sync_restored(&to, &backup, image == Image::Missing)
            .map_err(|e| format!("{}: {e}", to.display()));
    }
    let diverged = || {
        trf(
            "{file} doesn't match either version of the interrupted install. Nothing further was undone.",
            &[("file", &to.display().to_string())],
        )
    };
    if image != step.after && !(attempt.state == State::Pending && image == Image::Missing) {
        return Err(diverged());
    }
    if step.before != Image::Missing && Image::at(&backup)? != step.before {
        return Err(diverged());
    }
    let from = if step.before == Image::Missing {
        // Remove the new file by renaming it into our staging folder. If interrupted after
        // the rename, the recorded absent preimage proves the restored state at this frontier.
        to.as_path()
    } else {
        backup.as_path()
    };
    let discarded = home
        .join(&attempt.staging)
        .join(format!("discarded-{index}"));
    let dest = if step.before == Image::Missing {
        &discarded
    } else {
        &to
    };
    durable::restore_move(from, dest).map_err(|e| format!("{}: {e}", to.display()))
}

/// The newest attempt is the only one allowed to advance the frontier. Retire its record
/// before discarding its staging folder; an older attempt can then restore the same path again.
fn undo_last(home: &Path, record: &mut Record) -> Result<(), String> {
    let last = record.attempts.last_mut().expect("undo has an attempt");
    if last.undo.is_none() {
        last.undo = Some(last.steps.len());
        write(home, record)?;
    }
    while let Some(last) = record.attempts.last().filter(|a| a.undo != Some(0)) {
        let next = last
            .undo
            .ok_or_else(unsettled)?
            .checked_sub(1)
            .ok_or_else(unsettled)?;
        restore_step(home, last, next)?;
        record.attempts.last_mut().expect("same attempt").undo = Some(next);
        write(home, record)?;
    }
    let staging = home.join(record.attempts.pop().expect("undo has an attempt").staging);
    write(home, record)?;
    let _ = fs::remove_dir_all(staging);
    Ok(())
}

/// A publish failed partway through. Keep its staging folder even if rollback itself is
/// interrupted, and resume from the persisted frontier just like startup recovery.
pub(super) fn rollback(
    home: &Path,
    id: &str,
    staged: tempfile::TempDir,
    message: String,
) -> String {
    let _ = staged.keep();
    match undo(home, &[id.to_owned()]) {
        Ok(()) => message,
        Err(error) => format!(
            "{message}. {} ({error})",
            tr("Putting the files back failed too: restart the launcher to try again.")
        ),
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
        let mut record = record.ok_or_else(unsettled)?;
        let a = record
            .attempts
            .last_mut()
            .filter(|a| a.id == id && a.undo.is_none())
            .ok_or_else(unsettled)?;
        a.state = State::Published;
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
        let steps: Vec<LegacyStep> = fs::read_to_string(&journal)
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
            // Legacy records predate images and the frontier. Do not use their weaker
            // inference for a current transaction.
            for step in steps.iter().rev() {
                let (to, backup) = (home.join(&step.to), home.join(&step.backup));
                match (step.existed, backup.is_file()) {
                    (true, true) => fs::rename(backup, &to),
                    (false, _) if to.is_file() => fs::remove_file(&to),
                    _ => Ok(()),
                }
                .map_err(|e| kept(&journal, e.to_string()))?;
            }
            let _ = fs::remove_dir_all(home.join(staging));
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
    let mut record = read(home)?.unwrap_or_default();
    reconcile_legacy(home)?;
    let named = |a: &Attempt| ids.contains(&a.id);
    if let Some(first) = record.attempts.iter().position(named) {
        if record.attempts[first..].iter().any(|a| !named(a)) {
            return Err(unsettled());
        }
    }
    while record.attempts.last().is_some_and(named) {
        undo_last(home, &mut record)?;
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
    let mut record = read(home)?.unwrap_or_default();
    reconcile_legacy(home)?;
    while record
        .attempts
        .last()
        .is_some_and(|a| a.state == State::Pending || a.undo.is_some())
    {
        undo_last(home, &mut record)?;
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
            || attempt.undo.is_some()
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

/// The build worked: the installs `ids` names leave the record before any backups are deleted.
/// If persistence fails, retain every backup; a later build/recovery may still need it.
fn done(
    home: &Path,
    ids: &[String],
    persist: impl FnOnce(&Path, &Record) -> Result<(), String>,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let _lock = locked(home)?;
    let Some(mut record) = read(home)? else {
        return Ok(());
    };
    let mut finished = Vec::new();
    record.attempts.retain(|attempt| {
        if ids.contains(&attempt.id) {
            finished.push(home.join(&attempt.staging));
            false
        } else {
            true
        }
    });
    if !finished.is_empty() {
        persist(home, &record)?;
        for staging in finished {
            let _ = fs::remove_dir_all(staging);
        }
    }
    Ok(())
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
            let changes = [
                (&self.dep, "flywheel 0.4"),
                (&self.list, "new list"),
                (&self.added, "lumen"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (target, bytes))| {
                let source = staged.path().join(format!("new-{i}"));
                fs::write(&source, bytes).unwrap();
                (Some(source), target.clone())
            })
            .collect::<Vec<_>>();
            begin(self.home(), staged.path(), &changes, &[]).unwrap()
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
        done(scene.home(), &[id], write).unwrap();
        assert!(!kept.exists() && !scene.home().join(FILE).exists());
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
    }

    #[test]
    fn failed_build_commit_keeps_the_record_and_undo_files() {
        let scene = scene();
        let lease = Lease::acquire(scene.home()).unwrap();
        let id = waiting(&scene);
        let ids = lease.mark_build("tag").unwrap();
        let record = scene.home().join(FILE);
        let before = fs::read(&record).unwrap();
        let backup = scene.home().join(&id).join("old-0");
        let fail = |_: &Path, _: &Record| Err("cannot persist the commit".into());
        assert_eq!(
            done(scene.home(), &ids, fail),
            Err("cannot persist the commit".into())
        );
        assert_eq!(fs::read(&record).unwrap(), before);
        assert_eq!(fs::read_to_string(&backup).unwrap(), "flywheel 0.3");
        drop(lease);
        assert!(recover(scene.home()));
        scene.is_as_it_was();
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
        let next = second.path().join("new-list");
        fs::write(&next, "newer list").unwrap();
        let new_plain = second.path().join("new-plain");
        fs::write(&new_plain, "plain").unwrap();
        let id = begin(
            scene.home(),
            second.path(),
            &[
                (Some(next), scene.list.clone()),
                (Some(new_plain), plain.clone()),
            ],
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
        let refused =
            begin(scene.home(), next.path(), &[(None, scene.dep.clone())], &[]).unwrap_err();
        assert!(refused.contains("still being undone"), "{refused}");
        assert!(settle(scene.home()).is_err());
        // Cleared: the restore works, and the profile takes installs again.
        fs::remove_dir_all(&scene.dep).unwrap();
        assert!(recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
        assert!(begin(scene.home(), next.path(), &[(None, scene.dep.clone())], &[]).is_ok());
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
            &[(None, scene.dep.clone())],
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
            &[(None, scene.dep.clone())],
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
        let source = second.path().join("new");
        fs::write(&source, "newer").unwrap();
        let begin_second = || {
            begin(
                scene.home(),
                second.path(),
                &[(Some(source.clone()), newer.clone())],
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
        lease.done(&build).unwrap();
        let newer_id = begin_second().unwrap();
        fs::write(&newer, "newer").unwrap();
        commit(scene.home(), &newer_id, second, true).unwrap();
        // A late callback for the first batch cannot touch the next batch.
        lease.done(&build).unwrap();
        lease.undo(&build).unwrap();
        assert!(lease.outstanding() && newer.is_file());
        lease.undo(&[newer_id]).unwrap();
        assert!(!newer.exists() && !lease.outstanding());
    }

    fn wait_to_be_killed() {
        use std::io::Write;
        let mut out = io::stdout().lock();
        writeln!(out, "owner ready").unwrap();
        out.flush().unwrap();
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).unwrap();
        assert!(answer.is_empty());
    }

    /// Drives a child up to a durable boundary, then kills it without Rust destructors.
    fn crash_process<T>(
        test: &str,
        home: &Path,
        worker: Option<&str>,
        inspect: impl FnOnce() -> T,
    ) -> T {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let mut command = Command::new(std::env::current_exe().unwrap());
        if let Some(worker) = worker {
            command.env("SUNRISE_TEST_LIVE_PROFILE_WORKER", worker);
        }
        let mut child = command
            .args(["--exact", test, "--nocapture"])
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
        let result = ready.then(inspect);
        let killed = child.kill();
        let status = child.wait().unwrap();
        reader.join().unwrap();
        assert!(ready, "the child reached its crash boundary");
        killed.unwrap();
        assert!(!status.success());
        result.unwrap()
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
        let source = staged.path().join("new");
        fs::write(&source, b"new mod").unwrap();
        let id = begin(home, staged.path(), &[(Some(source), file.clone())], &[]).unwrap();
        fs::write(file, b"new mod").unwrap();
        commit(home, &id, staged, true).unwrap();
        lease.mark_build("tag").unwrap();
        let worker = std::env::var_os("SUNRISE_TEST_LIVE_PROFILE_WORKER");
        let mut sdk = None;
        let mut game = None;
        if let Some(worker) = worker {
            use std::process::{Command, Stdio};
            let mut cmd = Command::new(std::env::current_exe().unwrap());
            cmd.args([
                "--exact",
                "browser::sunrise::txn::tests::live_profile_worker",
                "--nocapture",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
            if worker == "sdk" {
                sdk = Some(crate::process::owned::Owned::spawn_profile(&mut cmd, &lease).unwrap());
            } else {
                game = Some(crate::run_leases::spawn(&mut cmd, true, Some(&lease)).unwrap());
            }
            let ready = home.join("worker-ready");
            let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !ready.is_file() && std::time::Instant::now() < limit {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(ready.is_file());
        }
        wait_to_be_killed();
        drop((sdk, game));
    }

    #[test]
    #[ignore = "daily"]
    fn startup_skips_another_live_owner_and_recovers_after_it_is_killed() {
        let scene = scene();
        let home = scene.home();
        let file = home.join("sunrise-mods/added.smod");
        let (skipped, refused) = crash_process(
            "browser::sunrise::txn::tests::live_owner_process",
            home,
            None,
            || {
                (
                    !recover(home) && file.is_file() && home.join(FILE).is_file(),
                    matches!(Lease::try_acquire(home), Ok(None)),
                )
            },
        );
        assert!(
            skipped && refused,
            "startup did not touch the live batch or acquire its lease"
        );
        assert!(recover(home));
        assert!(!file.exists() && !home.join(FILE).exists());
    }

    #[test]
    fn live_profile_worker() {
        let Some(home) = std::env::var_os("SUNRISE_TEST_INSTALL_OWNER") else {
            return;
        };
        let home = Path::new(&home);
        fs::write(home.join("worker-ready"), b"").unwrap();
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !home.join("stop-worker").is_file() && std::time::Instant::now() < limit {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_dead_launcher_is_not_abandoned_while_its_profile_child_lives() {
        for worker in ["sdk", "game"] {
            let scene = scene();
            let home = scene.home();
            let file = home.join("sunrise-mods/added.smod");
            crash_process(
                "browser::sunrise::txn::tests::live_owner_process",
                home,
                Some(worker),
                || (),
            );
            let blocked = !recover(home) && file.is_file() && home.join(FILE).is_file();
            // End the child even if the preceding check failed.
            fs::write(home.join("stop-worker"), b"").unwrap();
            let limit = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let recovered = loop {
                if recover(home) {
                    break true;
                }
                if std::time::Instant::now() >= limit {
                    break false;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert!(
                blocked,
                "the surviving {worker} pinned the transaction after launcher death"
            );
            assert!(
                recovered && !file.exists() && !home.join(FILE).exists(),
                "recovered after the {worker} exited"
            );
        }
    }

    // Kill recovery after carrying out a reverse move, before writing its next frontier.
    #[test]
    fn live_undo_process() {
        let Some(home) = std::env::var_os("SUNRISE_TEST_INSTALL_OWNER") else {
            return;
        };
        let home = Path::new(&home);
        let _lease = Lease::acquire(home).unwrap();
        let mut record = read(home).unwrap().unwrap();
        undo_last(home, &mut record).unwrap(); // C -> B, and retire this attempt.
        record.attempts.last_mut().unwrap().undo = Some(2);
        write(home, &record).unwrap();
        restore_step(home, record.attempts.last().unwrap(), 1).unwrap(); // B -> A
        wait_to_be_killed();
    }

    #[test]
    #[ignore = "daily"]
    fn interrupted_stacked_undo_resumes_at_the_exact_restored_image() {
        let scene = scene();
        let home = scene.home();
        let first = staging(home);
        let list = first.path().join("list-b");
        let dep = first.path().join("dep-b");
        fs::write(&list, "list B").unwrap();
        fs::write(&dep, "B").unwrap();
        let id = begin(
            home,
            first.path(),
            &[
                (Some(list.clone()), scene.list.clone()),
                (Some(dep.clone()), scene.dep.clone()),
            ],
            &[],
        )
        .unwrap();
        fs::rename(&scene.list, first.path().join("old-0")).unwrap();
        fs::rename(&scene.dep, first.path().join("old-1")).unwrap();
        fs::rename(list, &scene.list).unwrap();
        fs::rename(dep, &scene.dep).unwrap();
        commit(home, &id, first, true).unwrap();
        let second = staging(home);
        let newer = second.path().join("dep-c");
        fs::write(&newer, "C").unwrap();
        let next = begin(
            home,
            second.path(),
            &[(Some(newer.clone()), scene.dep.clone())],
            &[],
        )
        .unwrap();
        fs::rename(&scene.dep, second.path().join("old-0")).unwrap();
        fs::rename(newer, &scene.dep).unwrap();
        commit(home, &next, second, true).unwrap();
        crash_process(
            "browser::sunrise::txn::tests::live_undo_process",
            home,
            None,
            || (),
        );
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.3");
        assert_eq!(fs::read_to_string(&scene.list).unwrap(), "list B");
        assert_eq!(read(home).unwrap().unwrap().attempts[0].undo, Some(2));
        assert!(recover(home));
        scene.is_as_it_was();
        assert!(!home.join(FILE).exists());
    }

    #[test]
    fn a_new_intent_settles_an_abandoned_profile_without_waiting_for_startup() {
        let scene = scene();
        let current = Lease::admit(scene.home()).unwrap();
        let id = waiting(&scene);
        assert_eq!(current.mark_build("tag").unwrap(), [id]);
        // A second launcher was already running. Admission cannot start Play on these bytes.
        assert!(Lease::admit(scene.home()).is_err());
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
        drop(current);
        let next = Lease::admit(scene.home()).unwrap();
        scene.is_as_it_was();
        assert!(!next.outstanding());
        assert!(
            Lease::admit(scene.home()).is_err(),
            "the next operation kept the gate"
        );
        drop(next);
        fs::write(scene.home().join(FILE), "not a transaction").unwrap();
        assert!(Lease::admit(scene.home()).is_err());
        assert_eq!(
            fs::read_to_string(scene.home().join(FILE)).unwrap(),
            "not a transaction"
        );
    }

    #[test]
    fn a_missing_backup_is_not_evidence_of_a_restored_output() {
        let scene = scene();
        let id = waiting(&scene);
        fs::remove_file(scene.home().join(&id).join("old-0")).unwrap();
        assert!(!recover(scene.home()));
        assert_eq!(fs::read_to_string(&scene.dep).unwrap(), "flywheel 0.4");
        let record = read(scene.home()).unwrap().unwrap();
        assert_eq!(record.attempts[0].undo, Some(1));
    }

    #[test]
    fn an_attempt_is_begun_once_and_a_record_of_another_kind_blocks_the_profile() {
        let scene = scene();
        let staged = staging(scene.home());
        let id = scene.begin(&staged);
        assert!(begin(
            scene.home(),
            staged.path(),
            &[(None, scene.dep.clone())],
            &[]
        )
        .is_err());
        undo(scene.home(), &[id]).unwrap();
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
                &[(None, scene.dep.clone())],
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
