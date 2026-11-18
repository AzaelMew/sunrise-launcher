//! Arrows for the top bar's page tabs when they overflow, each on an edge that fades into the
//! plate.


use gpui::{ ScrollHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};



/// The tabs' scroll, the ends it last drew arrows for, and the arrows' and the scroll's motion.
#[derive(Clone, Default)]
pub struct NavStrip {
    scroll: ScrollHandle,
    drawn: Rc<Cell<(bool, bool)>>,
    motion: Rc<RefCell<Motion>>,
    ticking: Rc<Cell<bool>>,
    /// Whether changes are instant: Performance mode or reduced motion, set on each render.
    instant: Rc<Cell<bool>>,
}

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    began: Instant,
}


#[derive(Default)]
struct Fade {
    shown: bool,
    tween: Option<Tween>,
}


#[derive(Default)]
struct Motion {
    cues: [Fade; 2],
    slide: Option<Tween>,
}



