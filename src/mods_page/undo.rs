//! What an install replaced or added, kept in memory until its build ends: a build that stops
//! for a reason outside the mods puts every mod it touched back as it was (its old version, its
//! entry and whether it was on), and removes the ones the install added.
//!
//! The same is kept in the profile while the build is outstanding ([`begin`]), written
//! before the install starts publishing, so a launcher that closes or crashes at any point from
//! then on puts the mods back at its next start ([`recover`]), after the install's own journal
//! has been undone. It is cleared when the build ends or is undone ([`clear`]); a build that
//! waits for the player stays kept.
use serde::{Deserialize, Serialize};
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

/// The mods installing registry mod `id` can touch among `entries`: itself, what the plan
/// moves to another version and the `replacing` it removes, with their state now. Read before
/// the install starts.
pub(super) fn capture(
    home: &Path,
    entries: &[installed::Entry],
    id: &str,
    game: &str,
    replacing: &[String],
) -> Vec<Prior> {
    let mut ids = vec![id.to_owned()];
    ids.extend_from_slice(replacing);
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

/// What the install `done` touched, with each one's `priors`: what it put in, and what it
/// took out (a prior that isn't in `done`).
pub(super) fn touched(home: &Path, done: &[installed::Entry], priors: &[Prior]) -> Vec<Touched> {
    let put = done.iter().map(|e| Touched {
        home: home.to_owned(),
        id: e.project.clone(),
        prior: priors
            .iter()
            .find(|p| p.entry.project == e.project)
            .cloned(),
    });
    let removed = priors
        .iter()
        .filter(|p| !done.iter().any(|e| e.project == p.entry.project))
        .map(|p| Touched {
            home: home.to_owned(),
            id: p.entry.project.clone(),
            prior: Some(p.clone()),
        });
    put.chain(removed).collect()
}

/// Takes out of `list` what was touched in profile `home`: one profile's build is the only
/// one that may undo or clear it. The other profiles' stay in the list.
pub(super) fn take_for(list: &mut Vec<Touched>, home: &Path) -> Vec<Touched> {
    let (mine, others) = std::mem::take(list)
        .into_iter()
        .partition(|t| t.home == home);
    *list = others;
    mine
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

/// Where a profile keeps its outstanding build's undo record, one file written whole beside
/// a temporary one and renamed over the last (never half written, never emptied first).
const RECORD: &str = "mod-browser/build-undo.json";

/// One mod as it was, in the record: its file is stored in base64.
#[derive(Clone, Serialize, Deserialize)]
struct SavedPrior {
    entry: installed::Entry,
    on: bool,
    file: String,
}

/// What the build's installs changed, as the record keeps it: the registry mods installed
/// before the earliest of them (whatever is installed now and isn't listed was added) and the
/// state of each mod that was replaced, updated or removed.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Kept {
    before: Vec<String>,
    priors: Vec<SavedPrior>,
}

fn read(home: &Path) -> Option<Kept> {
    serde_json::from_str(&std::fs::read_to_string(home.join(RECORD)).ok()?).ok()
}

/// Writes `kept` (or nothing, to take the record away) so that the old record stays whole
/// until the new one is complete.
fn write(home: &Path, kept: Option<&Kept>) -> Result<(), String> {
    let record = home.join(RECORD);
    let Some(kept) = kept else {
        return match std::fs::remove_file(&record) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", record.display()))
            }
            _ => Ok(()),
        };
    };
    let next = record.with_extension("json.new");
    if let Some(dir) = record.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string(kept).map_err(|e| e.to_string())?;
    std::fs::write(&next, text).map_err(|e| format!("{}: {e}", next.display()))?;
    std::fs::rename(&next, &record).map_err(|e| format!("{}: {e}", record.display()))
}

