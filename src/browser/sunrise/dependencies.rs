//! A manual registry install stages and verifies the whole set before changing the profile.
use super::{fs, index, installed, mod_list, Install, SourceId};
use semver::{Version, VersionReq};
use std::io;
use std::path::{Path, PathBuf};
use sunrise_launcher_smod::mod_list::FILE as ENABLED;

pub(super) fn install(
    install: &Install<'_>,
    project: &index::Project,
    version: &index::Version,
    native_allowed: bool,
) -> Result<Vec<installed::Entry>, String> {
    let home = install.home;
    let _lock = crate::browser::lock_home(home).map_err(|e| e.to_string())?;
    let staged = tempfile::Builder::new()
        .prefix(".installed-mods-")
        .tempdir_in(home)
        .map_err(|e| format!("{}: {e}", home.display()))?;
    let mut plan = Plan {
        install,
        staged: staged.path(),
        installed: installed::Installed::load(home),
        selected: Vec::new(),
        native_allowed,
    };
    plan.add(project, version, 0)?;

    let mut changes = Vec::new();
    let mut done = Vec::new();
    let mut enable = vec![project.id.clone()];
    for (entry, file) in plan.selected {
        if let Some(file) = file {
            if let Some(old) = plan.installed.find(SourceId::Sunrise, &entry.project) {
                if old.file != entry.file {
                    return Err(format!(
                        "{} is installed in a different location: remove it first.",
                        entry.title
                    ));
                }
            }
            changes.push((file, home.join(&entry.file)));
            plan.installed
                .entries
                .retain(|e| e.source != SourceId::Sunrise || e.project != entry.project);
            plan.installed.entries.push(entry.clone());
            if entry.project != project.id {
                enable.push(entry.project.clone());
            }
            done.push(entry);
        } else if entry.project == project.id {
            done.push(entry);
        }
    }
    if !changes.is_empty() {
        plan.installed.save(staged.path())?;
        changes.push((
            staged.path().join(installed::MANIFEST),
            home.join(installed::MANIFEST),
        ));
    }
    if enable.iter().any(|id| !mod_list::is_enabled(home, id)) {
        let saved = home.join(ENABLED);
        let next = staged.path().join(ENABLED);
        match fs::copy(&saved, &next) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", saved.display())),
        }
        let ids = enable.iter().map(String::as_str).collect::<Vec<_>>();
        mod_list::set_all_enabled(staged.path(), &ids, true)?;
        changes.push((next, saved));
    }
    publish(staged, changes)?;
    Ok(done)
}

struct Plan<'a> {
    install: &'a Install<'a>,
    staged: &'a Path,
    installed: installed::Installed,
    selected: Vec<(installed::Entry, Option<PathBuf>)>,
    native_allowed: bool,
}

impl Plan<'_> {
    fn add(&mut self, p: &index::Project, v: &index::Version, depth: usize) -> Result<(), String> {
        if depth > installed::MAX_DEPTH {
            return Err(format!("{}'s dependencies go too deep.", p.name));
        }
        let (mut entry, download) =
            self.install
                .prepare(p, v, &self.installed, &self.staged.join(&p.id))?;
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
        let mut candidates = candidates.filter(|v| v.capabilities.unknown().is_empty());
        let v = if let Some(old) = self.installed.find(SourceId::Sunrise, id) {
            let v = candidates
                .find(|v| v.version == old.version)
                .ok_or_else(|| {
                    format!("installed {} does not match; change it first", old.version)
                })?;
            if !mod_list::is_enabled(self.install.home, id) {
                return Err("it is installed but off; turn it on first".into());
            }
            v
        } else {
            let mut taken_down = None;
            let v = candidates
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
            if p.kind == index::Kind::NativeMod && !self.native_allowed {
                return Err(
                    "it runs native code; install it explicitly to grant permission first".into(),
                );
            }
            v
        };
        self.add(p, v, depth)
    }
}

/// Same-filesystem moves with an undo list; even a failed metadata write restores old files.
/// The profile lock keeps another launcher or game start from seeing an intermediate state.
fn publish(staged: tempfile::TempDir, changes: Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
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
            fs::rename(&from, &to)?;
            undo.last_mut().expect("just recorded").2 = true;
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
                message
            } else {
                format!(
                    "{message}; rollback failed: {}. Backups: {}",
                    rollback_errors.join("; "),
                    staged.keep().display()
                )
            });
        }
    }
    Ok(())
}
