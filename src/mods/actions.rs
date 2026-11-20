//! Everything on the Mods screens that changes files. Each action is refused while the game or
//! a launch step is running. The launcher makes no backups of worlds itself: the game's own
//! Edit World › Make Backup is the way to keep one.
use super::model::{self, Included, Row};
use super::*;
use sunrise_launcher::browser::sunrise::auto;

#[derive(Clone)]
pub(super) enum Action {
    /// (mod id, title)
    TurnOff(Vec<(String, String)>),
    /// (mod id, project id, title)
    Remove(Vec<(String, String, String)>),
    /// The updates, and the mods the player answered the world question for.
    Update(Vec<Included>, std::collections::BTreeSet<String>),
}

/// Why mods can't change now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lock {
    /// The game is running or starting.
    GameRunning,
    /// Sunrise is getting ready to play.
    Preparing,
    /// The mods are being brought up to date (at start, or for the Play that waits for it).
    Updating,
    /// A native build is running.
    Building,
}

impl Lock {
    /// The first reason that applies, the most pressing first.
    fn of(game_running: bool, preparing: bool, updating: bool, building: bool) -> Option<Lock> {
        if game_running {
            Some(Lock::GameRunning)
        } else if preparing {
            Some(Lock::Preparing)
        } else if updating {
            Some(Lock::Updating)
        } else if building {
            Some(Lock::Building)
        } else {
            None
        }
    }

    fn why(self) -> &'static str {
        match self {
            Lock::GameRunning => {
                tr("The game is running or starting. Close it before changing mods.")
            }
            Lock::Preparing => {
                tr("Sunrise is getting ready to play. Wait for it to finish before changing mods.")
            }
            Lock::Updating => {
                tr("Your mods are being updated. Wait for it to finish before changing mods.")
            }
            Lock::Building => {
                tr("A native build is running. Wait for it to finish before changing mods.")
            }
        }
    }
}

impl Launcher {
    /// Opens a confirmation, remembering which folder it was reviewed for.
    fn confirm(&mut self, confirm: Confirm) {
        let home = self.ui.mods.snapshot.as_ref().map(|s| s.home.clone());
        self.ui.mods.reviewed_home = home;
        self.ui.mods.confirm = Some(confirm);
    }

    /// Whether the selected profile's game runs or is starting.
    pub(crate) fn game_running(&self) -> bool {
        self.client.is_some() || self.starting_client.is_some() || self.starting_play.is_some()
    }

    /// Why nothing may change now: the game, a launch step or a build is under way.
    pub(crate) fn mods_lock(&self) -> Option<Lock> {
        Lock::of(
            self.game_running(),
            self.preparing
                || self.installing.is_some()
                || self.play_when_prepared.is_some()
                || self.play_after_build.is_some(),
            // The updater of Play, or of the start: an install now would race it.
            self.mods.updating || self.play_when_updated.is_some(),
            self.native_step.is_some(),
        )
    }

