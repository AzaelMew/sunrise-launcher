//! Everything on the Mods screens that changes files. Each action is refused while the game or
//! a launch step is running; one that could take content out of a saved world (turning off,
//! removing or updating a mod a world was saved with) stops first and says which worlds, and
//! only goes on after each of those worlds is backed up and the backup read back. If the worlds
//! can't be read or a backup fails, nothing is changed.
use super::model::{self, Included, Row};
use super::*;
use sunrise_launcher::browser::sunrise::auto;
use sunrise_launcher::worlds;

#[derive(Clone)]
pub(super) enum Action {
    /// (mod id, title)
    TurnOff(Vec<(String, String)>),
    /// (mod id, project id, title)
    Remove(Vec<(String, String, String)>),
    Update(Vec<Included>),
}

impl Action {
    /// (mod id, title) of every mod the action touches.
    pub(super) fn mods(&self) -> Vec<(String, String)> {
        match self {
            Action::TurnOff(v) => v.clone(),
            Action::Remove(v) => v.iter().map(|(i, _, t)| (i.clone(), t.clone())).collect(),
            Action::Update(v) => v.iter().map(|i| (i.id.clone(), i.title.clone())).collect(),
        }
    }

    pub(super) fn consequence(&self) -> &'static str {
        match self {
            Action::TurnOff(_) => tr("Turning them off"),
            Action::Remove(_) => tr("Removing them"),
            Action::Update(_) => tr("Updating them"),
        }
    }
}

