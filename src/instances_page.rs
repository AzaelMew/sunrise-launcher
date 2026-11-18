//! Instances in the window (`sunrise_launcher::instances`): the sidebar's list, by group, and the
//! dialog that makes one (empty, or a copy of the selected one) or changes its name, group and
//! icon.
use super::*;
use gpui::RenderImage;
use servers_page::Field;
use std::collections::HashMap;
use std::sync::Arc;
use sunrise_launcher::instances::{self, Icon, Instance, Kind as InstanceKind};

/// The dialog: a new instance (`editing` None) or one being changed. Its name and group are the
/// text fields `Field::InstanceName` and `Field::InstanceGroup`.
pub(super) struct InstanceDialog {
    pub(super) editing: Option<String>,
    /// A new instance copies the selected one's worlds, options and packs.
    pub(super) copy: bool,
    pub(super) icon: Option<Icon>,
    /// A picture chosen since it opened (None in it: the instance's is removed on Save).
    pub(super) picture: Option<Option<instance_icons::Chosen>>,
    pub(super) error: Option<String>,
    pub(super) busy: bool,
    /// Sunrise, or the Java game it plays.
    pub(super) java: new_instance::JavaChoice,
    /// The Sunrise version it keeps (`Instance::version`); None follows the launcher's.
    pub(super) pin: Option<String>,
    /// The builds "latest" follows (`Instance::channel`); a new instance starts on Stable.
    pub(super) channel: instances::Channel,
    /// The sheet was closed with edits not saved: asking whether to drop them.
    pub(super) discarding: bool,
    /// A Java instance's maximum memory as set in the sheet, kept with Save: None until changed.
    pub(super) memory: Option<u32>,
}

/// The instances and their icons as images, kept between frames.
#[derive(Default)]
pub(super) struct Instances {
    pub(super) list: Vec<Instance>,
    pub(super) dialog: Option<InstanceDialog>,
    /// Textures, pictures and marks as images, by name and size in device pixels.
    pub(super) icons: HashMap<(String, u32), Option<Arc<RenderImage>>>,
    /// Mojang's versions, for a new Java instance.
    pub(super) versions: new_instance::VersionList,
}

impl Launcher {
    pub(super) fn reload_instances(&mut self) {
        self.instances.list = instances::list();
        // A picture may have come or gone with the change.
        self.instances
            .icons
            .retain(|(key, _), _| !key.starts_with("picture:"));
    }

    /// The instance Play starts.
    pub(super) fn selected_instance(&self) -> Option<&Instance> {
        let id = self
            .settings
            .instance
            .as_deref()
            .unwrap_or(instances::DEFAULT);
        let list = &self.instances.list;
        list.iter().find(|i| i.id == id).or(list.first())
    }

    /// The installed version `instance` plays: its own, else the launcher's.
    pub(super) fn version_of(&self, instance: &Instance) -> Option<&String> {
        match &instance.version {
            Some(own) => self.installed.iter().find(|t| *t == own),
            // Latest of its channel: the newest installed Stable build, or the launcher's
            // version (which follows the newest Nightly; also what is left offline).
            None => match instance.channel {
                instances::Channel::Stable => self
                    .stable_builds()
                    .iter()
                    .find_map(|b| self.installed.iter().find(|t| **t == b.tag))
                    .or_else(|| self.launcher_version()),
                instances::Channel::Nightly => self.launcher_version(),
            },
        }
    }

    /// The Minecraft version `instance`'s own build plays: what its mods and packs are for.
    pub(super) fn game_of(&self, instance: &Instance) -> String {
        backend::played_version_id_by(self.version_of(instance).map(String::as_str))
    }

    /// The Minecraft version the Mods page installs and updates for: the selected instance's build, else the launcher's.
    pub(super) fn mods_game(&self) -> String {
        self.selected_instance()
            .map_or_else(sunrise_launcher::browser::game, |i| self.game_of(i))
    }

    pub(super) fn select_instance(&mut self, instance: &Instance) {
        // A native build's warning names the mods of the instance it was opened for.
        self.pending_consent = None;
        self.settings.instance =
            (instance.kind != InstanceKind::Default).then(|| instance.id.clone());
        self.save_settings();
    }

