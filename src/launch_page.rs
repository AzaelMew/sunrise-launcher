//! Launch… on Home's Instance row: what runs around the selected instance's game
//! (`sunrise_launcher::launch`): a command before it, a wrapper, and environment variables.
use super::servers_page::Field;
use super::*;

use sunrise_launcher::launch;

pub(super) struct LaunchPanel {
    /// The instance it is for: picking another closes it.
    instance: String,
    /// What Save did: Ok with the summary of what runs (None when nothing runs), or Err with reason.
    saved: Option<Result<Option<String>, String>>,
    /// The Linux switches (GameMode, MangoHud, dedicated card) as flipped, kept with Save like
    /// the commands: None until one is flipped.
    switches: Option<[bool; 3]>,
}

/// What saving the memory slider did, kept to show under the slider in the active language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MemorySaved {
    /// Saved Java heap size in MB.
    Ok(u32),
    /// Saving failed on disk.
    Error(String),
}

/// The memory row on Home. What it shows comes from the selected instance, except during a drag.
#[derive(Default)]
pub(super) struct MemorySlider {
    /// What the last change did, or why not.
    saved: Option<MemorySaved>,
    /// The instance whose value is being typed: saved to it when the box loses the keyboard, even
    /// if that's because another instance was picked.
    typing: Option<String>,
    /// The instance `saved` is about.
    shown: Option<String>,
}

impl MemorySlider {
    /// A new sheet starts from the saved value: nothing left typed in the box carries over.
    pub(super) fn forget_typing(&mut self) {
        self.typing = None;
        self.shown = None;
    }
}

impl Launcher {
    /// What the hook fields hold now, when the panel is open for `id`: the commands before and
    /// after the game, the wrapper and the variables, as typed.
    pub(super) fn hooks_typed(&self, id: &str) -> Option<[String; 4]> {
        self.launch.as_ref().filter(|p| p.instance == id)?;
        let typed = |f: Field| self.servers.value(f).trim().to_string();
        Some([
            typed(Field::PreLaunch),
            typed(Field::Wrapper),
            typed(Field::PostExit),
            typed(Field::LaunchEnv),
        ])
    }

    /// The Linux switches as flipped in the open panel for `id`, when one was.
    pub(super) fn switches_typed(&self, id: &str) -> Option<[bool; 3]> {
        self.launch.as_ref().filter(|p| p.instance == id)?.switches
    }

    /// Parse the open command fields (and the flipped switches) without writing any instance
    /// settings.
    pub(super) fn parsed_launch(&self, id: &str) -> Option<Result<launch::Launch, String>> {
        let fields = self.hooks_typed(id)?;
        let saved = self.instances.list.iter().find(|i| i.id == id)?;
        let switches = self.switches_typed(id);
        Some(
            parse_commands(saved.launch.clone(), fields).map(|mut launch| {
                if let Some([gamemode, mangohud, gpu]) = switches {
                    launch.gamemode = gamemode;
                    launch.mangohud = mangohud;
                    launch.dedicated_gpu = gpu;
                }
                launch
            }),
        )
    }

    pub(super) fn save_launch(&mut self) {
        let Some(id) = self.launch.as_ref().map(|p| p.instance.clone()) else {
            return;
        };
        let Some(parsed) = self.parsed_launch(&id) else {
            return;
        };
        let saved = parsed.and_then(|launch| instances::edit(&id, |m| m.launch = launch));
        let saved = saved.map(|i| {
            self.reload_instances();
            i.launch.summary()
        });
        if let Some(p) = &mut self.launch {
            p.saved = Some(saved);
        }
    }

