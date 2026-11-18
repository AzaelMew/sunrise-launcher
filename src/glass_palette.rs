//! The palette (⌘K, Ctrl+K elsewhere): the mockup's `Palette`. A glass card 560 px wide at 14 %
//! of the window's height, over the dimmed launcher: a search line, then up to eight commands,
//! each its name and a hint. Arrows move, Enter runs, Esc or a click outside closes.
//!
//! The commands are the mockup's: Play each instance, Open each page and Settings, New
//! instance, Open the game folder, and from two letters on, Install or Remove the registry's
//! mods that match, in the selected instance.


use super::*;


use sunrise_launcher::browser::Project;



pub(super) struct Palette {
    focus: FocusHandle,
    /// The registry's mods for the query [`asked`](Self::asked) last.
    pub(super) mods: Vec<Project>,
    asked: String,
    /// The registry search's number: an answer to an older one is dropped.
    pub(super) search: u64,
}

