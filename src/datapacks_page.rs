//! A world's data packs, opened under its row on Home (`sunrise_launcher::datapacks`): what is
//! in `datapacks/`, Add (a file picker) or a zip dropped on the world, and Remove (to the trash,
//! after a confirmation).


use std::path::PathBuf;
use sunrise_launcher::datapacks::{self, DataPack};


#[derive(Default)]
pub(super) struct WorldPacks {
    /// The world whose packs are open, and its packs as last read.
    open: Option<PathBuf>,
    list: Vec<DataPack>,
    confirm_remove: Option<String>,
    note: Option<Result<String, String>>,
}

impl WorldPacks {
    /// Reads the open world's packs again, after something else changed them.
    pub(super) fn reread(&mut self) {
        if let Some(world) = &self.open {
            self.list = datapacks::list(world);
        }
    }
}