    pub(super) fn open_instance_dialog(&mut self, editing: Option<Instance>) {
        self.version_menu = false;
        // The hook fields start again from the saved instance, not from what was left typed, and
        // so does the memory box.
        self.launch = None;
        self.memory_slider.forget_typing();
        let editing_one = editing.is_some();
        let (name, group) = editing
            .as_ref()
            .map(|i| (i.name.clone(), i.group.clone().unwrap_or_default()))
            .unwrap_or_default();
        self.servers.text(Field::InstanceName).set(name);
        self.servers.text(Field::InstanceGroup).set(group);
        let game = editing
            .as_ref()
            .and_then(|i| i.java.clone())
            .unwrap_or_default();
        let (width, height) = game
            .window
            .map(|(w, h)| (w.to_string(), h.to_string()))
            .unwrap_or_default();
        self.servers.text(Field::JavaArgs).set(game.jvm_args);
        self.servers.text(Field::WindowWidth).set(width);
        self.servers.text(Field::WindowHeight).set(height);
        self.servers.focus_soon = Some(Field::InstanceName);
        // What it plays now: Edit can switch it (`instances::switch`).
        let java = match editing.as_ref().and_then(|i| i.java.as_ref()) {
            None => Default::default(),
            Some(j) => new_instance::JavaChoice {
                kind: match &j.loader {
                    Some(_) => new_instance::NewKind::Modded,
                    None => new_instance::NewKind::Vanilla,
                },
                loader: j
                    .loader
                    .as_ref()
                    .map_or(sunrise_launcher::loaders::Kind::Fabric, |l| l.kind),
                version: Some(j.minecraft.clone()),
                ..Default::default()
            },
        };
        self.instances.dialog = Some(InstanceDialog {
            icon: editing.as_ref().and_then(|i| i.icon.clone()),
            pin: editing.as_ref().and_then(|i| i.version.clone()),
            channel: editing
                .as_ref()
                .map_or(instances::Channel::Stable, |i| i.channel),
            discarding: false,
            memory: None,
            picture: None,
            editing: editing.map(|i| i.id),
            copy: false,
            error: None,
            busy: false,
            java,
        });
        self.servers.text(Field::InstanceVersion).set(String::new());
        // A new one starts with the name it would get ("My Sunrise"), until the player types one.
        if !editing_one {
            self.change(|_| {});
        }
    }

    /// Closes the dialog; a sheet holding edits not saved asks first. A dialog that is saving
    /// stays.
    pub(super) fn close_instance_dialog(&mut self) {
        let changed = self.sheet_changed();
        if let Some(d) = self.instances.dialog.as_mut().filter(|d| !d.busy) {
            if changed {
                d.discarding = true;
            } else {
                self.instances.dialog = None;
            }
        }
    }

    /// Escape closes the dialog, or answers the question about the edits with "Keep editing".
    /// Whether it did anything.
    pub(super) fn instance_escape(&mut self) -> bool {
        match self.instances.dialog.as_mut() {
            Some(d) if d.busy => false,
            Some(d) if d.discarding => {
                d.discarding = false;
                true
            }
            Some(_) => {
                self.close_instance_dialog();
                true
            }
            None => false,
        }
    }

