//! A Java instance's part of the glass instance dialog (`glass_instance.rs`; the mockup's
//! `InstanceModal` for Vanilla and Modded): Mojang's versions to pick from, then for Modded the
//! loaders, each with its logo, as many as `loaders::Kind` knows.
use super::*;

use new_instance::NewKind;

use sunrise_launcher::instances::{Instance, Plays};
use sunrise_launcher::loaders::Loader;

impl Launcher {
    /// What the dialog has `editing` play: Sunrise, or a Java game with its loader. The loader
    /// keeps its version while its kind and Minecraft stay; a new one's (empty here) is the
    /// newest stable, asked of its service on Save.
    pub(super) fn draft_plays(&self, editing: &Instance) -> Option<Plays> {
        let d = self.instances.dialog.as_ref()?;
        if d.java.kind == NewKind::Sunrise {
            return Some(Plays::Sunrise);
        }
        let minecraft = self.draft_minecraft()?;
        let loader = (d.java.kind == NewKind::Modded).then(|| {
            editing
                .java
                .as_ref()
                .filter(|j| j.minecraft == minecraft)
                .and_then(|j| j.loader.clone())
                .filter(|l| l.kind == d.java.loader)
                .unwrap_or(Loader {
                    kind: d.java.loader,
                    version: String::new(),
                })
        });
        Some(Plays::Java { minecraft, loader })
    }
}
