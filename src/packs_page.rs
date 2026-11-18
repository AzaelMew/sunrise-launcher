//! The selected instance's resource packs and shader packs on Home (`sunrise_launcher::packs`):
//! each turned on or off with one click, Remove (to the trash, after a confirmation), and
//! Add (a file picker) or a .zip or .sshader dropped anywhere on the section.
use super::*;
use gpui::{ Image, ImageFormat};
use std::path::PathBuf;
use std::sync::Arc;
use sunrise_launcher::packs::{self, Kind as PackKind, Pack};

#[derive(Default)]
pub(super) struct Packs {
    home: Option<PathBuf>,
    pub(super) resource: Vec<(Pack, Option<Arc<Image>>)>,
    pub(super) shader: Vec<Pack>,
    fresh: bool,
    /// The pack waiting for its removal to be confirmed: its kind and file.
    pub(super) confirm_remove: Option<(PackKind, String)>,
}

impl Packs {
    pub(super) fn stale(&mut self) {
        self.fresh = false;
    }
}

impl Launcher {
    pub(super) fn load_packs_list(&mut self, home: PathBuf) {
        if self.game_packs.home.as_ref() != Some(&home) {
            self.game_packs = Packs {
                home: Some(home.clone()),
                ..Packs::default()
            };
        }
        if self.game_packs.fresh {
            return;
        }
        // A few small reads per pack (pack.mcmeta, pack.png): fine while drawing.
        self.game_packs.resource = packs::list(&home, PackKind::Resource)
            .into_iter()
            .map(|p| {
                let icon = p
                    .icon
                    .clone()
                    .map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png)));
                (p, icon)
            })
            .collect();
        {
            let priority = packs::resource_order(&home);
            let rank: HashMap<_, _> = priority
                .iter()
                .enumerate()
                .map(|(i, name)| (name, i))
                .collect();
            // Stable sort leaves unselected packs alphabetic after the active stack.
            self.game_packs
                .resource
                .sort_by_key(|(p, _)| rank.get(&p.file).copied().unwrap_or(usize::MAX));
        }
        self.game_packs.shader = packs::list(&home, PackKind::Shader);
        self.game_packs.fresh = true;
    }

}