/// Before an install of this build publishes anything: adds its `priors` (the exact files,
/// entries and on/off of what it may replace, update or remove) to the record, beside what the
/// build's earlier installs kept (the earliest state of each mod stays). Returns what the
/// record said before, to give back with [`restore`] when the install ends with nothing for
/// the build to take up. A failure is logged: the in-memory list still undoes the build.
pub(super) fn begin(home: &Path, entries: &[installed::Entry], priors: &[Prior]) -> Option<Kept> {
    let previous = read(home);
    let mut next = previous.clone().unwrap_or_else(|| Kept {
        before: entries
            .iter()
            .filter(|e| e.source == SourceId::Sunrise)
            .map(|e| e.project.clone())
            .collect(),
        priors: Vec::new(),
    });
    for p in priors {
        if !next
            .priors
            .iter()
            .any(|have| have.entry.project == p.entry.project)
        {
            next.priors.push(SavedPrior {
                entry: p.entry.clone(),
                on: p.on,
                file: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &p.file),
            });
        }
    }
    if let Err(e) = write(home, Some(&next)) {
        eprintln!("Couldn't keep the build's undo record: {e}");
    }
    previous
}

/// The install ended with nothing for the build to take up, or failed and undid itself: the
/// record is as it was before it began.
pub(super) fn restore(home: &Path, previous: Option<Kept>) {
    if let Err(e) = write(home, previous.as_ref()) {
        eprintln!("Couldn't give back the build's undo record: {e}");
    }
}

/// The build ended or was undone: the record is no longer needed.
pub(super) fn clear(home: &Path) {
    let _ = write(home, None);
}

