//! The window's root: the launcher, and behind it Home's scenery as a sibling view. A view's
//! frame dirties the views it sits in, so a scene drifting inside the launcher would draw the
//! whole launcher again on every frame of it. As siblings, a scene frame draws the scene and this
//! thin root; the launcher is drawn from its last frame (GPUI Fast retains views).
use crate::{coast, rain, sky, Launcher};
use gpui::{
    div, prelude::*, px, AnimationExt, Bounds, Context, Entity, Hsla, IntoElement, Pixels, Render,
    Window,
};
use std::path::PathBuf;

/// What Home's page shows between the sidebars: the theme's animated pixel scene, or the
/// player's own picture.
#[derive(Clone, PartialEq)]
pub enum Cover {
    /// Sunrise's coast at first light (Sunrise's own theme, and Dawn's in its colours).
    Coast(coast::Palette),
    /// The theme's painted sky; Hypr Neon's rains.
    Sky {
        rains: bool,
    },
    Picture(PathBuf),
}

/// What is behind the launcher, as its last frame asked for.
#[derive(Clone, PartialEq)]
pub struct Back {
    /// Behind everything.
    pub side: Hsla,
    /// Home's scenery and where it goes: the page between the sidebars, under the top bar.
    pub cover: Option<(Cover, Bounds<Pixels>)>,
}

pub struct Backdrop {
    back: Option<Back>,
    /// How many times the scenery came into view: it fades in with the page each time.
    arrivals: usize,
    coast: Entity<coast::Coast>,
    sky: Entity<sky::SkyView>,
    rain: Entity<rain::Rain>,
}

impl Backdrop {
    pub fn new(
        coast: Entity<coast::Coast>,
        sky: Entity<sky::SkyView>,
        rain: Entity<rain::Rain>,
    ) -> Self {
        Self {
            back: None,
            arrivals: 0,
            coast,
            sky,
            rain,
        }
    }
}

impl Render for Backdrop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let back = div().absolute().top_0().left_0().size_full();
        let Some(b) = self.back.clone() else {
            return back;
        };
        let back = back.bg(b.side);
        let Some((cover, at)) = b.cover else {
            return back;
        };
        let place = div()
            .absolute()
            .left(at.origin.x)
            .top(at.origin.y)
            .w(at.size.width)
            .h(at.size.height)
            .overflow_hidden();
        let place = match cover {
            Cover::Coast(_) => place.child(self.coast.clone()),
            Cover::Sky { rains } => place
                .child(self.sky.clone())
                .when(rains, |d| d.child(self.rain.clone())),
            Cover::Picture(path) => place.child(
                gpui::img(path)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            ),
        };
        // The page's colour rises over the scene's foot, so it settles into the page rather
        // than stopping at an edge, and the hero's text reads over its top.
        let veil = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(f32::from(at.size.height) * 0.18))
            .bg(gpui::linear_gradient(
                0.,
                gpui::linear_color_stop(b.side, 0.),
                gpui::linear_color_stop(b.side.opacity(0.), 1.),
            ));
        // It arrives with Home, fading in as the page does.
        back.child(place.child(veil).with_animation(
            gpui::ElementId::named_usize("scenery-in", self.arrivals),
            crate::motion::eased(crate::motion::PAGE),
            |d, p| d.opacity(p),
        ))
    }
}

pub struct Shell {
    backdrop: Entity<Backdrop>,
    launcher: Entity<Launcher>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let launcher = cx.new(|cx| Launcher::new(window, cx));
        let backdrop = launcher.read(cx).backdrop.clone();
        Self { backdrop, launcher }
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(self.backdrop.clone())
            // Cached: a frame of the scenery redraws the backdrop, never the launcher, which is
            // drawn again only when it is notified.
            .child(
                self.launcher
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
    }
}
