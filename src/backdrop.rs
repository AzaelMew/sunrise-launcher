//! The window's still backdrop. It never owns a timer. The cat and visible operations wake
//! their own UI when they change; reduced motion does not need to stop a scenery animation.
use crate::{backend, theme::Theme};
use gpui::{div, img, prelude::*, Div, ObjectFit, RenderImage, Window};
use std::sync::{mpsc::Receiver, Arc};
use sunrise_launcher::{pictures, themes};
use sunrise_launcher_scenery::{coast, sky::Sky};

#[derive(Default)]
pub(super) struct Backdrop {
    coast: Option<Arc<RenderImage>>,
    sky: Sky,
    /// The player's picture, by its saved name: the one picture kept, freed when another
    /// replaces it.
    photo: Option<(String, Photo)>,
    /// A picture replaced or turned off, until the window frees it.
    retired: Vec<Arc<RenderImage>>,
}

/// The player's picture is read, decoded and fitted on a worker ([`themes::load_background`]),
/// never in a frame: whatever size the file is, the window is given one no larger than the GPU
/// holds. Until it is here, and when it can't be made, the theme's own scenery shows.
enum Photo {
    Loading(Receiver<Option<image::RgbaImage>>),
    Ready(Arc<RenderImage>),
    Failed,
}

impl Backdrop {
    /// Takes a picture a worker finished. True when there is something new to draw.
    pub(super) fn poll(&mut self) -> bool {
        let Some((_, photo)) = &mut self.photo else {
            return false;
        };
        let Photo::Loading(made) = photo else {
            return false;
        };
        *photo = match made.try_recv() {
            Ok(Some(picture)) => {
                Photo::Ready(Arc::new(RenderImage::new([image::Frame::new(picture)])))
            }
            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => Photo::Failed,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
        };
        true
    }

    /// The saved picture `name`, once it is made. Starts making it the first time.
    fn photo(&mut self, name: Option<&String>) -> Option<Arc<RenderImage>> {
        if self.photo.as_ref().map(|(old, _)| old) != name {
            if let Some((_, Photo::Ready(old))) = self.photo.take() {
                self.retired.push(old);
            }
            self.photo = name.map(|name| (name.clone(), Photo::Loading(load(name.clone()))));
        }
        match &self.photo {
            Some((_, Photo::Ready(image))) => Some(image.clone()),
            _ => None,
        }
    }

    pub(super) fn render(
        &mut self,
        settings: &backend::Settings,
        theme: &Theme,
        window: &mut Window,
    ) -> Div {
        let page: gpui::Hsla = gpui::rgb(crate::paint::palette().background).into();
        let back = div().absolute().inset_0().overflow_hidden().bg(page);
        sunrise_launcher_scenery::clock::set_preference(settings.reduce_motion);
        let photo = self.photo(settings.background.as_ref().filter(|_| settings.scenery()));
        for old in self.retired.drain(..) {
            let _ = window.drop_image(old);
        }
        if !settings.scenery() {
            return back;
        }
        if let Some(photo) = photo {
            // Unplated titles have to read even on a white photo. Panels use opaque roles.
            return back
                .child(img(photo).size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(page.opacity(0.78)));
        }
        match themes::theme(settings.theme.as_deref()) {
            Some(t) if t.id == themes::DAWN_THEME.id => {
                back.child(img("dawn.webp").size_full().object_fit(ObjectFit::Cover))
            }
            None => back.child(
                img(self.coast.get_or_insert_with(coast::poster).clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            ),
            Some(_) => {
                self.sky.free_replaced(window);
                let size = window.viewport_size();
                back.child(self.sky.frame_at(
                    &theme.night,
                    (size.width.into(), size.height.into()),
                    window.scale_factor(),
                    Default::default(),
                    0.,
                ))
                .child(div().absolute().inset_0().bg(page.opacity(0.46)))
            }
        }
    }
}

/// Makes the saved picture `name` on a worker and wakes the window when it is done: None when
/// the file is gone, is no picture, or is too large to decode.
fn load(name: String) -> Receiver<Option<image::RgbaImage>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let picture = themes::background(&backend::data_dir(), Some(&name))
            .and_then(|path| themes::load_background(&path, pictures::gpu_limit()).ok())
            .map(|mut picture| {
                // GPUI wants BGRA, its alpha premultiplied.
                for px in picture.pixels_mut() {
                    let [r, g, b, a] = px.0;
                    let pre = |c: u8| (c as u32 * a as u32 / 255) as u8;
                    px.0 = [pre(b), pre(g), pre(r), a];
                }
                picture
            });
        let _ = tx.send(picture);
        crate::wake::wake();
    });
    rx
}
