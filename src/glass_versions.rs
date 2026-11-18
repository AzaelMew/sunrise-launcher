//! The Versions page in the glass look: the mockup's `VersionsView`. Each Nightly on its own glass
//! card (its name, Latest on the first, when it came out and which instances play it), Install
//! or a bin on the right, and Install new versions automatically by the page's title.
//!
//! Not in the mockup, kept in its patterns: Use and In use (the version instances without their
//! own play), the space versions take, Import a game build, the native and imported builds,
//! install progress and errors, the confirmation before a removal.

use super::*;




/// A release's day as the mockup writes it ("Nov 1"), with its time in UTC when known.
pub(super) fn published(date: &str, time: &str) -> String {
    let day = sunrise_launcher::nightly::day(date);
    if time.is_empty() {
        day
    } else {
        trf("{day}, {time} UTC", &[("day", &day), ("time", time)])
    }
}

impl Launcher {
    /// Version `tag` as the Versions page titles it ("Nov 11 update", with its time when another
    /// update came out the same day, [`sunrise_launcher::nightly::same_day_time`]), for any screen
    /// that names it; the tag itself otherwise.
    pub(crate) fn version_name(&self, tag: &str) -> String {
        match sunrise_launcher::nightly::date_of(tag) {
            Some(date) => {
                let time = sunrise_launcher::nightly::same_day_time(tag).unwrap_or_default();
                trf("{date} update", &[("date", &published(date, &time))])
            }
            None => tag.to_owned(),
        }
    }

}