    /// [`Self::mods_lock`] in words.
    pub(crate) fn mods_locked(&self) -> Option<&'static str> {
        self.mods_lock().map(Lock::why)
    }

    /// The gate every change passes: not locked, nothing else running.
    pub(super) fn guard(&mut self) -> bool {
        let refusal = if let Some(why) = self.mods_locked() {
            Some(why.to_string())
        } else if self.mutating() {
            Some(tr("Wait for the current operation to finish.").to_string())
        } else {
            None
        };
        if let Some(why) = refusal {
            self.ui.mods.notice = Some(Err(why));
            return false;
        }
        true
    }

    /// Add `id` to the game: the existing install path, which asks first for native code, checks
    /// the file against the index and its signature, and refuses what was taken down. Only a mod
    /// the registry says fits this build. `shown` is the name the page or card that asks shows.
    pub(crate) fn install_registry_mod(&mut self, id: String, shown: String) -> Result<(), String> {
        let fit = match &self.ui.mods.catalog {
            Catalog::Ready(cards, _) => cards.iter().find(|c| c.id == id).map(|c| c.fit.clone()),
            _ => None,
        };
        let Some(fit) = fit else {
            return Err(tr("The registry hasn't answered about this mod yet.").to_string());
        };
        if let Some(why) = fit.reason() {
            return Err(why);
        }
        // The card says the same before the click (`InstallState::CloseGame`).
        if self.game_running() {
            return Err(tr("Close the game to install").to_string());
        }
        // A build or another install doesn't refuse: this one waits its turn.
        if let Some(lock) = self.mods_lock().filter(|lock| *lock != Lock::Building) {
            return Err(lock.why().to_string());
        }
        if self.ui.mods.busy.is_some() {
            return Err(tr("Wait for the current operation to finish.").to_string());
        }
        self.ui.mods.notice = None;
        self.mods.message = None;
        self.mods_install(SourceId::Sunrise, id, None, Some(shown));
        Ok(())
    }

    /// Turn on the registry mod `id` the card says is installed but off: the same switch the
    /// Installed list's, which asks the one native question first when it must.
    pub(crate) fn turn_on_registry_mod(&mut self, id: &str) {
        // The same gate as every change: not while the game runs, or another change is under way.
        if !self.guard() {
            return;
        }
        let entry = self
            .ui
            .mods
            .snapshot
            .as_ref()
            .and_then(|s| s.entries.iter().find(|e| e.project == id))
            .cloned();
        if let Some(entry) = entry {
            self.mods.message = None;
            self.mods_switch(&entry);
            if let Some(Err(e)) = self.mods.message.take() {
                self.ui.mods.notice = Some(Err(e));
            }
        }
    }

    pub(crate) fn add(&mut self, id: String, shown: String) {
        if let Err(why) = self.install_registry_mod(id, shown) {
            self.ui.mods.notice = Some(Err(why));
        }
    }

    /// Turn one mod on, or check its worlds before turning it off. For files installed
    /// outside the registry, switching on records the choice; the native consent and build
    /// gates still decide whether the game can use it.
    pub(super) fn toggle(&mut self, row: &Row) {
        if !self.guard() {
            return;
        }
        if row.on {
            self.run(Action::TurnOff(vec![(row.id.clone(), row.title.clone())]));
        } else if let Some(entry) = &row.entry {
            self.mods.message = None;
            self.mods_switch(entry);
            if let Some(Err(e)) = self.mods.message.take() {
                self.ui.mods.notice = Some(Err(e));
            }
        } else if row.native() && self.settings.native_mods_allowed_at.is_none() {
            // A hand-copied native mod asks the same one question a registry one does.
            self.pending_consent = Some(PendingConsent::TurnOn(row.id.clone(), row.title.clone()));
        } else {
            self.turn_on_local(&row.id);
        }
    }

    /// Turns on a mod that isn't from the registry.
    pub(crate) fn turn_on_local(&mut self, id: &str) {
        let Some(snapshot) = &mut self.ui.mods.snapshot else {
            return;
        };
        let Some(_lock) = sunrise_launcher::browser::try_lock_home(&snapshot.home) else {
            self.ui.mods.notice = Some(Err(tr("Wait for the current operation to finish.").into()));
            return;
        };
        self.ui.mods.notice = match mod_list::set_enabled(&snapshot.home, id, true) {
            Ok(()) => {
                snapshot.key.clear();
                None
            }
            Err(e) => Some(Err(e)),
        };
    }

    /// Turn the chosen registry mods on in one rewrite of the list. Local and GitHub mods
    /// have individual switches only: they aren't selectable for registry bulk actions.
    /// A mod that can't be turned on stays as it is and its row says why.
    pub(super) fn bulk_on(&mut self) {
        if !self.guard() {
            return;
        }
        let rows = self.rows();
        let chosen: Vec<&Row> = rows
            .iter()
            .filter(|r| r.local.is_none() && self.ui.mods.selection.contains(&r.id))
            .collect();
        let (go, held) =
            model::turn_on_plan(&chosen, self.settings.native_mods_allowed_at.is_some());
        let m = &mut self.ui.mods;
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
        m.notice = Some(match mod_list::set_all_enabled(&home, &ids, true) {
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

    pub(super) fn bulk_off(&mut self) {
        let rows = self.rows();
        let chosen: Vec<(String, String)> = rows
            .iter()
            .filter(|r| r.local.is_none() && r.on && self.ui.mods.selection.contains(&r.id))
            .map(|r| (r.id.clone(), r.title.clone()))
            .collect();
        if chosen.is_empty() {
            self.ui.mods.notice = Some(Err(
                tr("None of the selected mods is turned on.").to_string()
            ));
            return;
        }
        self.run(Action::TurnOff(chosen));
    }

    /// Asks for confirmation before removing the chosen mods.
    pub(super) fn ask_remove(&mut self) {
        let rows = self.rows();
        let chosen: Vec<(String, String, String)> = rows
            .iter()
            .filter(|r| r.local.is_none() && self.ui.mods.selection.contains(&r.id))
            .filter_map(|r| {
                Some((
                    r.id.clone(),
                    r.entry.as_ref()?.project.clone(),
                    r.title.clone(),
                ))
            })
            .collect();
        if !chosen.is_empty() {
            self.confirm(Confirm::Remove(chosen));
        }
    }

    /// Plans "Update N mods" for the whole pack, or for the selected rows only.
    pub(super) fn ask_update(&mut self, only_selected: bool) {
        let rows: Vec<Row> = self
            .rows()
            .into_iter()
            .filter(|r| !only_selected || self.ui.mods.selection.contains(&r.id))
            .collect();
        let plan = model::plan_updates(&rows, self.settings.native_mods_allowed_at.is_some());
        self.confirm(Confirm::Update(plan));
    }

    /// One mod's update: the same reviewed path as the batch, for one mod.
    pub(super) fn update_one(&mut self, row: &Row) {
        let Some(to) = row.update.clone().filter(|_| row.local.is_none()) else {
            return;
        };
        let refusal = if row.native() && self.settings.native_mods_allowed_at.is_none() {
            Some(tr(
                "It runs native code. Turn on a native mod to be asked about native code first, then update it.",
            ))
        } else {
            None
        };
        if let Some(why) = refusal {
            self.ui.mods.notice = Some(Err(why.to_string()));
            return;
        }
        self.run(Action::Update(
            vec![Included {
                id: row.id.clone(),
                title: row.title.clone(),
                from: row.version.clone(),
                to,
                native: row.native(),
            }],
            Default::default(),
        ));
    }

    /// The world question for updates that were held back, by what each would drop; Stay leaves
    /// them as they are, Switch anyway runs them again with those mods allowed.
    pub(super) fn ask_worlds_before_update(&mut self, items: Vec<Included>) {
        let Some(instance) = self.selected_instance() else {
            return;
        };
        let (id, home) = (instance.id.clone(), self.mods_home());
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        match sunrise_launcher::worlds::question(&home, &ids) {
            Ok(Some(question)) => {
                self.mods.world_ask = Some(crate::mods_page::WorldAsk {
                    instance: id,
                    question,
                    then: crate::mods_page::Then::UpdateChosen(items),
                });
            }
            // No world uses them after all: nothing stops the update.
            Ok(None) => self.update_allowing(items, ids.into_iter().collect()),
            Err(e) => self.ui.mods.notice = Some(Err(e)),
        }
    }

    /// The updates again, now that the world question was answered for `allow`.
    pub(crate) fn update_allowing(
        &mut self,
        items: Vec<Included>,
        allow: std::collections::BTreeSet<String>,
    ) {
        self.run(Action::Update(items, allow));
    }

    /// Starts `action` on a worker.
    pub(super) fn run(&mut self, action: Action) {
        if !self.guard() {
            return;
        }
        let game = self.mods_game();
        let m = &mut self.ui.mods;
        let current = m
            .snapshot
            .as_ref()
            .map(|s| s.home.clone())
            .unwrap_or_else(backend::game_home);
        if !reviewed_here(m.confirm.is_some(), &m.reviewed_home, &current) {
            m.confirm = None;
            m.selection.clear();
            m.notice = Some(Err(tr(
                "The profile changed since you reviewed this. Nothing was changed; choose again.",
            )
            .to_string()));
            return;
        }
        m.confirm = None;
        m.notice = None;
        m.errors.clear();
        m.busy = Some((tr("Working…").to_string(), 0.));
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
        std::thread::spawn(move || run(action, &home, &game, &tx));
    }
}

/// Whether a confirmation (if one is open) was made for the folder the change would go into.
fn reviewed_here(confirming: bool, reviewed: &Option<PathBuf>, current: &std::path::Path) -> bool {
    !confirming || reviewed.as_deref() == Some(current)
}

/// `one` for a count of 1, else `many` with its `{count}` filled in.
fn counted(count: usize, one: &'static str, many: &'static str) -> String {
    match count {
        1 => tr(one).to_string(),
        n => trf(many, &[("count", &n.to_string())]),
    }
}

fn run(action: Action, home: &std::path::Path, game: &str, tx: &Sender<Event>) {
    let refuse = |why: String| {
        let _ = tx.send(Event::Refused(why));
    };
    let _lock = match sunrise_launcher::browser::lock_home(home) {
        Ok(lock) => lock,
        Err(e) => return refuse(e.to_string()),
    };
    let done = |said: String, failed: Vec<(String, String)>| {
        let _ = tx.send(Event::Done(said, failed));
    };
    match action {
        Action::TurnOff(items) => {
            let ids: Vec<&str> = items.iter().map(|(id, _)| id.as_str()).collect();
            match mod_list::set_all_enabled(home, &ids, false) {
                Ok(()) => done(
                    counted(
                        ids.len(),
                        "Turned off 1 mod. The game reads this when it starts.",
                        "Turned off {count} mods. The game reads this when it starts.",
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
                counted(removed, "Removed 1 mod.", "Removed {count} mods."),
                failed,
            );
        }
        Action::Update(items, allow) => {
            let index = match sunrise::index() {
                Ok(index) => index,
                Err(e) => return refuse(e),
            };
            let (mut updated, mut failed, mut asked) = (0, Vec::new(), Vec::new());
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
                // Read now, not at planning: a mod that is off stays off.
                let was_on = mod_list::is_enabled(home, &item.id);
                let result = match item.native {
                    true => sunrise::Registry
                        .project(&item.id)
                        .and_then(|p| sunrise::update_native(&p, home, game, was_on, &step))
                        .map(|_| None),
                    false => auto::update_to(&index, &item.id, &item.to, home, &allow, &step),
                };
                match result {
                    Ok(None) => updated += 1,
                    // A world may use what this version drops: the question comes, not a failure.
                    Ok(Some(_)) => asked.push(item.clone()),
                    Err(e) => failed.push((item.id.clone(), e)),
                }
            }
            if !asked.is_empty() {
                let _ = tx.send(Event::Asked(asked));
            }
            done(
                counted(
                    updated,
                    "Updated 1 mod, checked against the registry.",
                    "Updated {count} mods, each checked against the registry.",
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
        mod_list::set_enabled(&home, mod_id, true).unwrap();
        mod_list::set_enabled(&home, "unrelated", true).unwrap();
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
    fn turning_off_a_mod_a_world_was_saved_with_is_just_the_switch() {
        let home = home_with_world("bricks");
        let (tx, rx) = channel();
        run(off(&["bricks"]), &home, "26.3", &tx);
        assert!(matches!(rx.try_iter().last(), Some(Event::Done(..))));
        assert!(!mod_list::is_enabled(&home, "bricks"));
        assert!(mod_list::is_enabled(&home, "unrelated"));
        assert!(!home.join("backups").exists(), "no backup, no prompt");
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn old_backups_are_left_exactly_where_they_are() {
        let home = home_with_world("bricks");
        let old = home.join("backups");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("2026-01-01_00-00-00_Home.zip"), b"older backup").unwrap();
        let (tx, rx) = channel();
        run(off(&["bricks"]), &home, "26.3", &tx);
        run(
            Action::Remove(vec![("bricks".into(), "bricks".into(), "BRICKS".into())]),
            &home,
            "26.3",
            &tx,
        );
        assert!(rx
            .try_iter()
            .any(|e| matches!(e, Event::Done(..) | Event::Refused(_))));
        let kept: Vec<_> = std::fs::read_dir(&old)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(kept, ["2026-01-01_00-00-00_Home.zip"]);
        assert_eq!(std::fs::read(old.join(&kept[0])).unwrap(), b"older backup");
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn counts_of_mods_say_mod_once_and_mods_otherwise() {
        assert_eq!(
            counted(1, "Removed 1 mod.", "Removed {count} mods."),
            "Removed 1 mod."
        );
        assert_eq!(
            counted(3, "Removed 1 mod.", "Removed {count} mods."),
            "Removed 3 mods."
        );
        assert_eq!(
            counted(0, "Removed 1 mod.", "Removed {count} mods."),
            "Removed 0 mods."
        );
    }

    #[test]
    fn a_mod_no_world_uses_changes_at_once() {
        let home = home_with_world("bricks");
        let (tx, rx) = channel();
        run(off(&["unrelated"]), &home, "26.3", &tx);
        assert!(matches!(rx.try_iter().last(), Some(Event::Done(..))));
        assert!(!mod_list::is_enabled(&home, "unrelated"));
        std::fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_review_made_for_one_setup_cannot_change_another() {
        let a = PathBuf::from("/a/home");
        let b = PathBuf::from("/b/home");
        assert!(
            reviewed_here(false, &None, &a),
            "no confirmation open: nothing to match"
        );
        assert!(reviewed_here(true, &Some(a.clone()), &a));
        assert!(!reviewed_here(true, &Some(a.clone()), &b));
        assert!(!reviewed_here(true, &None, &a));
    }
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    #[test]
    fn the_updater_of_play_and_of_the_start_holds_installs_like_a_build_does() {
        assert_eq!(Lock::of(false, false, false, false), None);
        assert_eq!(Lock::of(false, false, true, false), Some(Lock::Updating));
        assert_eq!(
            Lock::of(false, false, true, true),
            Some(Lock::Updating),
            "an install doesn't start under the updater even while a build runs"
        );
        assert_eq!(Lock::of(false, true, true, false), Some(Lock::Preparing));
        assert_eq!(Lock::of(true, true, true, true), Some(Lock::GameRunning));
        assert_eq!(Lock::of(false, false, false, true), Some(Lock::Building));
    }
}
