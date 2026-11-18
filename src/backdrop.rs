//! The window's still backdrop. It never owns a timer. The cat and visible operations wake
//! their own UI when they change; reduced motion does not need to stop a scenery animation.
use crate::settings::HomePhoto;
use crate::{backend, theme::Theme};
use gpui::{div, img, prelude::*, Div, ObjectFit, RenderImage, Window};
use std::sync::Arc;
use sunrise_launcher::themes;
use sunrise_launcher_scenery::{coast, sky::Sky};

#[derive(Default)]
pub(super) struct Backdrop {
    coast: Option<Arc<RenderImage>>,
    sky: Sky,
}

impl Backdrop {
    pub(super) fn render(
        &mut self,
        settings: &backend::Settings,
        theme: &Theme,
        photo: Option<HomePhoto>,
        window: &mut Window,
    ) -> Div {
        let page: gpui::Hsla = gpui::rgb(crate::paint::palette().background).into();
        let back = div().absolute().inset_0().overflow_hidden().bg(page);
        sunrise_launcher_scenery::clock::set_preference(settings.reduce_motion);
        if !settings.scenery() {
            return back;
        }
        if let Some(photo) = photo {
            let picture = match photo {
                HomePhoto::Personal(path) => img(path),
                HomePhoto::Theme(decoded) => img(decoded),
            };
            // Unplated titles have to read even on a white photo. Panels use opaque roles.
            return back
                .child(picture.size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().inset_0().bg(page.opacity(0.78)));
        }
        // A player's theme, there or missing, stands in Dawn's scenery.
        let scene = match themes::resolve(settings.theme.as_deref()) {
            themes::Resolved::Builtin(t) => Some(t.id),
            themes::Resolved::Local(_) | themes::Resolved::Missing => Some(themes::DAWN_THEME.id),
            themes::Resolved::Sunrise => None,
        };
        match scene {
            Some(id) if id == themes::DAWN_THEME.id => {
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
