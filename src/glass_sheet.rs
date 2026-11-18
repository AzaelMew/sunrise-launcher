//! Instance settings as a dialog centred over the page, drawn as Settings is (the glass look):
//! its icon, "Instance settings", the instance's name and version at the top; Settings' list of
//! sections on the left (General, Installation, Launch, Hooks, Storage) and the open one on the
//! right, under its name and what it is for, its rows in groups; Save and Cancel and the
//! instance's folder along the foot. The page behind is frosted and dimmed, as under Settings.
use super::*;


use new_instance::NewKind;
use servers_page::Field;
use sunrise_launcher::instances::{Channel, Icon, Instance, Plays};


/// The sheet's sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SheetTab {
    #[default]
    General,
    Installation,
    Launch,
    Hooks,
    Storage,
}

impl SheetTab {
    const ALL: [SheetTab; 5] = [
        SheetTab::General,
        SheetTab::Installation,
        SheetTab::Launch,
        SheetTab::Hooks,
        SheetTab::Storage,
    ];

    /// Its name, icon and the line under its name.
    fn about(self) -> (&'static str, &'static str, &'static str) {
        match self {
            SheetTab::General => (
                "General",
                "pencil-edit-02",
                "Name, icon and where it shows in the sidebar.",
            ),
            SheetTab::Installation => (
                "Installation",
                "download-04",
                "The game version this instance runs.",
            ),
            SheetTab::Launch => (
                "Launch",
                "play",
                "How the game starts and how much it may use.",
            ),
            SheetTab::Hooks => (
                "Launch commands",
                "computer-terminal-01",
                "Commands that run before and after the game. Most people never need these.",
            ),
            SheetTab::Storage => (
                "Storage",
                "package-01",
                "Space this instance takes, and ways to take it elsewhere.",
            ),
        }
    }

    /// `SUNRISE_LAUNCHER_SHEET` (screenshots): the section the sheet opens on.
    pub(super) fn from_env() -> Option<SheetTab> {
        let name = std::env::var("SUNRISE_LAUNCHER_SHEET").ok()?;
        SheetTab::ALL
            .into_iter()
            .find(|t| t.about().0.eq_ignore_ascii_case(&name))
    }
}


/// What the dialog would keep, as typed or picked.
struct Draft {
    name: String,
    group: String,
    icon: Option<Icon>,
    /// A picture chosen, or the saved one removed.
    picture: bool,
    /// Sunrise's version and builds, for an instance that plays Sunrise.
    sunrise: Option<(Option<String>, Channel)>,
    plays: Plays,
    /// A Java instance's arguments and window size.
    java: (String, (String, String)),
    /// The hook fields (before, wrapper, after, variables), once the Hooks section was opened.
    hooks: Option<[String; 4]>,
    /// The Linux switches, once one was flipped.
    switches: Option<[bool; 3]>,
    /// A Java instance's maximum memory, once changed; Err while the box holds no size.
    memory: Result<Option<u32>, ()>,
}

impl Draft {
    /// The sections whose fields Save would change in `saved`.
    fn changed(&self, saved: &Instance) -> Vec<SheetTab> {
        let hooks = [
            saved.launch.pre_launch.clone(),
            saved.launch.wrapper.clone(),
            saved.launch.post_exit.clone(),
            sunrise_launcher::launch::env_line(&saved.launch.env),
        ];
        let general = self.name != saved.name
            || self.group != saved.group.clone().unwrap_or_default()
            || self.icon != saved.icon
            || self.picture;
        let installation = self.plays != Plays::of(saved)
            || self
                .sunrise
                .as_ref()
                .is_some_and(|(pin, channel)| *pin != saved.version || *channel != saved.channel);
        let launch = saved.java.as_ref().is_some_and(|j| {
            self.java
                != (
                    j.jvm_args.trim().to_string(),
                    j.window
                        .map(|(w, h)| (w.to_string(), h.to_string()))
                        .unwrap_or_default(),
                )
                || match self.memory {
                    Ok(memory) => memory.is_some_and(|mb| {
                        mb != j.memory_mb.unwrap_or(instances::DEFAULT_MEMORY_MB)
                    }),
                    // Text that isn't a size is still an edit, kept until it is fixed.
                    Err(()) => true,
                }
        });
        let l = &saved.launch;
        let hooks = self.hooks.as_ref().is_some_and(|typed| *typed != hooks)
            || self
                .switches
                .is_some_and(|s| s != [l.gamemode, l.mangohud, l.dedicated_gpu]);
        [
            (general, SheetTab::General),
            (installation, SheetTab::Installation),
            (launch, SheetTab::Launch),
            (hooks, SheetTab::Hooks),
        ]
        .into_iter()
        .filter_map(|(changed, tab)| changed.then_some(tab))
        .collect()
    }
}

impl Launcher {
    /// Whether the sheet holds edits its Save has not kept.
    pub(super) fn sheet_changed(&self) -> bool {
        !self.sheet_changes().is_empty()
    }

