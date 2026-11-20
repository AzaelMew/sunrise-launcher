//! What an install replaced or added, kept in memory until its build ends: a build that stops
//! for a reason outside the mods puts every mod it touched back as it was (its old version, its
//! entry and whether it was on), and removes the ones the install added.
use std::path::{Path, PathBuf};
use sunrise_launcher::browser::{installed, mod_list, sunrise, SourceId};

/// A mod as it was before the install: its entry, whether it was on, and its file.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Prior {
    entry: installed::Entry,
    on: bool,
    file: Vec<u8>,
}

/// A mod an install touched in profile `home`: its state before, or `None` when the install
/// added it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Touched {
    pub(super) home: PathBuf,
    pub(super) id: String,
    prior: Option<Prior>,
}

/// The mods installing registry mod `id` can touch among `entries`: itself and what the plan
/// moves to another version, with their state now. Read before the install starts.
pub(super) fn capture(
    home: &Path,
    entries: &[installed::Entry],
    id: &str,
    game: &str,
) -> Vec<Prior> {
    let mut ids = vec![id.to_owned()];
    if let Some(index) = sunrise::known_index() {
        let held = sunrise::held_mods(entries);
        if let Ok(plan) =
            sunrise::check_set(&index, std::slice::from_ref(&id.to_owned()), &held, game)
        {
            ids.extend(plan.update.into_iter().map(|u| u.id));
        }
    }
    entries
        .iter()
        .filter(|e| e.source == SourceId::Sunrise && ids.contains(&e.project))
        .filter_map(|entry| {
            let file = std::fs::read(entry.path(home)?).ok()?;
            let on = sunrise::mod_id(entry).is_some_and(|id| mod_list::is_enabled(home, id));
            Some(Prior {
                entry: entry.clone(),
                on,
                file,
            })
        })
        .collect()
}

/// What the install `done` touched, with each one's `priors`.
pub(super) fn touched(home: &Path, done: &[installed::Entry], priors: &[Prior]) -> Vec<Touched> {
    done.iter()
        .map(|e| Touched {
            home: home.to_owned(),
            id: e.project.clone(),
            prior: priors
                .iter()
                .find(|p| p.entry.project == e.project)
                .cloned(),
        })
        .collect()
}

impl Touched {
    /// Puts the mod back as it was, or removes it when the install added it.
    pub(super) fn undo(&self) -> Result<(), String> {
        let home = &self.home;
        let Some(prior) = &self.prior else {
            return installed::remove(home, &self.id).map(|_| ());
        };
        let _lock = sunrise_launcher::browser::lock_home(home).map_err(|e| e.to_string())?;
        let mut list = installed::Installed::load(home);
        // The install may have put the new file elsewhere (a plain mod made native).
        if let Some(now) = list.find(SourceId::Sunrise, &self.id) {
            if now.file != prior.entry.file {
                if let Some(path) = now.path(home) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        let path = prior
            .entry
            .path(home)
            .ok_or_else(|| format!("{} is outside the game folder", prior.entry.file))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, &prior.file).map_err(|e| format!("{}: {e}", path.display()))?;
        list.entries
            .retain(|e| !(e.source == SourceId::Sunrise && e.project == self.id));
        list.entries.push(prior.entry.clone());
        list.save(home)?;
        if let Some(id) = sunrise::mod_id(&prior.entry) {
            mod_list::set_enabled(home, id, prior.on)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(project: &str, version: &str, file: &str) -> installed::Entry {
        installed::Entry {
            source: SourceId::Sunrise,
            project: project.into(),
            slug: project.into(),
            title: project.into(),
            kind: sunrise_launcher::browser::Kind::Mod,
            version: version.into(),
            number: version.into(),
            file: file.into(),
            checksum: format!("SHA-256:{version}"),
            page: String::new(),
            dependency: false,
            verdict: None,
        }
    }

    #[test]
    fn a_stopped_build_puts_an_updated_mod_back_as_it_was_and_removes_an_added_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("sunrise-mods")).unwrap();
        // A native mod at 0.3, turned off, which the install updates to 0.4; a content mod
        // that the install adds.
        let old = entry("flywheel", "0.3.0", "sunrise-mods/flywheel.smod");
        std::fs::write(home.join(&old.file), "old bytes").unwrap();
        let mut list = installed::Installed::default();
        list.entries.push(old.clone());
        list.save(home).unwrap();
        mod_list::set_enabled(home, "flywheel", false).unwrap();
        let priors = capture(home, &list.entries, "flywheel", "26.3");
        assert_eq!(priors.len(), 1);

        let new = entry("flywheel", "0.4.0", "sunrise-mods/flywheel.smod");
        let added = entry("lamps", "1.0.0", "sunrise-mods/lamps.smod");
        std::fs::write(home.join(&new.file), "new bytes").unwrap();
        std::fs::write(home.join(&added.file), "lamps").unwrap();
        let mut list = installed::Installed::default();
        list.entries.extend([new.clone(), added.clone()]);
        list.save(home).unwrap();
        mod_list::set_enabled(home, "flywheel", true).unwrap();
        mod_list::set_enabled(home, "lamps", true).unwrap();

        for t in touched(home, &[new, added], &priors) {
            t.undo().unwrap();
        }

        let list = installed::Installed::load(home);
        assert_eq!(list.find(SourceId::Sunrise, "flywheel"), Some(&old));
        assert!(list.find(SourceId::Sunrise, "lamps").is_none());
        assert_eq!(
            std::fs::read_to_string(home.join(&old.file)).unwrap(),
            "old bytes"
        );
        assert!(!home.join("sunrise-mods/lamps.smod").exists());
        assert!(!mod_list::is_enabled(home, "flywheel"), "it was off");
    }
}
