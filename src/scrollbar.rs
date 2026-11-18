//! Pointer-operated scrollbars for the launcher's vertical scroll areas.

use gpui::{  ScrollHandle};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Default)]
pub(crate) struct Scrollbar {
    pub handle: ScrollHandle,
    /// Distance from the top of the thumb to the point grabbed by the pointer.
    grabbed: Rc<Cell<Option<f32>>>,
    /// The vertical offset the last wheel event found, before any handler moved it (`take_wheel`).
    wheel_before: Rc<Cell<f32>>,
}



