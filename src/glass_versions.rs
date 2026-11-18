//! The Versions page in the glass look: the mockup's `VersionsView`. Each Nightly on its own glass
//! card (its name, Latest on the first, when it came out and which instances play it), Install
//! or a bin on the right, and Install new versions automatically by the page's title.
//!
//! Not in the mockup, kept in its patterns: Use and In use (the version instances without their
//! own play), the space versions take, Import a game build, the native and imported builds,
//! install progress and errors, the confirmation before a removal.

use super::*;


use sunrise_launcher_theme::glass::{ink, radius};

/// A release's day as the mockup writes it ("Nov 1"), with its time in UTC when known.
pub(super) fn published(date: &str, time: &str) -> String {
    let day = sunrise_launcher::nightly::day(date);
    if time.is_empty() {
        day
    } else {
        trf("{day}, {time} UTC", &[("day", &day), ("time", time)])
    }
}

/// The compact title and commit line for a published Nightly.
///
/// A nightly's short commit, which only a tooltip shows; empty for any other version.
fn commit_of(tag: &str) -> String {
    sunrise_launcher::nightly::label(tag)
        .and_then(|label| {
            label
                .split_once(" · ")
                .map(|(_, commit)| commit.chars().take(7).collect())
        })
        .unwrap_or_default()
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

    /// The names of the Sunrise instances that play `tag`.
    pub(crate) fn instances_using(&self, tag: &str) -> Vec<&str> {
        self.instances
            .list
            .iter()
            .filter(|i| i.java.is_none() && self.version_of(i).map(String::as_str) == Some(tag))
            .map(|i| i.name.as_str())
            .collect()
    }

}



#[cfg(test)]
mod tests {
    use super::*;

}
