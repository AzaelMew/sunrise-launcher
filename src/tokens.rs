//! The look's tokens: its type scale, its corner radii and the few sizes the whole
//! window shares. A screen names one of these, never a number of its own, so the scale changes
//! here and nowhere else. Sizes are logical pixels.
//!
//! Spacing (gaps and padding) and a single screen's own layout (a column's width, a picture's
//! height) are not tokens: they say how one screen is laid out, not what the look is.

// ---- type scale

/// Tags, counts and the rail's labels.
pub(crate) const CAPTION: f32 = 11.;
/// A second line under a name; hints.
pub(crate) const SMALL: f32 = 12.;
/// Controls, and the muted sentence under a heading.
pub(crate) const LABEL: f32 = 13.;
/// What the window is set in.
pub(crate) const BODY: f32 = 14.;
/// Play.
pub(crate) const LEAD: f32 = 16.;
/// A section of a page.
pub(crate) const HEADING: f32 = 18.;
/// A sheet's title.
pub(crate) const TITLE: f32 = 24.;
/// A page's title.
pub(crate) const DISPLAY: f32 = 28.;

// ---- corner radii

/// Progress bars and other hairlines.
pub(crate) const R_BAR: f32 = 2.;
/// Checkboxes, tags and small tiles.
pub(crate) const R_SMALL: f32 = 5.;
/// Buttons, fields and chips.
pub(crate) const R_CONTROL: f32 = 9.;
/// Cards and the rows' pictures.
pub(crate) const R_CARD: f32 = 12.;
/// Sheets, and the page itself.
pub(crate) const R_PANEL: f32 = 16.;

// ---- shared sizes

/// A button's height.
pub(crate) const CONTROL: f32 = 32.;
/// A text field's height, and a tab's.
pub(crate) const FIELD: f32 = 36.;
/// A page's or a sheet's main action.
pub(crate) const ACTION: f32 = 44.;
/// The rail's width.
pub(crate) const RAIL: f32 = 72.;
/// The play bar's height, and Play's inside it.
pub(crate) const BAR: f32 = 72.;
pub(crate) const PLAY: f32 = 50.;
