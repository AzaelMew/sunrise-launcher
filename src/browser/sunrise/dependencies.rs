//! A manual registry install stages and verifies the whole set before changing the profile.
use super::{fs, index, installed, mod_list, Install, SourceId};
use semver::{Version, VersionReq};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use sunrise_launcher_smod::index::resolve::{self, Request, RequestResult};
use sunrise_launcher_smod::manifest::Side;
use sunrise_launcher_smod::mod_list::FILE as ENABLED;

pub(super) fn install(
    install: &Install<'_>,
    project: &index::Project,
    version: &index::Version,
    native_allowed: bool,
    turn_on: bool,
) -> Result<Vec<installed::Entry>, String> {
    let home = install.home;
    let _lock = crate::browser::lock_home(home).map_err(|e| e.to_string())?;
    let staged = tempfile::Builder::new()
        .prefix(".installed-mods-")
        .tempdir_in(home)
        .map_err(|e| format!("{}: {e}", home.display()))?;
    // An install an earlier run left half done is undone first; while it can't be, nothing new
    // is begun on top of its list.
    if !super::journal::recover(home) {
        return Err(
            "An earlier install is still being undone. Restart the launcher and try again.".into(),
        );
    }
    let mut installed = installed::Installed::load(home);
    // The mods this install replaces are out of the picture for the plan and go with it.
    let (removing, kept): (Vec<_>, Vec<_>) = installed
        .entries
        .drain(..)
        .partition(|e| e.source == SourceId::Sunrise && install.replacing.contains(&e.project));
    installed.entries = kept;
    // The index's claims plan the versions first: an unsatisfiable set is refused with the planner's
    // reason before anything downloads, and each dependency is fetched at the version it chose.
    let held = super::held_mods(&installed.entries);
    let planned = resolve::install(
        install.index,
        &[Request {
            id: project.id.clone(),
            version: Some(version.version.clone()),
        }],
        &held,
        &version.game,
        Side::Both,
        |_, v| super::fits_game(v, &version.game),
    );
    if let Some(RequestResult::Refused { reason }) = planned.requested.get(&project.id) {
        return Err(format!("{} can't be installed: {reason}", project.name));
    }
    // Native code the plan names is refused before anything downloads; the files are checked
    // again below, for what the index doesn't declare.
    let native_message = |native: &str| {
        crate::i18n::trf(
            "{name} needs {native}, which runs native code: allow native mods first.",
            &[("name", &project.name), ("native", native)],
        )
    };
    if !native_allowed {
        let planned_native = planned
            .install
            .iter()
            .map(|m| &m.id)
            .chain(planned.update.iter().map(|u| &u.id))
            .filter_map(|id| install.index.project(id))
            .find(|p| p.kind == index::Kind::NativeMod);
        if let Some(native) = planned_native {
            return Err(native_message(&native.name));
        }
    }
    let chosen: BTreeMap<String, String> = planned
        .install
        .into_iter()
        .map(|m| (m.id, m.version))
        .chain(planned.update.into_iter().map(|u| (u.id, u.to)))
        .collect();
    let mut plan = Plan {
        install,
        staged: staged.path(),
        installed,
        selected: Vec::new(),
        unreadable: Vec::new(),
        turn_on,
        chosen,
    };
    plan.add(project, version, 0)?;
    // Native code follows the player's one-time choice, whatever brings it: the mod asked
    // for or any of what it needs, by the files themselves, not by what the index declares.
    // The install interface asks first; this is what holds for the headless ones.
    if !native_allowed {
        if let Some((entry, _)) = plan
            .selected
            .iter()
            .find(|(e, _)| crate::native_status::is_native_file(&e.file))
        {
            return Err(native_message(&entry.title));
        }
    }

    let mut changes = Vec::new();
    let mut done = Vec::new();
    // Unreadable files whose new version goes elsewhere: published over nothing, so removed.
    let mut stale = Vec::new();
    let mut enable = if turn_on {
        vec![project.id.clone()]
    } else {
        Vec::new()
    };
    for (entry, file) in plan.selected {
        if let Some(file) = file {
            // A dependency that was installed and off stays off when it is updated.
            let was_off = plan
                .installed
                .find(SourceId::Sunrise, &entry.project)
                .is_some()
                && !mod_list::is_enabled(home, &entry.project);
            if let Some(old) = plan.installed.find(SourceId::Sunrise, &entry.project) {
                // A file that is gone or no longer parses is replaced wherever the new one goes
                // (a native mod and a plain one live in different places); a good one is not.
                let old_path = old.path(home);
                let replaceable = old_path.as_ref().is_some_and(|path| {
                    plan.unreadable.contains(path) || path.try_exists().is_ok_and(|there| !there)
                });
                if old.file != entry.file && !replaceable {
                    return Err(format!(
                        "{} is installed in a different location: remove it first.",
                        entry.title
                    ));
                }
                if old.file != entry.file {
                    stale.extend(old_path.filter(|path| plan.unreadable.contains(path)));
                }
            }
            changes.push((Some(file), home.join(&entry.file)));
            plan.installed
                .entries
                .retain(|e| e.source != SourceId::Sunrise || e.project != entry.project);
            plan.installed.entries.push(entry.clone());
            if turn_on && entry.project != project.id && !was_off {
                enable.push(entry.project.clone());
            }
            done.push(entry);
        } else if entry.project == project.id {
            done.push(entry);
        }
    }
    // Publishing drops what it replaces, so a file that no longer parses is copied into the
    // staging folder first, and kept in `mod-browser/unreadable` only once publishing worked.
    let mut aside = Vec::new();
    for (n, broken) in plan.unreadable.iter().enumerate() {
        let copy = staged.path().join(format!("aside-{n}"));
        fs::copy(broken, &copy).map_err(|e| format!("{}: {e}", broken.display()))?;
        aside.push((broken.clone(), copy));
    }
    // What it replaces is moved aside with the rest, so a failure brings it back.
    let mut disable = Vec::new();
    for old in &removing {
        if home.join(&old.file).symlink_metadata().is_ok() {
            changes.push((None, home.join(&old.file)));
        }
        disable.extend(super::mod_id(old).map(str::to_owned));
    }
    if !changes.is_empty() || !removing.is_empty() {
        plan.installed.save(staged.path())?;
        changes.push((
            Some(staged.path().join(installed::MANIFEST)),
            home.join(installed::MANIFEST),
        ));
    }
    let stays_on = |id: &String| !mod_list::is_enabled(home, id);
    if enable.iter().any(stays_on) || disable.iter().any(|id| mod_list::is_enabled(home, id)) {
        let saved = home.join(ENABLED);
        let next = staged.path().join(ENABLED);
        match fs::copy(&saved, &next) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", saved.display())),
        }
        let ids = enable.iter().map(String::as_str).collect::<Vec<_>>();
        mod_list::set_all_enabled(staged.path(), &ids, true)?;
        let ids = disable.iter().map(String::as_str).collect::<Vec<_>>();
        mod_list::set_all_enabled(staged.path(), &ids, false)?;
        changes.push((Some(next), saved));
    }
    let _staged = publish(staged, changes)?;
    // The files are in place and the list is still kept: what is left to do is part of the
    // install, and a failure in it puts the files back.
    super::journal::commit(home, || {
        for (broken, copy) in aside {
            let to = super::unreadable_slot(home, &broken)?;
            fs::copy(copy, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        for path in stale {
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    return Err(format!("{}: {e}", path.display()));
                }
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(done)
}

struct Plan<'a> {
    install: &'a Install<'a>,
    staged: &'a Path,
    installed: installed::Installed,
    selected: Vec<(installed::Entry, Option<PathBuf>)>,
    /// Installed files that no longer parse and are about to be replaced.
    unreadable: Vec<PathBuf>,
    /// False when an installed mod that is off is being updated and must stay off.
    turn_on: bool,
    /// The version the index plan chose for each mod it installs or updates.
    chosen: BTreeMap<String, String>,
}

impl Plan<'_> {
    fn add(&mut self, p: &index::Project, v: &index::Version, depth: usize) -> Result<(), String> {
        if depth > installed::MAX_DEPTH {
            return Err(format!("{}'s dependencies go too deep.", p.name));
        }
        let (mut entry, download, unreadable) =
            self.install
                .prepare(p, v, &self.installed, &self.staged.join(&p.id))?;
        self.unreadable.extend(unreadable);
        let (path, facts) = match download {
            Some((path, facts)) => {
                entry.dependency = depth != 0
                    || self
                        .installed
                        .find(SourceId::Sunrise, &p.id)
                        .is_some_and(|e| e.dependency);
                (Some(path), facts)
            }
            None => {
                let (_, facts) = index::check_file_with_facts(
                    self.install.index,
                    p,
                    v,
                    &self.install.home.join(&entry.file),
                )
                .map_err(|e| format!("Cannot use installed {}: {e}", p.name))?;
                (None, facts)
            }
        };
        self.selected.push((entry, path));
        for (id, dependency) in facts.dependencies {
            let dep = dependency.details();
            if !dep.optional {
                self.require(&id, &dep.version, &v.game, depth + 1)
                    .map_err(|e| format!("{} requires {id} {}: {e}", p.name, dep.version))?;
            }
        }
        Ok(())
    }

    fn require(
        &mut self,
        id: &str,
        range: &VersionReq,
        game: &str,
        depth: usize,
    ) -> Result<(), String> {
        let index = self.install.index;
        let matches = |version: &str| Version::parse(version).is_ok_and(|v| range.matches(&v));
        if let Some((entry, _)) = self.selected.iter().find(|(e, _)| e.project == id) {
            return if matches(&entry.version) {
                Ok(())
            } else {
                Err(format!(
                    "{} is already selected for this install",
                    entry.version
                ))
            };
        }
        let p = index
            .project(id)
            .filter(|p| matches!(p.kind, index::Kind::Mod | index::Kind::NativeMod))
            .ok_or("no such mod in the Sunrise registry")?;
        let candidates = p
            .versions
            .iter()
            .filter(|v| v.game == game && matches(&v.version));
        // Only a mod the plan names is held to the planned version: one it doesn't list (its owner
        // carried no claim, or the plan kept an installed copy) takes the first version in range.
        let candidates: Vec<_> = candidates
            .filter(|v| v.capabilities.unknown().is_empty())
            .filter(|v| {
                self.chosen
                    .get(id)
                    .is_none_or(|chosen| *chosen == v.version)
            })
            .collect();
        let v = if let Some(old) = self.installed.find(SourceId::Sunrise, id) {
            if let Some(kept) = candidates.iter().find(|v| v.version == old.version) {
                if self.turn_on && !mod_list::is_enabled(self.install.home, id) {
                    return Err("it is installed but off; turn it on first".into());
                }
                kept
            } else if self.chosen.contains_key(id) {
                // The plan moves it: the version it chose replaces the installed one, which
                // stays on or off as it is (`Plan::kept_off`).
                let mut taken_down = None;
                candidates
                    .iter()
                    .find(|v| {
                        if let Some(r) = index.revoked(id, &v.file.sha256) {
                            taken_down = Some(&r.reason);
                            false
                        } else {
                            true
                        }
                    })
                    .ok_or_else(|| match taken_down {
                        Some(reason) => format!("taken down: {reason}"),
                        None => format!("no matching version for Minecraft {game}"),
                    })?
            } else {
                return Err(format!(
                    "installed {} does not match; change it first",
                    old.version
                ));
            }
        } else {
            let mut taken_down = None;
            let v = candidates
                .iter()
                .find(|v| {
                    if let Some(r) = index.revoked(id, &v.file.sha256) {
                        taken_down = Some(&r.reason);
                        false
                    } else {
                        true
                    }
                })
                .ok_or_else(|| match taken_down {
                    Some(reason) => format!("taken down: {reason}"),
                    None => format!("no matching version for Minecraft {game}"),
                })?;
            v
        };
        self.add(p, v, depth)
    }
}

/// Same-filesystem moves with an undo list; even a failed metadata write restores old files.
/// The list stays until the caller commits (`journal::commit`): publishing is not the end.
/// The profile lock keeps another launcher or game start from seeing an intermediate state.
fn publish(
    staged: tempfile::TempDir,
    changes: Vec<(Option<PathBuf>, PathBuf)>,
) -> Result<tempfile::TempDir, String> {
    let home = staged
        .path()
        .parent()
        .expect("staged in the profile")
        .to_path_buf();
    let targets: Vec<PathBuf> = changes.iter().map(|(_, to)| to.clone()).collect();
    super::journal::begin(&home, staged.path(), &targets)?;
    let mut undo = Vec::new();
    for (i, (from, to)) in changes.into_iter().enumerate() {
        let result = (|| -> io::Result<()> {
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir)?;
            }
            let backup = match fs::symlink_metadata(&to) {
                Ok(m) if m.is_file() => {
                    let backup = staged.path().join(format!("old-{i}"));
                    fs::rename(&to, &backup)?;
                    Some(backup)
                }
                Ok(_) => return Err(io::Error::other("not a regular file")),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            undo.push((to.clone(), backup, false));
            // No file to put there: the one that was is removed.
            if let Some(from) = from {
                fs::rename(&from, &to)?;
                undo.last_mut().expect("just recorded").2 = true;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let mut rollback_errors = Vec::new();
            for (file, backup, placed) in undo.into_iter().rev() {
                let restored = match backup {
                    Some(backup) => fs::rename(backup, &file),
                    None if placed => fs::remove_file(&file),
                    None => Ok(()),
                };
                if let Err(e) = restored {
                    rollback_errors.push(format!("{}: {e}", file.display()));
                }
            }
            let message = format!("Cannot install {}: {e}", to.display());
            return Err(if rollback_errors.is_empty() {
                match super::journal::finish(&home) {
                    Ok(()) => message,
                    Err(e) => format!("{message}; {e}"),
                }
            } else {
                format!(
                    "{message}; rollback failed: {}. Backups: {}",
                    rollback_errors.join("; "),
                    staged.keep().display()
                )
            });
        }
    }
    Ok(staged)
}