    /// The sections holding edits Save has not kept.
    pub(super) fn sheet_changes(&self) -> Vec<SheetTab> {
        let Some(d) = self.instances.dialog.as_ref() else {
            return Vec::new();
        };
        let Some(saved) = d
            .editing
            .as_ref()
            .and_then(|id| self.instances.list.iter().find(|i| &i.id == id))
        else {
            return Vec::new();
        };
        let draft = Draft {
            name: self.servers.value(Field::InstanceName).trim().to_string(),
            group: self.servers.value(Field::InstanceGroup).to_string(),
            icon: d.icon.clone(),
            picture: match d.picture {
                Some(Some(_)) => true,
                Some(None) => saved.home.join(instances::PICTURE).is_file(),
                None => false,
            },
            sunrise: (d.java.kind == NewKind::Sunrise).then(|| (d.pin.clone(), d.channel)),
            plays: self.draft_plays(saved).unwrap_or_else(|| Plays::of(saved)),
            java: (
                self.servers.value(Field::JavaArgs).trim().to_string(),
                (
                    self.servers.value(Field::WindowWidth).trim().to_string(),
                    self.servers.value(Field::WindowHeight).trim().to_string(),
                ),
            ),
            hooks: self.hooks_typed(&saved.id),
            switches: self.switches_typed(&saved.id),
            memory: self.sheet_memory(),
        };
        draft.changed(saved)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use sunrise_launcher::instances::Kind as InstanceKind;

    fn saved() -> Instance {
        Instance {
            id: "mine".into(),
            name: "Mine".into(),
            kind: InstanceKind::Own,
            work: Default::default(),
            home: Default::default(),
            icon: None,
            group: None,
            version: None,
            channel: Channel::Stable,
            native_profile: Default::default(),
            played: Default::default(),
            last_played: None,
            launch: Default::default(),
            java: None,
        }
    }

    fn untouched(saved: &Instance) -> Draft {
        Draft {
            name: saved.name.clone(),
            group: String::new(),
            icon: None,
            picture: false,
            sunrise: Some((None, Channel::Stable)),
            plays: Plays::Sunrise,
            java: (String::new(), (String::new(), String::new())),
            hooks: None,
            switches: None,
            memory: Ok(None),
        }
    }

    #[test]
    fn invalid_window_text_still_counts_as_an_unsaved_edit() {
        let mut saved = saved();
        saved.java = Some(instances::JavaGame::default());
        let mut draft = untouched(&saved);
        draft.plays = Plays::of(&saved);
        assert!(draft.changed(&saved).is_empty());
        for (width, height) in [
            ("bad", ""),
            ("0", "720"),
            ("1280", ""),
            ("4294967296", "720"),
        ] {
            draft.java.1 = (width.into(), height.into());
            assert_eq!(draft.changed(&saved), vec![SheetTab::Launch]);
        }
        saved.java.as_mut().unwrap().window = Some((1280, 720));
        draft.java.1 = ("1280".into(), "720".into());
        assert!(draft.changed(&saved).is_empty());
        draft.java.1 .0 = "oops".into();
        assert_eq!(draft.changed(&saved), vec![SheetTab::Launch]);
        // Memory set in the sheet waits for Save too; set back to what is saved, it is no edit.
        draft.java.1 = ("1280".into(), "720".into());
        draft.memory = Ok(Some(instances::DEFAULT_MEMORY_MB));
        assert!(draft.changed(&saved).is_empty());
        draft.memory = Ok(Some(instances::DEFAULT_MEMORY_MB * 2));
        assert_eq!(draft.changed(&saved), vec![SheetTab::Launch]);
        // Text in the box that isn't a size is an edit too, until it is fixed.
        draft.memory = Err(());
        assert_eq!(draft.changed(&saved), vec![SheetTab::Launch]);
    }

    #[test]
    fn save_waits_for_a_change_in_any_section() {
        let saved = saved();
        assert!(untouched(&saved).changed(&saved).is_empty());
        let general = SheetTab::General;
        let installation = SheetTab::Installation;
        type Edit = (fn(&mut Draft), SheetTab);
        let edits: [Edit; 8] = [
            (|d| d.name = "Renamed".into(), general),
            (|d| d.group = "Friends".into(), general),
            (|d| d.icon = Some(Icon::Colour("#6b4bff".into())), general),
            (|d| d.picture = true, general),
            (
                |d| d.sunrise = Some((Some("nightly-2026-11-13-aaaaaaa".into()), Channel::Stable)),
                installation,
            ),
            (|d| d.sunrise = Some((None, Channel::Nightly)), installation),
            // The Hooks section was opened and a command typed.
            (
                |d| {
                    d.hooks = Some([
                        "./backup.sh".into(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ])
                },
                SheetTab::Hooks,
            ),
            // A Linux switch flipped waits for Save like a typed command.
            (|d| d.switches = Some([true, false, false]), SheetTab::Hooks),
        ];
        // Each edit marks its own section, and only that one.
        for (edit, section) in edits {
            let mut draft = untouched(&saved);
            edit(&mut draft);
            assert_eq!(draft.changed(&saved), [section]);
        }
        // Hooks opened and left as they were is no edit.
        let mut draft = untouched(&saved);
        draft.hooks = Some(Default::default());
        assert!(draft.changed(&saved).is_empty());
        // A switch flipped and flipped back is no edit either.
        draft.switches = Some([false, false, false]);
        assert!(draft.changed(&saved).is_empty());
    }
}
