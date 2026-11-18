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

use std::sync::Arc;

#[derive(Default)]
pub(super) struct Pictures {
    /// Gone from `made`; freed from the window's sprite atlas on the next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl Pictures {
    /// Reclaim a thumbnail's atlas allocation once the frame that drew it is gone.
    pub(super) fn retire(&mut self, image: Arc<RenderImage>) {
        self.retired.push(image);
    }

    /// Frees the pictures dropped since the last frame where GPUI keeps them too.
    pub(super) fn free_retired(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }
}
