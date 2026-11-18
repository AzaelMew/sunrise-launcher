//! Finite, interruptible motion for changes of place. Deferred menus cannot drive frames in
//! GPUI Fast; animate their ordinary overlay instead. Reduced motion skips every frame request.
use gpui::{
    px, Animation, AnimationExt, AnyElement, App, Div, ElementId, IntoElement, Styled, Window,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;
use sunrise_launcher_theme as tokens;

pub(super) const FAST: tokens::Motion = tokens::Motion {
    name: "surface-fast",
    ms: 120,
    curve: tokens::MOVE.curve,
};
pub(super) const STANDARD: tokens::Motion = tokens::Motion {
    name: "surface-standard",
    ms: 180,
    curve: tokens::MOVE.curve,
};
pub(super) const LIFT: f32 = 6.;
pub(super) const ANCHOR: f32 = 3.;

/// Arriving controls are readable and usable on the first frame.
pub(super) fn arrival_opacity(progress: f32) -> f32 {
    0.85 + 0.15 * progress
}

#[derive(Clone, Copy)]
pub(super) struct Tween {
    started: Instant,
    from: f32,
    to: f32,
    motion: tokens::Motion,
}

impl Tween {
    pub(super) fn new(started: Instant, from: f32, to: f32, motion: tokens::Motion) -> Self {
        Self {
            started,
            from,
            to,
            motion,
        }
    }

    /// `None` means final value: the caller must neither retain the tween nor ask for a frame.
    fn running(self, now: Instant, enabled: bool) -> Option<f32> {
        let duration = self.motion.duration().mul_f32((self.to - self.from).abs());
        let elapsed = now.saturating_duration_since(self.started);
        (enabled && elapsed < duration).then(|| {
            self.from
                + (self.to - self.from)
                    * self
                        .motion
                        .ease(elapsed.as_secs_f32() / duration.as_secs_f32())
        })
    }

    pub(super) fn value(self, now: Instant, enabled: bool) -> f32 {
        self.running(now, enabled).unwrap_or(self.to)
    }
}

pub(super) struct Frame {
    pub(super) now: Instant,
    pub(super) enabled: bool,
    requested: Cell<bool>,
}

impl Frame {
    pub(super) fn new(window: &Window, cx: &App) -> Self {
        Self {
            now: Instant::now(),
            enabled: window.is_visible() && window.is_window_active() && !cx.reduce_motion(),
            requested: Cell::new(false),
        }
    }

    /// Samples all surfaces on one clock and coalesces their requests into one next frame.
    pub(super) fn sample(&self, tween: &mut Option<Tween>, window: &Window) -> Option<f32> {
        self.sample_and_request(tween, || window.request_animation_frame())
    }

    fn sample_and_request(&self, tween: &mut Option<Tween>, request: impl FnOnce()) -> Option<f32> {
        let sample = tween.and_then(|tween| tween.running(self.now, self.enabled));
        if sample.is_none() {
            *tween = None;
        } else if !self.requested.replace(true) {
            request();
        }
        sample
    }
}

pub(super) fn sync(settings: &crate::backend::Settings, cx: &mut App) {
    crate::clock::set_preference(settings.reduce_motion);
    cx.set_reduce_motion(still());
}

/// A switch's knob (`transition-[translate] duration-200`, Tailwind's default curve).
const SLIDE: tokens::Motion = tokens::Motion {
    name: "slide",
    ms: 200,
    curve: tokens::QUICK.curve,
};

/// Whether the player's system asks for reduced motion (or `SUNRISE_LAUNCHER_MOTION=off`).
pub fn still() -> bool {
    crate::clock::motion() == crate::clock::Motion::Still
}

fn eased(motion: tokens::Motion) -> Animation {
    Animation::new(motion.duration()).with_easing(move |t| motion.ease(t))
}

/// A hover easing on (to 1) or off (to 0) on its motion's curve, from wherever the last one
/// had got to: a pointer that leaves halfway turns it around there, instead of the element
/// jumping to fully lit first. The rest of the way takes its share of the motion's length.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    pub on: bool,
    from: f32,
    at: Instant,
    /// The motion it plays on, as it was last drawn with.
    pub motion: tokens::Motion,
}

impl Fade {
    /// Easing towards `on` from where `last` is now; from rest when there was none.
    pub fn toward(last: Option<Fade>, on: bool, now: Instant) -> Fade {
        Fade::toward_as(last, on, now, still())
    }

