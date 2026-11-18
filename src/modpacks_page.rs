//! Imported modpacks, on the Mods page's Modpacks tab: import a pack, read its coverage report,
//! port what this computer can and play it as its own profile (`sunrise_launcher::modpacks`).
use super::*;

use sunrise_launcher::modpacks::Imported;

impl Launcher {
    /// A finished step: the pack's new state, opened.
    pub(super) fn pack_done(&mut self, result: Result<Box<Imported>, String>) {
        self.pack_busy = false;
        match result {
            Ok(pack) => {
                self.mods.report_list.reset();
                self.pack_open = Some(pack.slug.clone());
                self.packs.retain(|p| p.slug != pack.slug);
                self.packs.push(*pack);
                self.packs.sort_by_key(|p| p.title().to_ascii_lowercase());
            }
            Err(e) => self.pack_error = Some(e),
        }
    }
}
