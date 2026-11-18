//! Export… on Home's Instance row: the selected instance as a `.mrpack` or a `.zip`, with the
//! worlds the player switches on, written to the launcher's `exports/` folder
//! (`sunrise_launcher::export`).

use std::path::PathBuf;
use sunrise_launcher::export::{ Format};

pub(super) struct ExportPanel {
    /// The instance it is for: picking another closes it.
    instance: String,
    format: Format,
    /// The instance's worlds (folder, name) and whether each goes in.
    worlds: Vec<(String, String, bool)>,
    busy: bool,
    /// The file written, or why not.
    done: Option<Result<(String, PathBuf), String>>,
}


