//! What the launcher just did, said for 4.2 s (the mockup's `say`): on a wide window (1280 px
//! and more) a line in the top bar between the pages and the account, right-aligned at 70 %;
//! narrower, a glass toast along the window's bottom (`hzl-float`, 20 px corners).
//!
//! Any handler can say something, even one with only `&mut App`: the line waits here, wakes the
//! launcher's drain (`wake.rs`), which draws it, then lets it go.

use super::*;

use std::path::Path;
use std::sync::Mutex;


/// How long a line stays (`setTimeout(…, 4200)`).
const SHOWN: Duration = Duration::from_millis(4200);

/// A line's pieces: the words, and a path or command (`true`) set in mono.
type Parts = Vec<(String, bool)>;

/// The line showing: its pieces, when its 4.2 s started (moved on by the time it was held
/// under the pointer), its number (a new one restarts them), and since when it's held.
struct Line {
    parts: Parts,
    since: Instant,
    n: u64,
    held: Option<Instant>,
}

static LINE: Mutex<Option<Line>> = Mutex::new(None);

fn say_parts(parts: Parts) {
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    let n = line.as_ref().map_or(1, |l| l.n + 1);
    *line = Some(Line {
        parts,
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

fn current() -> Option<(String, u64)> {
    current_parts().map(|(parts, n)| {
        let text: Vec<_> = parts.iter().map(|(p, _)| p.trim()).collect();
        (text.join(" "), n)
    })
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
        let now = current().map(|l| l.1);
        let changed = now != self.toast_seen;
        self.toast_seen = now;
        changed
    }

}
