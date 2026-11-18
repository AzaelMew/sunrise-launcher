//! The selected instance's screenshots on Home (`sunrise_launcher::screenshots`): the newest as
//! thumbnails, all of them on request, and a viewer (← and → step through) with Copy, Open,
//! Show in folder and Delete (to the trash, after a confirmation). Pictures are made at their
//! shown size (`picture_cache`), never decoded whole.







#[derive(Default)]
pub(super) struct Screenshots {
    fresh: bool,
}

impl Screenshots {
    /// Read again on the next frame: the game may have taken new ones.
    pub(super) fn stale(&mut self) {
        self.fresh = false;
    }
}

