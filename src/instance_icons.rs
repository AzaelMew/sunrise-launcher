//! What an instance's icon shows (plan R8): its own picture (one the player chose, or its
//! modpack's icon) first; else the block or colour chosen in its dialog; else its mark: the loader
//! it plays with, the grass block for plain Minecraft, Sunrise's own for Sunrise. The letter tile
//! is the last resort. Each is decoded once per size and kept in `Instances::icons`.
use super::*;
use gpui::RenderImage;
use std::sync::Arc;
use sunrise_launcher::browser::images;

impl Launcher {
    /// The image under `key` at `size` points, made from the file `path` names. The file is read
    /// and decoded by the one artwork worker, never in a frame: the first ask answers None (the
    /// caller draws its glyph) and queues it, and the picture shows once [`Self::artwork_tick`]
    /// has it. A file that is missing, over [`instances::MAX_ARTWORK`] or no picture stays
    /// None, and is not asked for again.
    pub(super) fn icon_image(
        &mut self,
        key: String,
        size: f32,
        path: impl FnOnce() -> Option<std::path::PathBuf>,
    ) -> Option<Arc<RenderImage>> {
        let key = (key, (size * self.scale) as u32);
        if let Some(image) = self.instances.icons.get(&key) {
            return image.clone();
        }
        self.instances.icons.insert(key.clone(), None);
        if let Some(path) = path() {
            let _ = loader().send(Job {
                key,
                path,
                size,
                scale: self.scale,
            });
        }
        None
    }

    /// Takes the pictures the worker finished. True when there is something new to draw.
    pub(super) fn artwork_tick(&mut self) -> bool {
        let made = std::mem::take(&mut *MADE.lock().unwrap_or_else(|e| e.into_inner()));
        let mut changed = false;
        for (job, texels) in made {
            // Asked for and since dropped (its profile's picture changed): the next frame asks
            // again, and the worker, which takes one job at a time in order, answers that later.
            let Some(slot) = self.instances.icons.get_mut(&job.key) else {
                continue;
            };
            *slot = texels.map(|texels| baked(texels, job.size, job.scale));
            changed |= slot.is_some();
        }
        changed
    }
}

/// One picture for [`Launcher::icon_image`]'s worker.
struct Job {
    key: (String, u32),
    path: std::path::PathBuf,
    size: f32,
    scale: f32,
}

/// A decoded picture: its pixels row by row, its width and its height.
type Texels = (Vec<[u8; 4]>, usize, usize);

/// What the worker has made since the window last looked.
static MADE: std::sync::Mutex<Vec<(Job, Option<Texels>)>> = std::sync::Mutex::new(Vec::new());

/// The one worker that reads artwork from disk: a queue, so a sidebar of many profiles or a
/// search that matches many worlds costs the window nothing, and at most one file is in memory.
fn loader() -> &'static std::sync::mpsc::Sender<Job> {
    static LOADER: std::sync::OnceLock<std::sync::mpsc::Sender<Job>> = std::sync::OnceLock::new();
    LOADER.get_or_init(|| {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        std::thread::spawn(move || {
            for job in jobs {
                let texels = artwork(&job.path, job.size, job.scale);
                MADE.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((job, texels));
                crate::wake::wake();
            }
        });
        tx
    })
}

/// The picture file at `path` decoded for drawing at `size` points: None when it is over
/// [`instances::MAX_ARTWORK`], past the decoder's limits, or no picture.
fn artwork(path: &std::path::Path, size: f32, scale: f32) -> Option<Texels> {
    texels(instances::read_artwork(path)?, size, scale)
}

/// An image file's bytes at most `size` points square, scaled down smoothly when larger.
fn texels(bytes: Vec<u8>, size: f32, scale: f32) -> Option<Texels> {
    let side = (size * scale).round().max(1.) as u32;
    let png = images::fit_png(bytes, side, 4096, 64 << 20)?;
    let image = image::load_from_memory(&png).ok()?.to_rgba8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    Some((image.pixels().map(|p| p.0).collect(), w, h))
}

/// [`texels`] as the window draws them: scaled up by whole device pixels when smaller than
/// `size`, so pixel-art logos stay sharp.
fn baked((texels, w, h): Texels, size: f32, scale: f32) -> Arc<RenderImage> {
    let side = (size * scale).round().max(1.) as u32;
    let whole = (side as usize / w.max(h)).max(1) as f32;
    art::bake(&texels, w, h, whole / scale, scale)
}

/// An image file's bytes drawn at most `size` points square.
fn fitted(bytes: Vec<u8>, size: f32, scale: f32) -> Option<Arc<RenderImage>> {
    texels(bytes, size, scale).map(|made| baked(made, size, scale))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_over_the_limit_or_no_picture_is_refused_and_a_picture_is_fitted() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("icon.png");
        image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255]))
            .save(&icon)
            .unwrap();
        let (pixels, w, h) = artwork(&icon, 16., 2.).unwrap();
        assert_eq!((w, h, pixels.len()), (32, 16, 32 * 16));
        assert_eq!(pixels[0], [10, 20, 30, 255]);
        // A file past the limit is never read, whatever it holds: here the same picture,
        // with its length stretched over the limit.
        let huge = dir.path().join("huge.png");
        std::fs::copy(&icon, &huge).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&huge)
            .unwrap()
            .set_len(instances::MAX_ARTWORK + 1)
            .unwrap();
        assert!(artwork(&huge, 16., 2.).is_none());
        std::fs::write(dir.path().join("notes.png"), "not a picture").unwrap();
        assert!(artwork(&dir.path().join("notes.png"), 16., 2.).is_none());
        assert!(artwork(&dir.path().join("missing.png"), 16., 2.).is_none());
    }
}
