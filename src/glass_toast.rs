//! What the launcher just did, said for 4.2 s in a floating toast. It doesn't take
//! space from navigation or steal focus. Hover pauses the dwell; a click dismisses it.
//!
//! Any handler can say something, even one with only `&mut App`: the line waits here, wakes the
//! launcher's drain (`wake.rs`), which draws it, then lets it go.

use super::*;

use crate::{glass_ui, motion, paint::palette};
use gpui::rgb;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How long a line stays (`setTimeout(…, 4200)`).
const SHOWN: Duration = Duration::from_millis(4200);

/// A line's pieces: the words, and a path or command (`true`) set in mono.
type Parts = Arc<[(String, bool)]>;

/// The line showing: its pieces, when its 4.2 s started (moved on by the time it was held
/// under the pointer), its number (a new one restarts them), and since when it's held.
struct Line {
    parts: Parts,
    since: Instant,
    n: u64,
    held: Option<Instant>,
}

static LINE: Mutex<Option<Line>> = Mutex::new(None);

#[derive(Default)]
pub(super) struct Presentation {
    drawn: Option<(Parts, u64)>,
    tween: Option<motion::Tween>,
    live: bool,
}

fn say_parts(parts: Vec<(String, bool)>) {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    *line = Some(Line {
        parts: parts.into(),
        since: Instant::now(),
        n,
        held: None,
    });
    crate::wake::wake();
}

/// Says `text` now, in place of whatever was said before.
pub(super) fn say(text: impl Into<String>) {
    say_parts(vec![(text.into(), false)]);
}

/// Says `en` translated, its one `{hole}` filled with `value` in mono, wherever the language
/// puts it.
pub(super) fn say_with(en: &str, value: &str) {
    let said = tr(en);
    let parts = match (said.find('{'), said.find('}')) {
        (Some(a), Some(b)) if a < b => [
            (said[..a].to_string(), false),
            (value.to_string(), true),
            (said[b + 1..].to_string(), false),
        ]
        .into_iter()
        .filter(|(p, _)| !p.trim().is_empty())
        .collect(),
        _ => vec![(said.to_string(), false)],
    };
    say_parts(parts);
}

/// The line still showing, and its number.
fn current_parts() -> Option<(Parts, u64)> {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    if line
        .as_ref()
        .is_some_and(|l| l.held.is_none() && l.since.elapsed() >= SHOWN)
    {
        *line = None;
    }
    line.as_ref().map(|l| (l.parts.clone(), l.n))
}

fn hold(n: u64, hovered: bool) {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(line) = line.as_mut().filter(|line| line.n == n) {
        match (hovered, line.held) {
            (true, None) => line.held = Some(Instant::now()),
            (false, Some(start)) => {
                line.since += start.elapsed();
                line.held = None;
            }
            _ => {}
        }
    }
}

/// `/Users/me/x` as `~/x`, as the mockup writes folders.
pub(super) fn tilde(path: &Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

/// Opens `path` in the system's file manager (or its app), and says so.
pub(super) fn open(cx: &mut App, path: &Path) {
    cx.open_with_system(path);
    say_with("Opened {path}", &tilde(path));
}

impl Launcher {
    /// From the drain: whether the line changed (a new one, or the last one gone).
    pub(super) fn toast_tick(&mut self) -> bool {
        // The cache cleanup after a start or a build says what it freed.
        if let Some(freed) = sunrise_launcher::storage::eviction::take_toast() {
            say(freed);
        }
        let now = current_parts().map(|(_, n)| n);
        let changed = now != self.toast_seen;
        self.toast_seen = now;
        changed
    }

    pub(super) fn toast(&mut self, time: &motion::Frame, window: &Window) -> Option<Div> {
        let current = current_parts();
        self.toast_seen = current.as_ref().map(|(_, id)| *id);
        let shown = &mut self.ui.toast;
        if let Some(line) = current {
            if shown.drawn.as_ref().is_none_or(|(_, old)| *old != line.1) {
                shown.drawn = Some(line);
                shown.tween = Some(motion::Tween::new(time.now, 0., 1., motion::FAST));
            }
            shown.live = true;
        } else if shown.live {
            shown.live = false;
            let p = shown
                .tween
                .map_or(1., |tween| tween.value(time.now, time.enabled));
            shown.tween = Some(motion::Tween::new(
                time.now,
                motion::arrival_opacity(p),
                0.,
                motion::FAST,
            ));
        }
        let progress = time.sample(&mut shown.tween, window);
        if !shown.live && progress.is_none() {
            shown.drawn = None;
            return None;
        }
        let live = shown.live;
        let (alpha, lift) = if live {
            let p = progress.unwrap_or(1.);
            (motion::arrival_opacity(p), motion::LIFT * (1. - p))
        } else {
            (progress.unwrap_or(0.), 0.)
        };
        let (parts, n) = shown.drawn.as_ref()?;
        let n = *n;
        let p = palette();
        let mut line = div()
            .flex()
            .flex_wrap()
            .items_baseline()
            .gap_x(px(5.))
            .min_w_0();
        for (text, mono) in parts.iter() {
            line = line.child(
                div()
                    .max_w_full()
                    .truncate()
                    .when(*mono, |d| d.font_family(sunrise_launcher_theme::MONO))
                    .child(text.trim().to_string()),
            );
        }
        Some(
            div()
                .absolute()
                .left(px(tokens::FRAME_INSET))
                .right(px(tokens::FRAME_INSET))
                .bottom(px(tokens::FRAME_INSET - lift))
                .flex()
                .justify_end()
                .child(
                    div()
                        .id(ElementId::named_usize("toast", n as usize))
                        .opacity(alpha)
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .max_w(px(
                            (self.window_width - 2. * tokens::FRAME_INSET).clamp(0., 560.)
                        ))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(tokens::R_PANEL))
                        .bg(rgb(p.raised))
                        .text_color(rgb(p.text))
                        .text_size(px(tokens::LABEL))
                        .border_1()
                        .border_color(rgb(p.line))
                        .shadow_lg()
                        .when(live, |d| {
                            d.occlude()
                                .cursor_pointer()
                                .on_hover(move |hovered, _, _| hold(n, *hovered))
                                .on_click(move |_, window, _| {
                                    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
                                    if line.as_ref().is_some_and(|line| line.n == n) {
                                        *line = None;
                                        window.refresh();
                                    }
                                })
                        })
                        .child(glass_ui::icon("tick-02", 16., rgb(p.accent_ink).into()).flex_none())
                        .child(line),
                ),
        )
    }
}
