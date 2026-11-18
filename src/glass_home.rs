//! Home's head in the glass look (the mockup's `HomeView`, `launcher-views.tsx`): the instance's
//! edition, its icon and name, how long it was played, then Play with its menu, the version, the
//! instance's mods and its folders, and joining a friend on the right. Dark appearance only.

/// The Version menu's state (Home's picker).
#[derive(Default)]
pub(super) struct PickerState {
    /// The menu is open (the old look's; the glass look keeps versions in instance settings).
    pub(super) open: bool,
}

