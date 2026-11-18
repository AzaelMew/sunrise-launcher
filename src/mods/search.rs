//! What the launcher's search (`command_search`) reads of the installed mods and packs, and how
//! it shows one of them on its page. Reads what is in memory or a few small files; writes
//! nothing.
use super::*;
use std::sync::Arc;
use sunrise_launcher::packs::Kind;

impl Launcher {
    /// The selected profile's installed mods: each one's name and its icon's address.
    pub(crate) fn mods_for_search(&mut self) -> Vec<(String, Option<String>)> {
        self.mods_refresh();
        self.rows()
            .into_iter()
            .map(|row| (row.title, row.icon))
            .collect()
    }

    /// Its resource packs, then its shader packs: each one's kind, name and picture.
    pub(crate) fn packs_for_search(&mut self) -> Vec<(Kind, String, Option<Arc<gpui::Image>>)> {
        self.load_packs_list(self.mods_home());
        let packs = &self.game_packs;
        packs
            .resource
            .iter()
            .map(|(pack, icon)| (pack.kind, pack.title.clone(), icon.clone()))
            .chain(
                packs
                    .shader
                    .iter()
                    .map(|pack| (pack.kind, pack.title.clone(), None)),
            )
            .collect()
    }

    /// Mods › Installed lists the mod called `title`, as typing its name in the page's own
    /// filter would.
    pub(crate) fn show_installed_mod(&mut self, title: &str) {
        self.mods_refresh();
        let m = &mut self.ui.mods;
        m.have.set(title.to_string());
        m.row_filter.text = title.to_string();
    }

    /// Packs › Installed lists the pack called `title`, on the shelf of its kind.
    pub(crate) fn show_installed_pack(&mut self, kind: Kind, title: &str) {
        self.choose_view(Page::Packs);
        let m = &mut self.ui.mods;
        m.shelf = match kind {
            Kind::Resource => Shelf::Resource,
            Kind::Shader => Shelf::Shader,
        };
        m.have.set(title.to_string());
        m.row_filter.text = title.to_string();
    }
}
