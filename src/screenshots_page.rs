//! The selected instance's screenshots on Home (`sunrise_launcher::screenshots`): the newest as
//! thumbnails, all of them on request, and a viewer (← and → step through) with Copy, Open,
//! Show in folder and Delete (to the trash, after a confirmation). Pictures are made at their
//! shown size (`picture_cache`), never decoded whole.



use std::path::PathBuf;
use sunrise_launcher::screenshots::{ Shot};


#[derive(Default)]
pub(super) struct Screenshots {
    home: Option<PathBuf>,
    pub(super) list: Vec<Shot>,
    fresh: bool,
    pub(super) all: bool,
    /// The picture open in the viewer, by its place in `list`.
    pub(super) viewing: Option<usize>,
    pub(super) confirm_delete: bool,
    /// What the last action did, or why it failed.
    pub(super) note: Option<Result<String, String>>,
}

impl Screenshots {
    /// Read again on the next frame: the game may have taken new ones.
    pub(super) fn stale(&mut self) {
        self.fresh = false;
    }
}

