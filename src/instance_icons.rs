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
    pub(super) preview: Arc<RenderImage>,
}

impl Launcher {
    /// Decode the chosen file once, off the UI thread. Keep this draft open until the picker
    /// returns: a response to one profile must never replace the icon on another.
    pub(super) fn choose_profile_picture(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.instances.dialog.as_mut().filter(|d| !d.busy) else {
            return;
        };
        d.busy = true;
        let scale = self.scale;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Choose a picture").into()),
        });
        cx.spawn(async move |this, cx| {
            let choice = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let made = if let Some(path) = choice {
                Some(
                    cx.background_executor()
                        .spawn(async move {
                            use std::io::Read as _;
                            const MAX: u64 = 64 << 20;
                            let mut bytes = Vec::new();
                            std::fs::File::open(path)
                                .and_then(|f| f.take(MAX + 1).read_to_end(&mut bytes))
                                .map_err(|e| e.to_string())?;
                            if bytes.len() as u64 > MAX {
                                return Err(tr("Choose a picture smaller than 64 MiB.").into());
                            }
                            let png = instances::picture(bytes)?;
                            let preview = fitted(png.clone(), 64., scale)
                                .ok_or_else(|| tr("Not a picture").to_string())?;
                            Ok(Chosen { png, preview })
                        })
                        .await,
                )
            } else {
                None
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(d) = this.instances.dialog.as_mut() {
                    d.busy = false;
                    match made {
                        Some(Ok(c)) => {
                            d.picture = Some(Some(c));
                            d.icon = None;
                            d.error = None;
                        }
                        Some(Err(e)) => d.error = Some(e),
                        None => {}
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