    /// [`Fade::toward`] with the reduced-motion preference given, not read.
    fn toward_as(last: Option<Fade>, on: bool, now: Instant, still: bool) -> Fade {
        let (from, motion) = match last {
            Some(last) => (last.value_as(now, still), last.motion),
            None => (if on { 0. } else { 1. }, tokens::QUICK),
        };
        Fade {
            on,
            from,
            at: now,
            motion,
        }
    }

    fn target(self) -> f32 {
        if self.on {
            1.
        } else {
            0.
        }
    }

    /// How long it plays: the motion's length, for the share of the way left.
    fn length(self) -> std::time::Duration {
        self.motion
            .duration()
            .mul_f32((self.target() - self.from).abs().min(1.))
    }

    /// The strength at `now`: 0 at rest, 1 lit (more, briefly, on an overshooting curve); `still`
    /// (reduced motion) holds it at its target.
    fn value_as(self, now: Instant, still: bool) -> f32 {
        let length = self.length().as_secs_f32();
        if length <= 0. || still {
            return self.target();
        }
        let t = (now.saturating_duration_since(self.at).as_secs_f32() / length).min(1.);
        self.from + (self.target() - self.from) * self.motion.ease(t)
    }

    /// Ended off: nothing is left to draw.
    pub fn rested(self, now: Instant) -> bool {
        !self.on && now.saturating_duration_since(self.at) >= self.length()
    }

    /// `el` drawn by `paint` at this fade's strength, which GPUI plays from here on. `key`
    /// names the element; each fade plays under its own animation id.
    pub fn play<E: IntoElement + 'static>(
        self,
        key: &gpui::SharedString,
        el: E,
        paint: impl Fn(E, f32) -> E + 'static,
    ) -> AnyElement {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = *START.get_or_init(Instant::now);
        let id = ElementId::named_usize(
            key.clone(),
            self.at.saturating_duration_since(start).as_micros() as usize,
        );
        let (from, to, motion) = (self.from, self.target(), self.motion);
        let length = self.length();
        if still() || self.at.elapsed() >= length {
            return paint(el, to).into_any_element();
        }
        let animation = Animation::new(length).with_easing(move |t| motion.ease(t));
        el.with_animation(id, animation, move |d, p| paint(d, from + (to - from) * p))
            .into_any_element()
    }
}

thread_local! {
    /// Each switch's last state, and when it last changed.
    static SWITCHES: RefCell<HashMap<String, (bool, Option<Instant>)>> = RefCell::new(HashMap::new());
}

/// When switch `id`'s knob started sliding towards `on`, while it still slides: a switch seen
/// for the first time, or changed long ago, is already there.
fn slide(id: &str, on: bool, now: Instant, animate: bool) -> Option<Instant> {
    SWITCHES.with_borrow_mut(|switches| {
        let seen = switches.entry(id.to_string()).or_insert((on, None));
        if !animate {
            *seen = (on, None);
        } else if seen.0 != on {
            *seen = (on, Some(now));
        }
        seen.1
            .filter(|since| now.duration_since(*since) < SLIDE.duration())
    })
}

/// While switch `id` slides towards `on`: a number for this slide, to give each of its
/// animations its own id so they play once, together.
pub fn sliding(id: &str, on: bool) -> Option<usize> {
    let since = slide(id, on, Instant::now(), !still())?;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    Some(since.saturating_duration_since(start).as_micros() as usize)
}

/// How a switch slides after [`sliding`] confirmed that motion is allowed.
pub fn slide_motion() -> Animation {
    eased(SLIDE)
}

