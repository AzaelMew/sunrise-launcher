//! The look's plates (panels, the sidebar, dialogs, menus): a flat fill in the theme's card
//! colour under a 1 px line, with the look's corners. Nothing is frosted: the plates are solid,
//! so what is under a dialog never reads through it.
use gpui::{div, prelude::*, px, rgb, AnyElement, Div};
use std::cell::Cell;

/// The Outline look's flat plate, under its 1 px line.
pub const PANEL: u32 = 0x171b2e;

thread_local! {
    /// The Outline look's corner radius ([`set_corners`]).
    static CORNERS: Cell<f32> = const { Cell::new(0.) };
    /// The flat plate's colour for the theme and appearance shown ([`set_panel`]).
    static OUTLINE_PANEL: Cell<u32> = const { Cell::new(PANEL) };
    /// Whether the pages being built draw the Outline look ([`set_outline`]).
    static OUTLINE: Cell<bool> = const { Cell::new(false) };
}

/// Set with the launcher's look as each frame's pages start being built.
pub fn set_outline(on: bool) {
    OUTLINE.set(on);
}

fn outline() -> bool {
    OUTLINE.get()
}

/// A corner: in the Outline look, the radius the player picked, the same for every corner
/// ([`set_corners`]).
fn rad(r: f32) -> f32 {
    if outline() {
        CORNERS.get()
    } else {
        r
    }
}

/// Set as each frame starts: the Outline look's corner radius in px.
pub fn set_corners(radius: f32) {
    CORNERS.set(radius);
}

/// Set as each frame starts: the theme's card colour, so plates match its sky.
pub fn set_panel(c: u32) {
    OUTLINE_PANEL.set(c);
}

/// The Outline look's plate colour now.
pub fn panel() -> u32 {
    OUTLINE_PANEL.get()
}

/// How far below the top bar the pages' scroll area starts, so what scrolls up stops short of the
/// bar instead of running into it. The pages' heads hold that much less above their titles, so
/// nothing moves at rest.
pub const CONTENT_INSET: f32 = 14.;

/// A plate's fill with its corners: put it first inside a `relative` element, the content after
/// it. Panels and menus share the one solid fill.
pub fn backdrop(radius: f32) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(rgb(panel()))
        .rounded(px(rad(radius)))
        .into_any_element()
}

/// A plate's 1 px edge: a faint line, so a plate reads as a surface.
fn edge() -> gpui::Hsla {
    gpui::white().opacity(if outline() { 0.12 } else { 0.09 })
}

/// A `relative` div on a plate with `radius` corners.
pub fn plate(radius: f32) -> Div {
    div()
        .relative()
        .rounded(px(rad(radius)))
        .border_1()
        .border_color(edge())
        .child(backdrop(radius))
}

/// The same for a menu or popover.
pub fn float(radius: f32) -> Div {
    div()
        .relative()
        .rounded(px(rad(radius)))
        .border_1()
        .border_color(edge())
        .child(backdrop(radius))
}
