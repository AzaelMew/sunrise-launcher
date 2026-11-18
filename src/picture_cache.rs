//! The pictures Home draws from the player's disk (screenshots, their blurred copies, the
//! background), made in the background at the size they are drawn and kept under a byte budget
//! (`sunrise_launcher::pictures`).
//!
//! GPUI's `img(path)` keeps every picture it ever drew, at full size, until the launcher quits,
//! in RAM and in its sprite atlas: one 32768 × 16384 screenshot is 2 GiB, and Overview draws the
//! three newest as soon as Home opens (24 GiB reported on Linux, 2026-11-05). And a picture
//! wider than the GPU's texture limit made GPUI's atlas add a max-size texture on every frame.
//! Pictures are therefore made at the size the screen shows them, never above that limit
//! (`pictures::drawn_side`, `pictures::gpu_limit`).
use super::*;
use gpui::RenderImage;
use std::collections::HashSet;
use std::path::{ PathBuf};
use std::sync::Arc;
use sunrise_launcher::pictures::{ Lru};

/// What the made pictures may take together, unless more than that is on screen at once.
const BUDGET: u64 = 384 << 20;
/// A picture drawn this recently is on screen: it stays even over the budget.
const GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Look {
    /// Fitted inside this many pixels square.
    Fit(u32),
    /// Small and blurred (`glass_shot_blur`).
    Blurred,
}

type Key = (PathBuf, Look);

pub(super) struct Pictures {
    /// None: the file couldn't be read as a picture.
    made: Lru<Key, Option<Arc<RenderImage>>>,
    making: HashSet<Key>,
    /// Gone from `made`; freed from the window's sprite atlas on the next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl Default for Pictures {
    fn default() -> Self {
        Pictures {
            made: Lru::new(BUDGET, GRACE),
            making: HashSet::new(),
            retired: Vec::new(),
        }
    }
}

impl Pictures {

    /// Frees the pictures dropped since the last frame where GPUI keeps them too.
    pub(super) fn free_retired(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }
}


