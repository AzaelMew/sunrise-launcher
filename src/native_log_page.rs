//! A native-mods build attempt's own log (`native_mods::log_path`): each attempt starts it fresh
//! before its preflight can fail (`native_log_reset`, `native_log_preflight`), so the file never
//! shows the last attempt as the latest.
use super::*;

use sunrise_launcher::native_mods;

/// Whether the attempt's log could be started, kept so a preflight failure is written only into
/// a log that was cleared.
#[derive(Default)]
pub(super) struct NativeLog {
    /// Why this attempt's log could not be reset.
    reset: Option<String>,
}

impl Launcher {
    /// Begins a build attempt's own log, before anything of it can fail: the last attempt's
    /// file is emptied.
    pub(super) fn native_log_reset(&mut self) {
        self.native_log.reset = native_mods::clear_log().err();
    }

    /// Records a failure from before the build itself started (no Sunrise to build on, no mods
    /// to build), where the build's own output would have gone.
    pub(super) fn native_log_preflight(&mut self, error: &str) {
        if self.native_log.reset.is_none() {
            self.native_log.reset = native_mods::append_log(error).err();
        }
    }
}
