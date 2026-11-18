//! The window's still backdrop. It never owns a timer. The cat and visible operations wake
//! their own UI when they change; reduced motion does not need to stop a scenery animation.
use crate::{backend, theme::Theme};
use gpui::{div, img, prelude::*, Div, ObjectFit, RenderImage, Window};
use std::{path::PathBuf, sync::Arc};
use sunrise_launcher::themes;
use sunrise_launcher_scenery::{coast, sky::Sky};

#[derive(Default)]
pub(super) struct Backdrop {
    coast: Option<Arc<RenderImage>>,
    sky: Sky,
    photo: Option<(String, Option<PathBuf>)>,
}

impl Backdrop {
    pub(super) fn render(
        &mut self,
        settings: &backend::Settings,
        theme: &Theme,
        window: &mut Window,
    ) -> Div {
        let page: gpui::Hsla = gpui::rgb(crate::paint::palette().background).into();
        let back = div().absolute().inset_0().overflow_hidden().bg(page);
        if !settings.scenery() {
            return back;
        }
        sunrise_launcher_scenery::clock::set_preference(settings.reduce_motion);
        if let Some(name) = &settings.background {
            if self.photo.as_ref().is_none_or(|(old, _)| old != name) {
                self.photo = Some((
                    name.clone(),
                    themes::background(&backend::data_dir(), Some(name)),
                ));
            }
            if let Some((_, Some(path))) = &self.photo {
                // Unplated titles have to read even on a white photo. Panels use opaque roles.
                return back
                    .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
                    .child(div().absolute().inset_0().bg(page.opacity(0.78)));
            }
        } else {
            self.photo = None;
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
