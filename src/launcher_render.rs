//! The launcher's frame (`Render for Launcher`) and its opening.
use super::*;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.alt_frame(window, cx)
    }
}

/// What the launcher last handed the views behind it: the backdrop, and the scene that shows.
#[derive(Clone, PartialEq)]
pub(crate) struct Pushed {
    back: window_root::Back,
    coast: Option<(sunrise_launcher_scenery::coast::Palette, (f32, f32), f32)>,
    sky: Option<(
        sunrise_launcher_theme::night::Night,
        (f32, f32),
        f32,
        rain::Keep,
    )>,
    rain: Option<(bool, u32, rain::Keep)>,
}

