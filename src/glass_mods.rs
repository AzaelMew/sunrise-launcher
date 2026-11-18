//! The Mods page in the glass look: the mockup's `ModsView`. The page's tabs on the right of its
//! title, then two glass cards: the search and the list of what it found (icon, name, author,
//! summary, then downloads for the registry's and how it runs for Modrinth's, a tick on what the
//! instance has), and the picked project's card (its
//! icon, name, line, summary, what it may do, downloads, and Install in or Remove from the
//! instance).
//!
//! The mockup's tabs are its sources (Sunrise, Modrinth, CurseForge soon); the launcher's page
//! is by kind (mods, modpacks, resource packs, shader packs, data packs), each with its sources,
//! so the tabs are the kinds and the sources are a switch under the search: Sunrise's registry
//! or Modrinth, CurseForge beside them as coming soon, on the kinds both serve (`switches`):
//! shader packs are Sunrise's registry's only, and a Java
//! instance's files always Modrinth's. Modrinth's Java mods are offered to a Sunrise instance
//! too, each marked Works, Partly or Doesn't run in Sunrise. Not in the mockup, kept in its
//! patterns: Show everything, the filters, which instance or world installs go to (under the
//! search); the
//! project's page, its rating and why it runs or not (in the project's card); adding a native mod
//! from GitHub, the instance's installed list and its native build, imported modpacks and the
//! install's progress (cards under the two).


use crate::glass_ui::{ white};
use sunrise_launcher_theme::glass::{ink, radius};


/// Bringing a picked project's card into view where it sits under the list (a narrow window):
/// a click on a row would otherwise seem to do nothing, its card twenty rows further down.
#[derive(Default)]
pub(crate) struct Reveal {
    /// A project was just picked: the next layouts scroll the page to its card, this many more
    /// times at most. The list above it builds only its nearby rows and guesses the others'
    /// heights, so scrolling there measures some and moves the card: the next frame corrects.
    pub wanted: std::cell::Cell<u8>,
    /// Where the page's top was laid out, measured as the card is: the page scrolls in its own
    /// layer, so only the distance between the two means the same in every frame.
    top: std::cell::Cell<Option<gpui::Pixels>>,
}


