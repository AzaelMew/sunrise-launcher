//! Automatic updates of what the browser installed from Sunrise's registry (Settings: "Update
//! registry mods automatically", on by default): at the launcher's start and before Play, each
//! installed registry project moves to its newest version for this game ([`Index::installable`]),
//! checked as an install is. What it never touches:
//!
//! - a game folder inside a modpack's profile: the pack pins its mods by SHA-256 and updates as a
//!   whole, when the player asks;
//! - code mods, which update only when the player asks;
//! - a data pack: it lives in a world, not in the browser's list;
//! - a version for another Minecraft than the one installed, and anything older than what is installed.
//!
//! A content mod (`.smod`) whose new version drops a block, item, entity, effect, menu or
//! creative tab the installed one has waits for the player (its blocks would vanish from their
//! worlds): the notice says which. The file an update replaces is kept in
//! `mod-browser/replaced/` until the next start. A leftover ledger is only advisory: never
//! restore over, or delete, live packages based on that ledger.
use super::{install_version_guarded, Index};
use crate::browser::{installed, Kind, SourceId};
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use sunrise_launcher_smod::index;

/// Where replaced files wait for the next start, and their list.
const KEPT_DIR: &str = "mod-browser/replaced";
const KEPT: &str = "mod-browser/replaced.json";

/// What happens to one installed project.
#[derive(Debug, PartialEq)]
pub enum Decision<'a> {
    Update(&'a index::Version),
    /// Not updated automatically, or nothing newer.
    Skip,
}

/// Whether the game folder belongs to a modpack, and the Minecraft version Sunrise plays.
pub struct Context<'a> {
    pub in_pack: bool,
    pub game: &'a str,
}

/// What an automatic update did: `Title version` updated, what waits for the player and why,
/// and what failed.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub updated: Vec<String>,
    pub held: Vec<String>,
    pub failed: Vec<String>,
}

impl Report {
    /// The lines the launcher shows: nothing when nothing happened.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.updated.is_empty() {
            out.push(trf(
                "Updated automatically: {list}.",
                &[("list", &self.updated.join(", "))],
            ));
        }
        out.extend(self.held.iter().cloned());
        out.extend(self.failed.iter().cloned());
        out
    }
}

/// Whether game folder `home` is a registry modpack's profile (`pack::home`).
pub fn in_pack(home: &Path) -> bool {
    home.ends_with("home") && home.parent().is_some_and(super::pack::is_pack)
}

/// Whether `entry` updates automatically, and to what.
pub fn decide<'a>(index: &'a Index, entry: &installed::Entry, ctx: &Context) -> Decision<'a> {
    let skip_native = crate::native_status::is_native_file(&entry.file);
    if entry.source != SourceId::Sunrise
        || ctx.in_pack
        || skip_native
        || entry.kind == Kind::DataPack
    {
        return Decision::Skip;
    }
    let Some(p) = index.project(&entry.project) else {
        return Decision::Skip;
    };
    let Some(target) = index.installable(p, ctx.game) else {
        return Decision::Skip;
    };
    // Versions are newest first: only forward, from a version the index still lists.
    let at = |version: &str| p.versions.iter().position(|v| v.version == version);
    let (Some(now), Some(next)) = (at(&entry.version), at(&target.version)) else {
        return Decision::Skip;
    };
    let same_game = p.versions[now].game == target.game;
    let native_next = p.kind == index::Kind::NativeMod;
    if next >= now || !same_game || native_next {
        return Decision::Skip;
    }
    Decision::Update(target)
}

/// The kinds a content mod declares one file each for (`data/<namespace>/sunrise/<kind>/<id>.json`), as notices name them.
const DECLARED: [(&str, &str); 6] = [
    ("block", "block"),
    ("item", "item"),
    ("entity_type", "entity"),
    ("mob_effect", "effect"),
    ("menu", "menu"),
    ("creative_tab", "creative tab"),
];

