//! What an instance's icon shows (plan R8): its own picture (one the player chose, or its
//! modpack's icon) first; else the block or colour chosen in its dialog; else its mark: the loader
//! it plays with, the grass block for plain Minecraft, Sunrise's own for Sunrise. The letter tile
//! is the last resort. Each is decoded once per size and kept in `Instances::icons`.
use super::*;
use gpui::RenderImage;
use std::sync::Arc;
use sunrise_launcher::browser::images;

impl Launcher {
    /// The image under `key` at `size` points, made from `bytes` the first time. A missing one is
    /// remembered only when `remember_missing` (a file that may never exist); else it is asked
    /// again next frame (a logo still downloading).
    pub(super) fn icon_image(
        &mut self,
        key: String,
        size: f32,
        remember_missing: bool,
        bytes: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<Arc<RenderImage>> {
        let key = (key, (size * self.scale) as u32);
        if let Some(image) = self.instances.icons.get(&key) {
            return image.clone();
        }
        let image = bytes().and_then(|b| fitted(b, size, self.scale));
        if image.is_some() || remember_missing {
            self.instances.icons.insert(key, image.clone());
        }
        image
    }
}

/// An image file's bytes drawn at most `size` points square: scaled down smoothly when larger,
/// up by whole device pixels when smaller, so pixel-art logos stay sharp.
fn fitted(bytes: Vec<u8>, size: f32, scale: f32) -> Option<Arc<RenderImage>> {
    let side = (size * scale).round().max(1.) as u32;
    let png = images::fit_png(bytes, side, 4096, 64 << 20)?;
    let image = image::load_from_memory(&png).ok()?.to_rgba8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let texels: Vec<[u8; 4]> = image.pixels().map(|p| p.0).collect();
    let whole = (side as usize / w.max(h)).max(1) as f32;
    Some(art::bake(&texels, w, h, whole / scale, scale))
}

/// A picture chosen in the instance dialog, written on Save.
#[derive(Clone)]
pub(super) struct Chosen {
    pub(super) png: Vec<u8>,
}