    fn save_memory(&mut self, id: &str, memory: u32) {
        // In instance settings it is part of the draft: Save keeps it, Cancel drops it.
        let saved_mb = self
            .instances
            .list
            .iter()
            .find(|i| i.id == id)
            .and_then(Self::saved_memory);
        if let Some(d) =
            (self.instances.dialog.as_mut()).filter(|d| d.editing.as_deref() == Some(id))
        {
            d.memory = (saved_mb != Some(memory)).then_some(memory);
            self.memory_slider.saved = None;
            return;
        }
        let saved = instances::edit(id, |m| {
            if let Some(java) = &mut m.java {
                java.memory_mb = Some(memory);
            }
        });
        self.memory_slider.saved = Some(match saved {
            Ok(_) => MemorySaved::Ok(memory),
            Err(e) => MemorySaved::Error(e),
        });
        self.reload_instances();
    }

    /// The memory instance settings would save for the instance it edits: the box's text while
    /// the row shows that instance (typed or not), else the slider's draft. Err: the box holds
    /// something that isn't a size. Ok(None): nothing set in the sheet.
    pub(super) fn sheet_memory(&self) -> Result<Option<u32>, ()> {
        let Some(d) = self.instances.dialog.as_ref() else {
            return Ok(None);
        };
        let Some(id) = d.editing.as_deref() else {
            return Ok(None);
        };
        if self.memory_slider.shown.as_deref() != Some(id) {
            return Ok(d.memory);
        }
        launch::parse_memory(self.servers.value(Field::MemoryLimit))
            .map(|typed| Some(typed.unwrap_or(instances::DEFAULT_MEMORY_MB)))
            .map_err(|_| ())
    }

    /// The value the row shows: the sheet's draft for the instance it edits, else the saved one.
    fn shown_memory(&self, instance: &instances::Instance) -> Option<u32> {
        (self.instances.dialog.as_ref())
            .filter(|d| d.editing.as_deref() == Some(instance.id.as_str()))
            .and_then(|d| d.memory)
            .or_else(|| Self::saved_memory(instance))
    }

    /// The instance's saved value.
    pub(super) fn saved_memory(instance: &instances::Instance) -> Option<u32> {
        instance
            .java
            .as_ref()
            .map(|java| java.memory_mb.unwrap_or(instances::DEFAULT_MEMORY_MB))
    }

    /// Saves what's in the box to `id`, if it's a value and a new one.
    fn save_typed_memory(&mut self, id: &str) {
        let typed = self.servers.value(Field::MemoryLimit).to_owned();
        let Ok(memory) = launch::parse_memory(&typed) else {
            self.memory_slider.saved = None;
            self.servers.set_field_invalid(Field::MemoryLimit, true);
            return;
        };
        let memory = memory.unwrap_or(instances::DEFAULT_MEMORY_MB);
        self.servers.set_field_invalid(Field::MemoryLimit, false);
        let same = instances::list()
            .iter()
            .find(|i| i.id == id)
            .is_some_and(|i| self.shown_memory(i) == Some(memory));
        if !same {
            self.save_memory(id, memory);
        }
    }

    /// Enter in the box.
    pub(super) fn memory_enter(&mut self) {
        if let Some(id) = self.memory_slider.typing.clone() {
            self.save_typed_memory(&id);
        }
    }
}

fn parse_commands(
    mut launch: launch::Launch,
    [pre, wrapper, post, env]: [String; 4],
) -> Result<launch::Launch, String> {
    launch.env = launch::parse_env(&env)?;
    launch.pre_launch = pre;
    launch.wrapper = wrapper;
    launch.post_exit = post;
    Ok(launch)
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn parsing_commands_is_an_unsaved_draft() {
        let saved = launch::Launch::default();
        let fields = [
            "backup".into(),
            "wrapper".into(),
            "after".into(),
            "MODE=play".into(),
        ];
        let draft = parse_commands(saved.clone(), fields).unwrap();
        assert!(saved.pre_launch.is_empty());
        assert!(saved.env.is_empty());
        assert_eq!(draft.pre_launch, "backup");
        assert_eq!(draft.wrapper, "wrapper");
        assert_eq!(draft.post_exit, "after");
        assert_eq!(draft.env.get("MODE").unwrap(), "play");
        assert!(parse_commands(
            saved,
            [
                String::new(),
                String::new(),
                String::new(),
                "not an assignment".into()
            ]
        )
        .is_err());
    }
}
