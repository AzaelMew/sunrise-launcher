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

/// The room Home's hero takes at the top of the page (width, height), which drifting snow,
/// leaves, petals and rain stay out of.
const HERO_KEEP: (f32, f32) = (520., 440.);

impl Launcher {
    /// Hands the backdrop what goes behind the launcher: the page's colour, and on Home its
    /// scenery where the page between the sidebars was laid out. Each view draws again only
    /// when what it is handed changed.
    fn push_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = self.theme;
        // Across, Home's page lies between the sidebars, which this frame knows; only its top
        // and height come from the page as laid out, the same on every page. The frame that
        // switches to Home has the last page's width laid out, which the scene would take for
        // a frame and then be baked again.
        let side = self.outline_side_width();
        let at = self.cover_at.get().map(|at| {
            gpui::Bounds::new(
                gpui::point(px(side), at.origin.y),
                gpui::size(px(self.window_width - 2. * side), at.size.height),
            )
        });
        let cover = self.home_cover().zip(at);
        let scale = window.scale_factor();
        let mut pushed = Pushed {
            back: window_root::Back {
                side: t.side,
                cover: cover.clone(),
            },
            coast: None,
            sky: None,
            rain: None,
        };
        if let Some((cover, at)) = &cover {
            let size = (f32::from(at.size.width), f32::from(at.size.height));
            match cover {
                window_root::Cover::Coast(palette) => pushed.coast = Some((*palette, size, scale)),
                window_root::Cover::Sky { rains } => {
                    // Snow, leaves, petals and rain keep off the hero: its column, from the
                    // edition tabs down to the invite code.
                    let mut keep = rain::Keep::default();
                    keep.0[0] = [size.0 / 2. - HERO_KEEP.0 / 2., 0., HERO_KEEP.0, HERO_KEEP.1];
                    pushed.sky = Some((t.night, size, scale, keep));
                    let rains = *rains && !self.settings.performance();
                    pushed.rain = Some((rains, self.look.accent, keep));
                }
                window_root::Cover::Picture(_) => {}
            }
        }
        if self.pushed.as_ref() == Some(&pushed) {
            return;
        }
        self.pushed = Some(pushed.clone());
        // Handed over after this frame: a view that touches an entity while it renders is drawn
        // again whenever that entity is, and the scenery steps a few times a second.
        let (backdrop, coast, sky, rain) = (
            self.backdrop.clone(),
            self.coast.clone(),
            self.sky.clone(),
            self.rain.clone(),
        );
        cx.defer(move |cx| {
            if let Some((palette, size, scale)) = pushed.coast {
                coast.update(cx, |c, cx| c.set(palette, size, scale, cx));
            }
            if let Some((night, size, scale, keep)) = pushed.sky {
                sky.update(cx, |s, cx| s.set(night, size, scale, keep, cx));
            }
            if let Some((on, accent, keep)) = pushed.rain {
                rain.update(cx, |r, cx| r.set(on, accent, keep, cx));
            }
            backdrop.update(cx, |b, cx| b.set(pushed.back, cx));
        });
    }

    /// Records where the page between the sidebars is laid out, for Home's scenery: a frame
    /// later when it moved (a resize, a sidebar folding).
    fn measure_cover(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (at, this) = (self.cover_at.clone(), cx.entity().downgrade());
        gpui::canvas(
            move |bounds, _, cx| {
                if at.get() != Some(bounds) {
                    at.set(Some(bounds));
                    let this = this.clone();
                    cx.defer(move |cx| {
                        let _ = this.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Where the opening is: the reveal's progress (1 when there is no opening) and how long
    /// the wordmark has been showing, in seconds.
    pub(crate) fn opening_phase(&mut self, window: &mut Window) -> (f32, f32) {
        // Reduced motion: the wordmark's arrival and the reveal are all motion, so no opening.
        if motion::still() {
            self.opening = None;
        }
        let loaded = !matches!(self.releases, Load::Loading);
        let Some(opening) = &mut self.opening else {
            return (1., 10.);
        };
        let held = opening.started.get_or_insert_with(Instant::now).elapsed();
        if opening.reveal.is_none() && ((held >= OPENING_HOLD && loaded) || held >= OPENING_MAX) {
            opening.reveal = Some(Instant::now());
        }
        let reveal = opening.reveal.map_or(0., |at| {
            at.elapsed().as_secs_f32() / OPENING_REVEAL.as_secs_f32()
        });
        if reveal >= 1. {
            self.opening = None;
            return (1., 10.);
        }
        window.request_animation_frame();
        (reveal, held.as_secs_f32())
    }

    /// `sunrise-launcher benchmark`: scroll the page a little on every frame, back to the top at
    /// the bottom, so the frame times measure scrolling.
    pub(crate) fn bench_scroll(&mut self, window: &mut Window) {
        let handle = &self.content_scroll.handle;
        let mut offset = handle.offset();
        offset.y -= px(24.);
        if -offset.y > handle.max_offset().y {
            offset.y = px(0.);
        }
        handle.set_offset(offset);
        window.request_animation_frame();
    }

    pub(crate) fn skip_opening(&mut self) {
        if let Some(opening) = &mut self.opening {
            opening.reveal.get_or_insert_with(Instant::now);
        }
    }
}