    /// Creates or changes the instance as the dialog says. A copy can be gigabytes of worlds, so
    /// the work runs in the background.
    pub(super) fn save_instance(&mut self, cx: &mut Context<Self>) {
        let edited_id = self
            .instances
            .dialog
            .as_ref()
            .filter(|d| !d.busy)
            .and_then(|d| d.editing.clone());
        if edited_id.is_some() {
            // Nothing to save, as the footer's Save says.
            if !self.sheet_changed() {
                return;
            }
        }
        let launch = match edited_id
            .as_deref()
            .and_then(|id| self.parsed_launch(id))
            .transpose()
        {
            Ok(launch) => launch,
            Err(e) => {
                if let Some(d) = &mut self.instances.dialog {
                    d.error = Some(e);
                }
                return;
            }
        };
        let selected = self.selected_instance().map(|i| i.id.clone());
        let name = self.servers.value(Field::InstanceName).trim().to_string();
        let group = Some(self.servers.value(Field::InstanceGroup).to_string());
        // A Java instance's arguments and window (the sheet's Launch): both sizes or neither.
        let jvm_args = self.servers.value(Field::JavaArgs).trim().to_string();
        let window = match validate_window(
            self.servers.value(Field::WindowWidth),
            self.servers.value(Field::WindowHeight),
        ) {
            Ok(window) => window,
            Err(()) => {
                if let Some(d) = &mut self.instances.dialog {
                    d.error = Some(tr("Window size: enter both width and height as positive whole numbers, or leave both empty.").into());
                }
                return;
            }
        };
        // A Sunrise version picked but not installed downloads once the instance is saved.
        let download = self
            .draft_sunrise()
            .and_then(|tag| self.release_to_install(&tag));
        // An edited instance switched to another game (`instances::switch`).
        let switch_to = self
            .instances
            .dialog
            .as_ref()
            .and_then(|d| d.editing.as_ref())
            .and_then(|id| self.instances.list.iter().find(|i| &i.id == id))
            .filter(|i| i.kind == InstanceKind::Own)
            .and_then(|i| {
                self.draft_plays(i)
                    .filter(|to| *to != instances::Plays::of(i))
            });
        // Maximum memory as the box holds it, typed or not; text that isn't a size stops Save.
        let Ok(memory) = self.sheet_memory() else {
            if let Some(d) = &mut self.instances.dialog {
                d.error = Some(
                    tr("Maximum memory: enter a whole number of MB, or leave it empty for the default.")
                        .into(),
                );
            }
            return;
        };
        let Some(d) = self.instances.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        let icon = d.icon.clone();
        let (pin, channel) = (d.pin.clone(), d.channel);
        let picture = d
            .picture
            .as_ref()
            .map(|p| p.as_ref().map(|c| c.png.clone()));
        let editing = d.editing.clone();
        let edited = editing.is_some();
        let copy = selected.filter(|_| d.copy);
        // The picture choice this save keeps: once kept, it is no longer an unsaved edit.
        let sent_picture = picture.clone();
        // The Sunrise version is Sunrise's instances' alone: a Java one plays its own game.
        let plays_sunrise = d.java.kind == new_instance::NewKind::Sunrise;
        // A new Java instance: what it plays, and its name when none is typed.
        let java = (editing.is_none() && d.java.kind != new_instance::NewKind::Sunrise)
            .then(|| d.java.clone());
        let minecraft = java.as_ref().map(|j| {
            j.minecraft(match &self.instances.versions {
                new_instance::VersionList::Ready(v) => Some(v),
                _ => None,
            })
        });
        let Some(d) = self.instances.dialog.as_mut() else {
            return;
        };
        let download = download.filter(|_| plays_sunrise);
        let (java, name) = match (java, minecraft) {
            (Some(_), Some(None)) => {
                d.error = Some(tr("Mojang's version list isn't here yet.").into());
                return;
            }
            (Some(j), Some(Some(mc))) => {
                let name = if name.is_empty() {
                    j.default_name(&mc)
                } else {
                    name
                };
                (Some((j, mc)), name)
            }
            _ => (None, name),
        };
        d.busy = true;
        d.error = None;
        let task = cx.background_executor().spawn(async move {
            let id = match (editing, java) {
                (Some(id), _) => {
                    if let Some(to) = switch_to {
                        instances::switch(&id, new_instance::with_loader(to)?)?;
                    }
                    id
                }
                (None, Some((j, mc))) => new_instance::create_java(&j, &name, &mc)?.id,
                (None, None) => instances::create(&name, copy.as_deref())?.id,
            };
            // Finish fallible picture work before committing the edited metadata and commands.
            if let Some(png) = picture {
                let instance = instances::find(&id)?;
                instances::set_picture(
                    &instance.home,
                    png.as_deref(),
                    &sunrise_launcher::desktop_seams::to_trash,
                )?;
            }
            let instance = instances::edit(&id, |m| {
                m.name = Some(name);
                m.group = group;
                m.icon = icon;
                if let Some(launch) = launch {
                    m.launch = launch;
                }
                if let Some(game) = m.java.as_mut().filter(|_| edited) {
                    game.jvm_args = jvm_args;
                    game.window = window;
                    if let Some(memory) = memory {
                        game.memory_mb = Some(memory);
                    }
                }
                if plays_sunrise {
                    m.version = pin;
                    m.channel = Some(channel);
                }
            })?;
            Ok(instance)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(instance) => {
                        let name = [("name", instance.name.as_str())];
                        crate::glass_toast::say(if edited {
                            trf("Saved {name}", &name)
                        } else {
                            trf("Created {name}", &name)
                        });
                        this.select_instance(&instance);
                        this.reload_instances();
                        if let Some(d) = &mut this.instances.dialog {
                            let now = d.picture.as_ref().map(|p| p.as_ref().map(|c| &c.png));
                            if now == sent_picture.as_ref().map(Option::as_ref) {
                                d.picture = None;
                            }
                        }
                        // Edits typed while it saved are still open: the sheet stays for them.
                        let typed_meanwhile = edited && this.sheet_changed();
                        match (typed_meanwhile, &mut this.instances.dialog) {
                            (true, Some(d)) => d.busy = false,
                            _ => this.instances.dialog = None,
                        }
                        // Play shows the download's progress; Versions shows it too.
                        if let Some(release) = download {
                            this.install_for_instance(release);
                        }
                        if matches!(instance.kind, InstanceKind::Default | InstanceKind::Own) {
                            this.go(Page::Play);
                        }
                    }
                    Err(e) => {
                        if let Some(d) = &mut this.instances.dialog {
                            d.busy = false;
                            d.error = Some(e);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Validate the two fields together, without treating an invalid edit as an empty size.
fn validate_window(width: &str, height: &str) -> Result<Option<(u32, u32)>, ()> {
    let (width, height) = (width.trim(), height.trim());
    if width.is_empty() && height.is_empty() {
        return Ok(None);
    }
    let width = width.parse::<u32>().map_err(|_| ())?;
    let height = height.parse::<u32>().map_err(|_| ())?;
    if width == 0 || height == 0 {
        return Err(());
    }
    Ok(Some((width, height)))
}

#[cfg(test)]
mod window_tests {
    use super::validate_window;

    #[test]
    fn window_size_requires_two_positive_dimensions_or_two_empty_fields() {
        assert_eq!(validate_window("", " "), Ok(None));
        assert_eq!(validate_window(" 1280", "720 "), Ok(Some((1280, 720))));
        for (width, height) in [
            ("", "720"),
            ("1280", ""),
            ("0", "720"),
            ("1280", "0"),
            ("wide", "720"),
            ("1280", "1.5"),
            ("4294967296", "720"),
        ] {
            assert_eq!(validate_window(width, height), Err(()));
        }
    }
}
