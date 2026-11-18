//! Variable-height lists in the page's scroll area. Only nearby rows build elements;
//! unseen rows use estimated heights and consecutive hidden rows share one spacer.

use std::{cell::RefCell, rc::Rc};


#[derive(Clone, Default)]
pub struct Cull(Rc<RefCell<Drawn>>);

#[derive(Default)]
struct Drawn {
}

impl Cull {
    /// Invalidate measurements when filtering, ordering or collection identity changes.
    pub fn reset(&self) {
        *self.0.borrow_mut() = Drawn::default();
    }

}