impl Launcher {
    /// Why nothing may change now: the game, a launch step or a build is under way.
    pub(crate) fn alt_mods_locked(&self) -> Option<&'static str> {
        if self.client.is_some()
            || self.starting_client.is_some()
            || self.starting_instance.is_some()
        {
            Some(tr(
                "The game is running or starting. Close it before changing mods.",
            ))
        } else if self.preparing
            || self.installing.is_some()
            || self.play_when_prepared.is_some()
            || self.play_after_build.is_some()
        {
            Some(tr(
                "Sunrise is getting ready to play. Wait for it to finish before changing mods.",
            ))
        } else if self.native_step.is_some() {
            Some(tr(
                "A native build is running. Wait for it to finish before changing mods.",
            ))
        } else {
            None
        }
    }

    /// The gate every change passes: not an example, not locked, nothing else running.
    fn alt_guard(&mut self) -> bool {
        let refusal = if self.alt.mods.examples {
            Some(tr("These are examples: nothing can be changed.").to_string())
        } else if let Some(why) = self.alt_mods_locked() {
            Some(why.to_string())
        } else if self.alt_mutating() {
            Some(tr("Wait for the current operation to finish.").to_string())
        } else {
            None
        };
        if let Some(why) = refusal {
            self.alt.mods.notice = Some(Err(why));
            return false;
        }
        true
    }

    /// Add `id` to the game: the existing install path, which asks first for native code, checks
    /// the file against the index and its signature, and refuses what was taken down. Only a mod
    /// the registry says fits this build.
    pub(crate) fn alt_mods_install(&mut self, id: String) -> Result<(), String> {
        let fit = match &self.alt.mods.catalog {
            Catalog::Ready(cards, _) => cards.iter().find(|c| c.id == id).map(|c| c.fit.clone()),
            _ => None,
        };
        let Some(fit) = fit else {
            return Err(tr("The registry hasn't answered about this mod yet.").to_string());
        };
        if let Some(why) = fit.reason() {
            return Err(why);
        }
        if !self.alt_guard() {
            return Err(match &self.alt.mods.notice {
                Some(Err(why)) => why.clone(),
                _ => tr("Can't change mods right now.").to_string(),
            });
        }
        self.alt.mods.notice = None;
        self.mods.message = None;
        self.mods_install(SourceId::Sunrise, id, None);
        Ok(())
    }

    pub(crate) fn alt_add(&mut self, id: String) {
        if let Err(why) = self.alt_mods_install(id) {
            self.alt.mods.notice = Some(Err(why));
        }
    }

    /// One mod on or off. On: the existing switch, with its native-permission and taken-down
    /// checks. Off: like any change that could take content out of a world, after the worlds
    /// check.
    pub(super) fn alt_toggle(&mut self, row: &Row) {
        if row.example || row.local.is_some() || !self.alt_guard() {
            return;
        }
        let Some(entry) = row.entry.clone() else {
            return;
        };
        if row.on {
            self.alt_run(
                Action::TurnOff(vec![(row.id.clone(), row.title.clone())]),
                false,
            );
            return;
        }
        self.mods.message = None;
        self.mods_switch(&entry);
        if let Some(Err(e)) = self.mods.message.take() {
            self.alt.mods.notice = Some(Err(e));
        }
    }

    /// Turn the chosen mods on in one rewrite of the list. A mod that can't be turned on (taken
    /// down, or native code with no permission) stays as it is and its row says why.
    pub(super) fn alt_bulk_on(&mut self) {
        if !self.alt_guard() {
            return;
        }
        let rows = self.alt_rows();
        let chosen: Vec<&Row> = rows
            .iter()
            .filter(|r| !r.example && r.local.is_none() && self.alt.mods.selection.contains(&r.id))
            .collect();
        let (go, held) =
            model::turn_on_plan(&chosen, self.settings.native_mods_allowed_at.is_some());
        let m = &mut self.alt.mods;
        m.errors = held
            .iter()
            .map(|(r, why)| (r.id.clone(), why.clone()))
            .collect();
        let Some(home) = m.snapshot.as_ref().map(|s| s.home.clone()) else {
            return;
        };
        if go.is_empty() {
            m.notice = Some(Err(tr("Nothing selected can be turned on yet.").to_string()));
            return;
        }
        let Some(_lock) = sunrise_launcher::browser::try_lock_home(&home) else {
            m.notice = Some(Err(
                tr("Wait for the current operation to finish.").to_string()
            ));
            return;
        };
        let ids: Vec<&str> = go.iter().map(|r| r.id.as_str()).collect();
        let count = ids.len().to_string();
        m.notice = Some(match java::set_all_enabled(&home, &ids, true) {
            Ok(()) if held.is_empty() => Ok(trf(
                "Turned on {count} mods. The game reads this when it starts.",
                &[("count", &count)],
            )),
            Ok(()) => Err(trf(
                "Turned on {done}. {held} stayed off: see their rows.",
                &[("done", &count), ("held", &held.len().to_string())],
            )),
            Err(e) => Err(e),
        });
    }

    pub(super) fn alt_bulk_off(&mut self) {
        let rows = self.alt_rows();
        let chosen: Vec<(String, String)> = rows
            .iter()
            .filter(|r| {
                !r.example && r.local.is_none() && r.on && self.alt.mods.selection.contains(&r.id)
            })
            .map(|r| (r.id.clone(), r.title.clone()))
            .collect();
        if chosen.is_empty() {
            self.alt.mods.notice = Some(Err(
                tr("None of the selected mods is turned on.").to_string()
            ));
            return;
        }
        self.alt_run(Action::TurnOff(chosen), false);
    }

    /// Asks for confirmation before removing the chosen mods.
    pub(super) fn alt_ask_remove(&mut self) {
        let rows = self.alt_rows();
        let chosen: Vec<(String, String, String)> = rows
            .iter()
            .filter(|r| !r.example && r.local.is_none() && self.alt.mods.selection.contains(&r.id))
            .filter_map(|r| {
                Some((
                    r.id.clone(),
                    r.entry.as_ref()?.project.clone(),
                    r.title.clone(),
                ))
            })
            .collect();
        if !chosen.is_empty() {
            self.alt.mods.confirm = Some(Confirm::Remove(chosen));
        }
    }

    /// Plans "Update N mods" for the whole pack, or for the selected rows only.
    pub(super) fn alt_ask_update(&mut self, only_selected: bool) {
        let rows: Vec<Row> = self
            .alt_rows()
            .into_iter()
            .filter(|r| !only_selected || self.alt.mods.selection.contains(&r.id))
            .collect();
        let plan = model::plan_updates(&rows, self.settings.native_mods_allowed_at.is_some());
        self.alt.mods.confirm = Some(Confirm::Update(plan));
    }

    /// One mod's update: the same reviewed path as the batch, for one mod.
    pub(super) fn alt_update_one(&mut self, row: &Row) {
        let Some(to) = row
            .update
            .clone()
            .filter(|_| !row.example && row.local.is_none())
        else {
            return;
        };
        let refusal = if !row.on {
            Some(tr(
                "It is turned off. Turn it on first: an update never switches a mod on.",
            ))
        } else if row.native() && self.settings.native_mods_allowed_at.is_none() {
            Some(tr(
                "It runs native code. Allow native mods first (Allow… on its row), then update it.",
            ))
        } else {
            None
        };
        if let Some(why) = refusal {
            self.alt.mods.notice = Some(Err(why.to_string()));
            return;
        }
        self.alt_run(
            Action::Update(vec![Included {
                id: row.id.clone(),
                title: row.title.clone(),
                from: row.version.clone(),
                to,
                native: row.native(),
            }]),
            false,
        );
    }

    /// Starts `action` on a worker. `backed_up` is the player's yes to backing up the worlds it
    /// would touch; without it the worker only checks and, if worlds are at stake, stops.
    pub(super) fn alt_run(&mut self, action: Action, backed_up: bool) {
        if !self.alt_guard() {
            return;
        }
        let game = self.mods_game();
        let m = &mut self.alt.mods;
        m.confirm = None;
        m.notice = None;
        m.errors.clear();
        m.busy = Some((tr("Checking your worlds…").to_string(), 0.));
        if matches!(action, Action::Remove(_)) {
            m.selection.clear();
        }
        let (tx, home) = (
            m.tx.clone(),
            m.snapshot
                .as_ref()
                .map(|s| s.home.clone())
                .unwrap_or_else(backend::game_home),
        );
        std::thread::spawn(move || run(action, backed_up, &home, &game, &tx));
    }
}

