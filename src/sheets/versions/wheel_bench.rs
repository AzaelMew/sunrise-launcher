//! The `sunrise-launcher benchmark` hooks that scroll the build picker a wheel step per frame.
use super::*;

impl Launcher {
    /// The build list as `sunrise-launcher benchmark` finds it after a frame, and the bounds a
    /// wheel step goes over. Read between frames, never while one is built: a read inside one
    /// would make the list a dependency of the launcher's view and turn off the scroll-only
    /// reuse a player's wheel gets.
    ///
    /// Once `timed`, the rows are known to be drawn and only the list's place is read: working
    /// out the rows again (the profile's build is read from disk) would add work to every
    /// measured frame that a player's wheel doesn't cause.
    fn bench_target(&self, timed: bool) -> (perf::Target, gpui::Bounds<gpui::Pixels>) {
        use perf::Target;
        let scroll = self.ui.sheets.versions.scroll.0.borrow();
        let bounds = scroll.base_handle.bounds();
        let laid = || Target::Laid {
            offset: f32::from(scroll.base_handle.offset().y),
            max: f32::from(scroll.base_handle.max_offset().y),
        };
        let target = if self.ui.sheet != Some(Sheet::Versions) {
            Target::Closed
        } else if timed {
            laid()
        } else if let Some(instance) = self.selected_instance() {
            if matches!(self.releases, Load::Loading) {
                Target::Loading
            } else {
                let plays = self.profile_target(instance);
                let rows = self.version_lines(instance, plays.as_deref()).len();
                // The last frame laid the list out with every loaded row, at its height: not
                // the short list drawn before the releases arrived.
                let drawn = scroll.last_item_size.is_some_and(|size| {
                    (f32::from(size.contents.height) - rows as f32 * LINE_H).abs() < 0.5
                        && (f32::from(bounds.size.height) - self.versions_list_height(rows)).abs()
                            < 0.5
                });
                match (rows, drawn) {
                    (0, _) => Target::Empty,
                    (_, false) => Target::LayingOut,
                    (_, true) => laid(),
                }
            }
        } else {
            Target::NoProfile
        };
        (target, bounds)
    }
}

/// `sunrise-launcher benchmark`: after each frame, looks at the list it scrolls (the build
/// picker's, or Installed's: [`perf::Bench::list`]) and sends one wheel step over it through the
/// window's ordinary input path, so the list scrolls, caches and redraws as it does under a
/// player's wheel. Nothing is read inside a frame.
///
/// It keeps looking while the list loads and is laid out with the rows it brings; the
/// benchmark's timed phase only begins once they are drawn ([`perf::Bench::after_frame`]). With
/// no profile, no rows, a list that couldn't be read, or rows that fit, the phase ends there and
/// no more frames are asked for.
pub(crate) fn bench_wheel(launcher: gpui::WeakEntity<Launcher>, window: &mut Window) {
    window.on_next_frame(move |window, cx| {
        let Some(entity) = launcher.upgrade() else {
            return;
        };
        let this = entity.read(cx);
        let timed = this.bench.as_ref().is_some_and(perf::Bench::timing_scroll);
        let (target, bounds) = match this.bench.as_ref().map(perf::Bench::list) {
            Some(perf::List::Installed) => this.installed_bench_target(),
            _ => this.bench_target(timed),
        };
        let now = std::time::Instant::now();
        let step = entity.update(cx, |l, _| l.bench.as_mut()?.after_frame(now, target));
        if let Some(step) = step {
            window.dispatch_event(bench_wheel_event(bounds, step), cx);
        }
        let scrolling = entity
            .read(cx)
            .bench
            .as_ref()
            .is_some_and(perf::Bench::scrolling);
        if scrolling {
            bench_wheel(launcher, window);
        }
    });
}

/// The wheel event a benchmark sends: `step` pixels down (negative) or up, at the middle of
/// `bounds`.
fn bench_wheel_event(bounds: gpui::Bounds<gpui::Pixels>, step: f32) -> gpui::PlatformInput {
    gpui::PlatformInput::Scroll(gpui::ScrollEvent {
        position: bounds.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(step))),
        ..Default::default()
    })
}