/// The blocks, items, entities, effects, menus and creative tabs a content mod declares, read from its files' paths: the game checks their contents when it loads the mod.
pub fn content_ids(path: &Path) -> Result<BTreeSet<String>, String> {
    let manifest = sunrise_launcher_smod::Package::read(path)?.manifest;
    let names = if path.is_dir() {
        sunrise_launcher_smod::files::content_files(path)?
    } else {
        let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        zip::ZipArchive::new(file)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .file_names()
            .map(|name| {
                name.map(|name| name.into_owned())
                    .map_err(|e| format!("{}: {e}", path.display()))
            })
            .collect::<Result<_, _>>()?
    };
    let owned: BTreeSet<&str> = manifest.owned_namespaces().collect();
    Ok(names
        .iter()
        .filter_map(|name| {
            let rest = name.strip_prefix("data/")?;
            let (namespace, rest) = rest.split_once('/')?;
            let rest = rest.strip_prefix("sunrise/")?;
            let (kind, file) = rest.split_once('/')?;
            let id = file.strip_suffix(".json")?;
            let label = DECLARED.iter().find(|(dir, _)| *dir == kind)?.1;
            owned
                .contains(namespace)
                .then(|| format!("{label} {namespace}:{id}"))
        })
        .collect())
}

/// What the content mod at `old` has that the one at `new` lacks.
pub fn dropped(old: &Path, new: &Path) -> Result<Vec<String>, String> {
    let new = content_ids(new)?;
    Ok(content_ids(old)?.difference(&new).cloned().collect())
}

/// A replaced file kept until the next start, with the entry it had.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Kept {
    entry: installed::Entry,
    /// Its copy, relative to the game folder.
    copy: String,
    pid: u32,
    /// Current-process entries are not retired while this updater is still using them. A PID
    /// alone isn't evidence: a legacy row may name a PID the OS has since reused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<[u8; 16]>,
}

static SESSION: OnceLock<[u8; 16]> = OnceLock::new();

fn kept_list(home: &Path) -> Result<Vec<Kept>, String> {
    let path = home.join(KEPT);
    let fail = |error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let root = path.parent().expect("browser folder");
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() && regular(&meta) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(fail(&e)),
        Ok(_) => return Err(fail(&io::ErrorKind::InvalidData)),
    }
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && regular(&meta) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(fail(&e)),
        Ok(_) => return Err(fail(&io::ErrorKind::InvalidData)),
    }
    let bytes = fs::read(&path).map_err(|e| fail(&e))?;
    let list: Vec<Kept> = serde_json::from_slice(&bytes).map_err(|e| fail(&e))?;
    let mut seen = BTreeSet::new();
    if list.iter().any(|row| {
        let first = format!("{KEPT_DIR}/{}", row.entry.project);
        let fresh = row
            .copy
            .strip_prefix(&first)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|random| {
                random.len() == 24 && random.bytes().all(|b| b.is_ascii_alphanumeric())
            });
        row.entry.source != SourceId::Sunrise
            || !index::valid_id(&row.entry.project)
            || (row.copy != first && !fresh)
            || !seen.insert(row.entry.project.as_str())
    }) {
        return Err(fail(&io::ErrorKind::InvalidData));
    }
    Ok(list)
}

fn regular(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

fn exact_package(home: &Path, path: &Path, checksum: &str) -> bool {
    let Some(hash) = checksum.strip_prefix("SHA-256:") else {
        return false;
    };
    let mut parent = path.parent();
    while let Some(dir) = parent.filter(|dir| *dir != home) {
        if !fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir() && regular(&meta)) {
            return false;
        }
        parent = dir.parent();
    }
    parent == Some(home)
        && fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && regular(&meta))
        && index::sha256_file(path).is_ok_and(|actual| actual.eq_ignore_ascii_case(hash))
}