/// How many folders of `saves/` hold a `level.dat`: each one is a world.
fn world_folders(home: &std::path::Path) -> usize {
    std::fs::read_dir(home.join("saves"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("level.dat").is_file())
        .count()
}

/// Reads the backup back: the zip opens and holds the world's `level.dat`.
fn verify_backup(zip: &std::path::Path, world: &worlds::World) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    let name = format!("{}/level.dat", world.folder);
    let entry = archive
        .by_name(&name)
        .map_err(|_| trf("the backup has no {name}", &[("name", &name)]))?;
    match entry.size() {
        0 => Err(trf("the backup's {name} is empty", &[("name", &name)])),
        _ => Ok(()),
    }
}

fn run(action: Action, backed_up: bool, home: &std::path::Path, game: &str, tx: &Sender<Event>) {
    let refuse = |why: String| {
        let _ = tx.send(Event::Refused(why));
    };
    let _lock = match sunrise_launcher::browser::lock_home(home) {
        Ok(lock) => lock,
        Err(e) => return refuse(e.to_string()),
    };
    // Which saved worlds hold content of these mods.
    let all = worlds::list(home);
    let folders = world_folders(home);
    if folders != all.len() {
        return refuse(trf(
            "{count} of your worlds can't be read, so Sunrise can't tell whether they use these mods. Nothing was changed.",
            &[("count", &folders.saturating_sub(all.len()).to_string())],
        ));
    }
    let mods = action.mods();
    let ids: Vec<String> = mods.iter().map(|(id, _)| id.clone()).collect();
    let at_stake = worlds::saved_with(&all, &ids);
    let mut backups = 0;
    if !at_stake.is_empty() {
        if !backed_up {
            let title = |id: &String| {
                mods.iter()
                    .find(|(m, _)| m == id)
                    .map_or(id.clone(), |(_, t)| t.clone())
            };
            let list = at_stake
                .iter()
                .map(|(w, used)| (w.name.clone(), used.iter().map(title).collect()))
                .collect();
            let _ = tx.send(Event::Stake(action, list));
            return;
        }
        for (i, (world, _)) in at_stake.iter().enumerate() {
            let _ = tx.send(Event::Progress(
                trf("Backing up {world}…", &[("world", &world.name)]),
                i as f32 / at_stake.len() as f32,
            ));
            match worlds::backup(home, world)
                .and_then(|zip| verify_backup(&zip, world).map(|()| zip))
            {
                Ok(_) => backups += 1,
                Err(e) => {
                    return refuse(trf(
                        "Backing up {world} failed, so nothing was changed: {error}",
                        &[("world", &world.name), ("error", &e)],
                    ))
                }
            }
        }
    }
    let backed = match backups {
        0 => String::new(),
        n => trf(
            "Backed up {count} worlds first (see backups/).",
            &[("count", &n.to_string())],
        ),
    };
    let done = |said: String, failed: Vec<(String, String)>| {
        let _ = tx.send(Event::Done(
            format!("{said} {backed}").trim().to_string(),
            failed,
        ));
    };
    match action {
        Action::TurnOff(items) => {
            let ids: Vec<&str> = items.iter().map(|(id, _)| id.as_str()).collect();
            match java::set_all_enabled(home, &ids, false) {
                Ok(()) => done(
                    trf(
                        "Turned off {count} mods. The game reads this when it starts.",
                        &[("count", &ids.len().to_string())],
                    ),
                    Vec::new(),
                ),
                Err(e) => refuse(e),
            }
        }
        Action::Remove(items) => {
            let (mut removed, mut failed) = (0, Vec::new());
            for (i, (id, project, title)) in items.iter().enumerate() {
                let _ = tx.send(Event::Progress(
                    trf("Removing {name}…", &[("name", title)]),
                    i as f32 / items.len() as f32,
                ));
                match installed::remove(home, project) {
                    Ok(_) => removed += 1,
                    Err(e) => failed.push((id.clone(), e)),
                }
            }
            done(
                trf("Removed {count} mods.", &[("count", &removed.to_string())]),
                failed,
            );
        }
        Action::Update(items) => {
            let index = match sunrise::index() {
                Ok(index) => index,
                Err(e) => return refuse(e),
            };
            let (mut updated, mut failed) = (0, Vec::new());
            let count = items.len();
            for (i, item) in items.iter().enumerate() {
                let step = |line: &str, f: f32| {
                    let _ = tx.send(Event::Progress(
                        trf(
                            "{n} of {count}: {line}",
                            &[
                                ("n", &(i + 1).to_string()),
                                ("count", &count.to_string()),
                                ("line", line),
                            ],
                        ),
                        (i as f32 + f) / count as f32,
                    ));
                };
                // Read again now: it may have been switched off since the plan was made.
                if !java::is_enabled(home, &item.id) {
                    failed.push((item.id.clone(), tr("turned off: left as it is").to_string()));
                    continue;
                }
                let result = match item.native {
                    true => sunrise::Registry
                        .project(&item.id)
                        .and_then(|p| sunrise::install(&p, home, game, &step))
                        .map(|_| None),
                    false => auto::update_to(&index, &item.id, &item.to, home, &step),
                };
                match result {
                    Ok(None) => updated += 1,
                    Ok(Some(why)) => failed.push((item.id.clone(), why)),
                    Err(e) => failed.push((item.id.clone(), e)),
                }
            }
            done(
                trf(
                    "Updated {count} mods, each checked against the registry.",
                    &[("count", &updated.to_string())],
                ),
                failed,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use sunrise_launcher_nbt::{self as nbt, Limits, Tag};

    fn home_with_world(mod_id: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let home = std::env::temp_dir().join(format!(
            "alt-mods-actions-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&home);
        let dir = home.join("saves/Home");
        std::fs::create_dir_all(dir.join("region")).unwrap();
        let record = Tag::compound([
            ("ModId", Tag::String(mod_id.into())),
            ("ModVersion", Tag::String("1".into())),
        ]);
        let root = Tag::compound([
            (
                "Data",
                Tag::compound([("LevelName", Tag::String("Home".into()))]),
            ),
            (
                "sunrise",
                Tag::compound([("ContentModList", Tag::List(vec![record]))]),
            ),
        ]);
        let bytes = nbt::write_named(
            &nbt::NamedTag {
                name: "".into(),
                tag: root,
            },
            Limits::default(),
        )
        .unwrap();
        std::fs::write(dir.join("level.dat"), bytes).unwrap();
        std::fs::write(dir.join("region/r.0.0.mca"), [7u8; 64]).unwrap();
        java::set_enabled(&home, mod_id, true).unwrap();
        java::set_enabled(&home, "unrelated", true).unwrap();
        home
    }

    fn off(ids: &[&str]) -> Action {
        Action::TurnOff(
            ids.iter()
                .map(|i| (i.to_string(), i.to_uppercase()))
                .collect(),
        )
    }

    #[test]
    fn turning_off_a_mod_a_world_was_saved_with_stops_and_changes_nothing() {
        let home = home_with_world("bricks");
        let (tx, rx) = channel();
        run(off(&["bricks"]), false, &home, "26.3", &tx);
        match rx.try_recv() {
            Ok(Event::Stake(_, worlds)) => {
                assert_eq!(worlds, [("Home".to_string(), vec!["BRICKS".to_string()])])
            }
            _ => panic!("expected the worlds at stake"),
        }
        assert!(
            java::is_enabled(&home, "bricks"),
            "nothing was switched off"
        );
        assert!(
            !home.join("backups").exists(),
            "no backup before the player agrees"
        );
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn after_the_player_agrees_the_world_is_backed_up_and_checked_before_the_change() {
        let home = home_with_world("bricks");
        let (tx, rx) = channel();
        run(off(&["bricks"]), true, &home, "26.3", &tx);
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(
            matches!(events.last(), Some(Event::Done(..))),
            "it went through"
        );
        let zips: Vec<_> = std::fs::read_dir(home.join("backups"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(zips.len(), 1);
        let world = worlds::list(&home).remove(0);
        verify_backup(&zips[0].path(), &world).unwrap();
        assert!(!java::is_enabled(&home, "bricks"));
        assert!(
            java::is_enabled(&home, "unrelated"),
            "only the chosen mod changed"
        );
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_failed_backup_refuses_and_leaves_the_mod_as_it_was() {
        let home = home_with_world("bricks");
        // The game holds the world: its lock can't be taken, so it can't be backed up.
        let held = std::fs::File::options()
            .write(true)
            .create(true)
            .truncate(false)
            .open(home.join("saves/Home/session.lock"))
            .unwrap();
        held.lock().unwrap();
        let (tx, rx) = channel();
        run(off(&["bricks"]), true, &home, "26.3", &tx);
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(
            matches!(events.last(), Some(Event::Refused(why)) if why.contains("nothing was changed"))
        );
        assert!(java::is_enabled(&home, "bricks"));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_mod_no_world_uses_changes_at_once() {
        let home = home_with_world("bricks");
        let (tx, rx) = channel();
        run(off(&["unrelated"]), false, &home, "26.3", &tx);
        assert!(matches!(rx.try_iter().last(), Some(Event::Done(..))));
        assert!(!java::is_enabled(&home, "unrelated"));
        std::fs::remove_dir_all(home).ok();
    }
}