/// A switch's knob at `left` px when off and `left + travel` when on, sliding there when the
/// switch `id` changed in the last 200 ms.
#[cfg_attr(not(windows), allow(dead_code))] // the Windows setup window
pub fn knob(id: &str, on: bool, knob: Div, left: f32, travel: f32) -> AnyElement {
    let at = move |p: f32| left + travel * if on { p } else { 1. - p };
    match slide(id, on, Instant::now(), !still()) {
        Some(since) => {
            // Each slide gets its own animation id, from when it started, so it plays once.
            static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let start = *START.get_or_init(Instant::now);
            let key = since.saturating_duration_since(start).as_micros() as usize;
            knob.with_animation(
                ElementId::named_usize(format!("{id}-knob"), key),
                eased(SLIDE),
                move |d, p| d.left(px(at(p))),
            )
            .into_any_element()
        }
        None => knob.left(px(at(1.))).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_hover_left_halfway_turns_around_where_it_got_to() {
        let linear = tokens::Motion {
            name: "linear",
            ms: 100,
            curve: [0., 0., 1., 1.],
        };
        let at = Instant::now();
        let ms = |n: u64| at + std::time::Duration::from_millis(n);
        let on = Fade {
            motion: linear,
            ..Fade::toward_as(None, true, at, false)
        };
        assert!((on.value_as(ms(50), false) - 0.5).abs() < 1e-3);
        assert!(!on.rested(ms(1000)), "a lit element stays lit");
        // The pointer leaves halfway: from 0.5 back to rest, over half the length.
        let off = Fade::toward_as(Some(on), false, ms(50), false);
        assert!((off.value_as(ms(50), false) - 0.5).abs() < 1e-3);
        assert!((off.value_as(ms(75), false) - 0.25).abs() < 1e-3);
        assert!(!off.rested(ms(99)));
        assert!(off.rested(ms(100)));
        // Back on before it rested: from where it was, not from 0.
        let again = Fade::toward_as(Some(off), true, ms(75), false);
        assert!((again.value_as(ms(75), false) - 0.25).abs() < 1e-3);
        assert!((again.value_as(ms(150), false) - 1.).abs() < 1e-3);
    }

    #[test]
    fn surfaces_request_frames_only_during_their_ease_out() {
        let start = Instant::now();
        for (token, ms) in [(FAST, 120), (STANDARD, 180)] {
            let tween = Tween::new(start, 0., 1., token);
            assert_eq!(tween.running(start, true), Some(0.));
            assert!(tween.value(start + Duration::from_millis(ms / 2), true) > 0.5);
            assert!(tween
                .running(start + Duration::from_millis(ms - 1), true)
                .is_some());
            assert_eq!(tween.running(start + Duration::from_millis(ms), true), None);
            assert_eq!(tween.running(start, false), None);
            assert_eq!(tween.value(start, false), 1.);
        }
    }

    #[test]
    fn reversing_a_width_tween_keeps_its_current_value_and_shortens_the_return() {
        let start = Instant::now();
        let flip = start + Duration::from_millis(35);
        let expanded = Tween::new(start, 0., 1., STANDARD).value(flip, true);
        let closing = Tween::new(flip, expanded, 0., STANDARD);
        assert_eq!(closing.running(flip, true), Some(expanded));
        let halfway_back = flip + STANDARD.duration().mul_f32(expanded / 2.);
        assert!(closing.running(halfway_back, true).unwrap() < expanded);
        assert_eq!(
            closing.running(flip + STANDARD.duration().mul_f32(expanded), true),
            None
        );
    }

    #[test]
    fn frame_demand_is_coalesced_and_ends_with_the_last_surface() {
        let now = Instant::now();
        let mut fast = Some(Tween::new(now, 0., 1., FAST));
        let mut standard = Some(Tween::new(now, 0., 1., STANDARD));
        let calls = Cell::new(0);
        let request = || calls.set(calls.get() + 1);
        for (elapsed, enabled, expected) in [(0, true, 1), (120, true, 2), (180, true, 2)] {
            let frame = Frame {
                now: now + Duration::from_millis(elapsed),
                enabled,
                requested: Cell::new(false),
            };
            frame.sample_and_request(&mut fast, request);
            frame.sample_and_request(&mut standard, request);
            assert_eq!(calls.get(), expected);
        }
        assert!(fast.is_none() && standard.is_none());
        let mut active = Some(Tween::new(now, 0., 1., STANDARD));
        let reduced = Frame {
            now,
            enabled: false,
            requested: Cell::new(false),
        };
        assert_eq!(reduced.sample_and_request(&mut active, request), None);
        assert!(active.is_none());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_switch_slides_only_after_it_changed() {
        let t0 = Instant::now();
        assert_eq!(
            slide("test-a", true, t0, true),
            None,
            "first seen: already there"
        );
        let flip = t0 + Duration::from_millis(100);
        assert_eq!(slide("test-a", false, flip, true), Some(flip));
        let midway = flip + Duration::from_millis(50);
        assert_eq!(slide("test-a", false, midway, true), Some(flip));
        assert_eq!(slide("test-a", false, midway, false), None);
        assert_eq!(
            slide("test-a", false, midway, true),
            None,
            "do not replay after reducing motion"
        );
    }
}