fn save_kept(home: &Path, list: &[Kept]) -> Result<(), String> {
    let path = home.join(KEPT);
    if list.is_empty() {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        };
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = home.join(format!(
        "mod-browser/.replaced.{}-{id}.part",
        std::process::id()
    ));
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let res = fs::write(&part, text).and_then(|()| fs::rename(&part, &path));
    if let Err(e) = res {
        let _ = fs::remove_file(&part);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

/// Whether the browser's list has `kept`'s project at a newer version whose file is in place.
fn finished(home: &Path, kept: &Kept) -> bool {
    let list = installed::Installed::load(home);
    let Some(now) = list.find(SourceId::Sunrise, &kept.entry.project) else {
        return false;
    };
    let hash = now.checksum.strip_prefix("SHA-256:").unwrap_or_default();
    now.version != kept.entry.version
        && now
            .path(home)
            .and_then(|p| index::sha256_file(&p).ok())
            .is_some_and(|h| h.eq_ignore_ascii_case(hash))
}

/// Puts `kept` back: its file where it was and its entry in the browser's list.
fn restore(home: &Path, kept: &Kept) -> Result<(), String> {
    let (Some(target), copy) = (kept.entry.path(home), home.join(&kept.copy)) else {
        return Err(format!("{} is outside the game folder", kept.entry.file));
    };
    if !exact_package(home, &copy, &kept.entry.checksum) {
        return Err(tr("The saved mod copy couldn't be checked.").into());
    }
    fs::copy(&copy, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut list = installed::Installed::load(home);
    let ours =
        |e: &installed::Entry| e.source == SourceId::Sunrise && e.project == kept.entry.project;
    if let Some(now) = list.entries.iter().find(|e| ours(e)) {
        if now.file != kept.entry.file {
            if let Some(path) = now.path(home) {
                let _ = fs::remove_file(path);
            }
        }
    }
    list.entries.retain(|e| !ours(e));
    list.entries.push(kept.entry.clone());
    list.save(home)
}

fn lock_for_updates(home: &Path) -> Result<crate::browser::HomeLockGuard, String> {
    crate::browser::lock_home(home).map_err(|error| {
        eprintln!(
            "Sunrise launcher: could not lock registry updates for {}: {error}",
            home.display()
        );
        tr("The launcher couldn't check for unfinished mod updates. You can still play.").into()
    })
}

/// Retire only advisory entries proved redundant. The legacy updater still owns the live
/// ledger name here, so unreadable or ambiguous evidence stays in place and refuses *updates*,
/// not Play. Can't move just the ledger away: the updater could overwrite a retained copy.
pub fn recover(home: &Path) -> Result<(), String> {
    let _lock = lock_for_updates(home)?;
    installed::recover_game_install(home)
        .map_err(|_| installed::GameInstallProblem::summary().to_owned())?;
    let notice = || {
        tr("A backup from an older mod update was kept because it couldn't be checked. You can still play.").to_owned()
    };
    let mut kept = kept_list(home).map_err(|_| notice())?;
    if kept.is_empty() {
        return Ok(());
    }
    // The usual display reader drops unknown or malformed claims. No dropped claim may
    // be treated as proof that another row alone owns these bytes.
    #[derive(Deserialize)]
    struct Claims {
        entries: Vec<installed::Entry>,
    }
    let manifest = home.join(installed::MANIFEST);
    if !fs::symlink_metadata(&manifest).is_ok_and(|meta| meta.is_file() && regular(&meta)) {
        return Err(notice());
    }
    let bytes = fs::read(&manifest).map_err(|_| notice())?;
    let claims: Claims = serde_json::from_slice(&bytes).map_err(|_| notice())?;
    if claims
        .entries
        .iter()
        .any(|claim| claim.path(home).is_none())
    {
        return Err(notice());
    }
    // A session token is not evidence that the last attempt finished or rolled back.
    for old in &kept {
        let mut rows = claims
            .entries
            .iter()
            .filter(|row| row.source == SourceId::Sunrise && row.project == old.entry.project);
        let current = rows.next().filter(|_| rows.next().is_none());
        if !current.is_some_and(|row| {
            row.path(home)
                .is_some_and(|path| exact_package(home, &path, &row.checksum))
        }) || !exact_package(home, &home.join(&old.copy), &old.entry.checksum)
        {
            return Err(notice());
        }
    }
    let before = kept.len();
    kept.retain(|row| {
        row.session
            .as_ref()
            .is_some_and(|id| Some(id) == SESSION.get())
    });
    if kept.len() != before {
        save_kept(home, &kept).map_err(|_| notice())?;
    }
    // No longer referenced. Retain even proved copies in this standalone upgrade step;
    // the old format doesn't bind directory identity or provide a retirement barrier.
    Ok(())
}

/// Keeps the file `entry` installed before it is replaced.
fn keep(home: &Path, entry: &installed::Entry) -> Result<Kept, String> {
    let from = entry
        .path(home)
        .ok_or_else(|| format!("{} is outside the game folder", entry.file))?;
    if !index::valid_id(&entry.project) {
        return Err(format!("invalid registry project id {:?}", entry.project));
    }
    let mut list = kept_list(home)?;
    let dir = home.join(KEPT_DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir() && regular(&meta)) {
        return Err(tr("The mod backup folder couldn't be checked.").into());
    }
    let mut fresh = tempfile::Builder::new()
        .prefix(&format!("{}.", entry.project))
        .rand_bytes(24)
        .tempfile_in(&dir)
        .map_err(|e| e.to_string())?;
    let mut original = fs::File::open(&from).map_err(|e| format!("{}: {e}", from.display()))?;
    io::copy(&mut original, fresh.as_file_mut())
        .and_then(|_| fresh.as_file().sync_all())
        .map_err(|e| format!("{}: {e}", fresh.path().display()))?;
    if !exact_package(home, fresh.path(), &entry.checksum) {
        return Err(tr("The saved mod copy couldn't be checked.").into());
    }
    let file = fresh
        .path()
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("generated ASCII filename");
    let copy = format!("{KEPT_DIR}/{file}");
    let mut session = [0; 16];
    if let Some(id) = SESSION.get() {
        session = *id;
    } else {
        getrandom::fill(&mut session).map_err(|e| e.to_string())?;
        session = *SESSION.get_or_init(|| session);
    }
    let kept = Kept {
        entry: entry.clone(),
        copy,
        pid: std::process::id(),
        session: Some(session),
    };
    // Keep the exact allocated path even if saving returns after a partially visible transition.
    fresh.keep().map_err(|e| e.to_string())?;
    list.retain(|k| k.entry.project != entry.project);
    list.push(kept.clone());
    save_kept(home, &list)?;
    Ok(kept)
}

/// Updates every installed registry project of game folder `home` that [`decide`] says, by the
/// index (fetched at most every ten minutes); None when updates are disabled or it can't be read.
/// An interrupted update is recovered first, even when updates are disabled or offline.
pub fn run(
    home: &Path,
    ctx: &Context,
    enabled: bool,
    progress: &dyn Fn(&str, f32),
) -> Option<Report> {
    // The start's run and Play's never touch a folder at once.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(notice) = recover(home) {
        return Some(Report {
            held: vec![notice],
            ..Report::default()
        });
    }
    if !enabled {
        return None;
    }
    let index = super::index().ok()?;
    Some(run_with(&index, super::local(), home, ctx, progress))
}

pub(super) fn run_with(
    index: &Index,
    local: bool,
    home: &Path,
    ctx: &Context,
    progress: &dyn Fn(&str, f32),
) -> Report {
    let _lock = match lock_for_updates(home) {
        Ok(guard) => guard,
        Err(notice) => {
            return Report {
                held: vec![notice],
                ..Report::default()
            };
        }
    };
    if let Err(notice) = recover(home) {
        return Report {
            held: vec![notice],
            ..Report::default()
        };
    }
    let mut report = Report::default();
    for entry in installed::Installed::load(home).entries {
        let Decision::Update(v) = decide(index, &entry, ctx) else {
            continue;
        };
        let p = index.project(&entry.project).expect("decide found it");
        match update(index, local, p, v, &entry, home, progress) {
            Ok(None) => {
                report.updated.push(format!("{} {}", p.name, v.version));
            }
            Ok(Some(why)) => report.held.push(why),
            Err(e) => report.failed.push(trf(
                "{title} wasn't updated: {error}",
                &[("title", &p.name), ("error", &e)],
            )),
        }
    }
    report
}

/// One update the player chose, by the path the automatic update takes: the replaced file is
/// kept until the update is done (an interrupted one is rolled back), the mod's on/off state is
/// never touched, and a content mod whose new version drops something worlds may use, or any
/// code mod, is held: `Ok(Some(why))`, nothing changed. Native mods have their own path.
pub fn update_to(
    index: &Index,
    project: &str,
    version: &str,
    home: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Option<String>, String> {
    let entry = installed::Installed::load(home)
        .find(SourceId::Sunrise, project)
        .cloned()
        .ok_or_else(|| {
            trf(
                "{project} wasn't installed from the registry.",
                &[("project", project)],
            )
        })?;
    if crate::native_status::is_native_file(&entry.file) {
        return Err(trf(
            "{project} is a native mod: it updates by its own path.",
            &[("project", project)],
        ));
    }
    let p = index.project(project).ok_or_else(|| {
        trf(
            "{project} isn't in Sunrise's registry.",
            &[("project", project)],
        )
    })?;
    let v = p
        .versions
        .iter()
        .find(|v| v.version == version && index.revoked(&p.id, &v.file.sha256).is_none())
        .ok_or_else(|| {
            trf(
                "{project} {version} can't be installed.",
                &[("project", project), ("version", version)],
            )
        })?;
    update(index, super::local(), p, v, &entry, home, progress)
}

/// Updates `entry` to `v`; Some(why) when it waits for the player instead.
fn update(
    index: &Index,
    local: bool,
    p: &index::Project,
    v: &index::Version,
    entry: &installed::Entry,
    home: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Option<String>, String> {
    let _lock = lock_for_updates(home)?;
    recover(home)?;
    let list = installed::Installed::load(home);
    let Some(current) = list.find(entry.source, &entry.project) else {
        return Ok(None);
    };
    if current.version != entry.version {
        return Ok(None);
    }
    let old: Option<PathBuf> = entry.path(home).filter(|p| p.is_file());
    let held = RefCell::new(None);
    // Content mods: nothing the worlds may hold goes missing.
    let guard = |new: &Path| -> Result<(), String> {
        let content = entry.file.ends_with(".smod") && v.file.name.ends_with(".smod");
        if !content {
            return Ok(());
        }
        let Some(old) = &old else {
            return Ok(());
        };
        let gone = dropped(old, new)?;
        if gone.is_empty() {
            return Ok(());
        }
        let why = trf(
            "{title} {version} waits for you: it no longer has {ids}, which your worlds may use. Update it on the Mods page when you're ready.",
            &[
                ("title", &p.name),
                ("version", &v.version),
                ("ids", &gone.join(", ")),
            ],
        );
        *held.borrow_mut() = Some(why.clone());
        Err(why)
    };
    let kept = match &old {
        Some(_) => Some(keep(home, entry)?),
        None => None,
    };
    match install_version_guarded(index, local, p, v, home, &guard, progress) {
        Ok(_) => Ok(None),
        Err(e) => {
            if let Some(kept) = &kept {
                finish_failed_update(home, kept, save_kept)
                    .map_err(|restore| format!("{e}; {restore}"))?;
            }
            match held.into_inner() {
                Some(why) => Ok(Some(why)),
                None => Err(e),
            }
        }
    }
}

/// Preserve the only preimage until its ledger transition succeeds. A failed transition leaves
/// the row in place; later recovery checks its actual bytes even in the same process.
fn finish_failed_update(
    home: &Path,
    kept: &Kept,
    save: impl FnOnce(&Path, &[Kept]) -> Result<(), String>,
) -> Result<(), String> {
    let mut list = kept_list(home)?;
    if !finished(home, kept) {
        if let Err(error) = restore(home, kept) {
            if let Some(record) = list
                .iter_mut()
                .find(|k| k.entry.project == kept.entry.project)
            {
                record.pid = 0;
                record.session = None;
            }
            return Err(match save(home, &list) {
                Ok(()) => format!("restore failed: {error}"),
                Err(save) => format!("restore failed: {error}; ledger save failed: {save}"),
            });
        }
    }
    list.retain(|k| k.entry.project != kept.entry.project);
    save(home, &list)?;
    let _ = fs::remove_file(home.join(&kept.copy));
    Ok(())
}

#[cfg(test)]
#[path = "auto/tests.rs"]
mod tests;
