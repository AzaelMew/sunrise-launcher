//! A panel that stays in view beside a long list as the page scrolls (CSS `position: sticky;
//! top: 0`, which GPUI has no style for): the Mods page's project card (the mockup's
//! `lg:sticky lg:top-0`).
//!
//! The row holding the list and the panel records where it lies in the page and how tall it and
//! the panel are while it paints; the panel is then drawn moved down (`top`, which takes nothing
//! from the layout) by how far the page has scrolled past the row's top, never past the row's
//! end. GPUI Fast draws a scrolled page from its layer without rendering it again, so the page's
//! scroll asks [`Sticky::moved`] on the next frame whether the panel must be drawn elsewhere.

use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct Measured {
    /// The row's top in the page's content (unscrolled), its height, the panel's height.
    top: f32,
    row: f32,
    panel: f32,
}

#[derive(Clone, Default)]
pub struct Sticky {
    measured: Rc<Cell<Option<Measured>>>,
    /// The offset the panel was last drawn at.
    drawn: Rc<Cell<f32>>,
}

impl Sticky {

    /// Forgets the row (the page left), so a new one starts unmoved.
    pub fn reset(&self) {
        self.measured.set(None);
        self.drawn.set(0.);
    }

}