/// At start, for a profile whose build never ended and whose install journal is already
/// undone: puts its mods back as the record says and removes what was added since. A mod that
/// can't be put back keeps the record for the next start.
pub(crate) fn recover(home: &Path) {
    let Some(kept) = read(home) else {
        // A write that was cut off before its rename left only the new file: nothing used it.
        let _ = std::fs::remove_file(home.join(RECORD).with_extension("json.new"));
        return;
    };
    let mut undone = true;
    let mut touched: Vec<Touched> = Vec::new();
    for saved in &kept.priors {
        let Ok(file) =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &saved.file)
        else {
            eprintln!("Couldn't read the kept file of {}", saved.entry.project);
            undone = false;
            continue;
        };
        touched.push(Touched {
            home: home.to_owned(),
            id: saved.entry.project.clone(),
            prior: Some(Prior {
                entry: saved.entry.clone(),
                on: saved.on,
                file,
            }),
        });
    }
    let added: Vec<Touched> = installed::Installed::load(home)
        .entries
        .iter()
        .filter(|e| e.source == SourceId::Sunrise && !kept.before.contains(&e.project))
        .filter(|e| !touched.iter().any(|t| t.id == e.project))
        .map(|e| Touched {
            home: home.to_owned(),
            id: e.project.clone(),
            prior: None,
        })
        .collect();
    for t in touched.iter().chain(&added) {
        if let Err(e) = t.undo() {
            eprintln!("Couldn't put {} back: {e}", t.id);
            undone = false;
        }
    }
    if undone {
        clear(home);
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
        let priors = capture(home, &list.entries, "flywheel", "26.3", &[]);
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

    #[test]
    fn a_build_cut_off_by_a_crash_is_rolled_back_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("sunrise-mods")).unwrap();
        // flywheel 0.3 is off and gets replaced; dyno is what a conflict removes; owls was
        // there before and stays.
        let old = entry("flywheel", "0.3.0", "sunrise-mods/flywheel.smod");
        let dyno = entry("dyno", "1.0.0", "sunrise-mods/dyno.smod");
        let owls = entry("owls", "1.0.0", "sunrise-mods/owls.smod");
        for e in [&old, &dyno, &owls] {
            std::fs::write(home.join(&e.file), format!("old {}", e.project)).unwrap();
        }
        let mut list = installed::Installed::default();
        list.entries
            .extend([old.clone(), dyno.clone(), owls.clone()]);
        list.save(home).unwrap();
        mod_list::set_enabled(home, "flywheel", false).unwrap();
        mod_list::set_enabled(home, "dyno", true).unwrap();

        // The record is written before the install publishes anything.
        let priors = capture(
            home,
            &list.entries,
            "flywheel",
            "26.3",
            &["dyno".to_string()],
        );
        assert_eq!(priors.len(), 2, "what it updates and what it removes");
        let previous = begin(home, &list.entries, &priors);
        assert!(previous.is_none());

        // The install published: flywheel replaced, dyno removed, lamps added. Then a second
        // install of the same build begins, with the state the first one left.
        let new = entry("flywheel", "0.4.0", "sunrise-mods/flywheel.smod");
        let added = entry("lamps", "1.0.0", "sunrise-mods/lamps.smod");
        std::fs::write(home.join(&new.file), "new bytes").unwrap();
        std::fs::write(home.join(&added.file), "lamps").unwrap();
        std::fs::remove_file(home.join(&dyno.file)).unwrap();
        let mut list = installed::Installed::default();
        list.entries
            .extend([new.clone(), added.clone(), owls.clone()]);
        list.save(home).unwrap();
        mod_list::set_enabled(home, "flywheel", true).unwrap();
        mod_list::set_enabled(home, "lamps", true).unwrap();
        let second = capture(home, &list.entries, "flywheel", "26.3", &[]);
        begin(home, &list.entries, &second);
        assert!(!home.join(RECORD).with_extension("json.new").exists());

        // The launcher is gone. Its next start reads the profile.
        recover(home);

        let list = installed::Installed::load(home);
        assert_eq!(list.find(SourceId::Sunrise, "flywheel"), Some(&old));
        assert_eq!(list.find(SourceId::Sunrise, "dyno"), Some(&dyno));
        assert!(list.find(SourceId::Sunrise, "owls").is_some());
        assert!(list.find(SourceId::Sunrise, "lamps").is_none());
        assert_eq!(
            std::fs::read_to_string(home.join(&old.file)).unwrap(),
            "old bytes".replace("bytes", "flywheel")
        );
        assert_eq!(
            std::fs::read_to_string(home.join(&dyno.file)).unwrap(),
            "old dyno"
        );
        assert!(!home.join("sunrise-mods/lamps.smod").exists());
        assert!(!mod_list::is_enabled(home, "flywheel"), "it was off");
        assert!(mod_list::is_enabled(home, "dyno"));
        assert!(!home.join(RECORD).exists(), "and it is not undone twice");
    }

    #[test]
    fn an_install_that_ends_with_nothing_to_build_gives_the_record_back() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("sunrise-mods")).unwrap();
        let old = entry("flywheel", "0.3.0", "sunrise-mods/flywheel.smod");
        std::fs::write(home.join(&old.file), "old").unwrap();
        let mut list = installed::Installed::default();
        list.entries.push(old);
        list.save(home).unwrap();
        let priors = capture(home, &list.entries, "flywheel", "26.3", &[]);
        let previous = begin(home, &list.entries, &priors);
        assert!(home.join(RECORD).is_file(), "kept while the install runs");
        restore(home, previous);
        assert!(!home.join(RECORD).exists());
        // A record from an earlier install of the build comes back whole.
        let first = begin(home, &list.entries, &priors);
        let again = begin(home, &list.entries, &[]);
        assert!(again.is_some());
        restore(home, again);
        assert!(home.join(RECORD).is_file());
        drop(first);
        clear(home);
        recover(home);
        assert_eq!(installed::Installed::load(home).entries.len(), 1);
    }

    #[test]
    fn one_profiles_build_takes_only_its_own_mods_from_the_list() {
        let (a, b) = (PathBuf::from("/a"), PathBuf::from("/b"));
        let touched = |home: &PathBuf, id: &str| Touched {
            home: home.clone(),
            id: id.into(),
            prior: None,
        };
        let mut list = vec![touched(&a, "one"), touched(&b, "two"), touched(&a, "three")];
        let mine = take_for(&mut list, &b);
        assert_eq!(mine, [touched(&b, "two")]);
        assert_eq!(list, [touched(&a, "one"), touched(&a, "three")]);
        assert!(take_for(&mut list, &b).is_empty());
    }
}
