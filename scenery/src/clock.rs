//! How the scenery moves: the player's or the system's reduced motion ([`motion`]), and the
//! animation's own [`Clock`], which stops while the window is behind others.
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How the scenery moves: not at all if either the launcher or the system asks to reduce motion, or with
/// `SUNRISE_LAUNCHER_MOTION=off`; while the window is in front (and 10 s after); or always, with
/// `SUNRISE_LAUNCHER_MOTION=always`, for recordings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Motion {
    Still,
    InFront,
    Always,
}

/// Whether the player independently asks to reduce motion in the launcher.
static REDUCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A saved `false` or no preference follows the system. Neither can override OS reduction.
pub fn set_preference(reduce: Option<bool>) {
    REDUCE.store(
        reduce.unwrap_or(false),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn motion() -> Motion {
    static FORCED: LazyLock<Option<Motion>> =
        LazyLock::new(
            || match std::env::var("SUNRISE_LAUNCHER_MOTION").as_deref() {
                Ok("off" | "reduced") => Some(Motion::Still),
                Ok("always") => Some(Motion::Always),
                Ok("full") => Some(Motion::InFront),
                _ => None,
            },
        );
    let launcher = REDUCE.load(std::sync::atomic::Ordering::Relaxed);
    let system = !launcher && *FORCED != Some(Motion::Still) && system_reduces_motion();
    resolve(launcher, system, *FORCED)
}

fn resolve(launcher: bool, system: bool, forced: Option<Motion>) -> Motion {
    if launcher || system {
        Motion::Still
    } else {
        forced.unwrap_or(Motion::InFront)
    }
}

/// The system's reduced motion, asked again at most every 2 s, so turning it on or off applies
/// without restarting the launcher.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn system_reduces_motion() -> bool {
    static ASKED: std::sync::Mutex<Option<(Instant, bool)>> = std::sync::Mutex::new(None);
    let mut asked = ASKED.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    match *asked {
        Some((at, reduce)) if now.duration_since(at) < Duration::from_secs(2) => reduce,
        _ => {
            let reduce = ask_system();
            *asked = Some((now, reduce));
            reduce
        }
    }
}

#[cfg(target_os = "windows")]
fn ask_system() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut animate: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL through the pointer it is given.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut animate as *mut i32).cast(),
            0,
        )
    };
    ok != 0 && animate == 0
}

#[cfg(target_os = "macos")]
fn ask_system() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// GNOME's "Animations" switch (`org.gnome.desktop.interface enable-animations`), read by
/// `gsettings` on a worker thread: the answer lands for the next ask, and a desktop without the
/// key or the tool answers "moves".
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn system_reduces_motion() -> bool {
    use std::sync::Mutex;
    struct Preference {
        asked: Option<Instant>,
        pending: bool,
        reduce: bool,
    }
    static PREF: Mutex<Preference> = Mutex::new(Preference {
        asked: None,
        pending: false,
        reduce: true, // Until the first answer, don't animate against a possible OS preference.
    });
    let mut pref = PREF.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if !pref.pending
        && pref
            .asked
            .is_none_or(|at| now - at >= Duration::from_secs(2))
    {
        pref.asked = Some(now);
        pref.pending = true;
        std::thread::spawn(|| {
            let reduce = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "enable-animations"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false");
            let mut pref = PREF.lock().unwrap_or_else(|e| e.into_inner());
            pref.reduce = reduce;
            pref.pending = false;
        });
    }
    pref.reduce
}

/// The animation's own time: it runs only while the scenery moves, so a window that comes back
/// picks up where it stopped instead of jumping.
#[derive(Default)]
pub struct Clock {
    banked: Duration,
    running: Option<Instant>,
}

impl Clock {
    pub fn at(&self, now: Instant) -> Duration {
        self.banked + self.running.map_or(Duration::ZERO, |since| now - since)
    }

    /// Whether the clock is running.
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    pub fn set(&mut self, run: bool, now: Instant) {
        match (run, self.running) {
            (true, None) => self.running = Some(now),
            (false, Some(since)) => {
                self.banked += now - since;
                self.running = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_preference_takes_priority_over_even_recording_motion() {
        assert_eq!(resolve(false, false, None), Motion::InFront);
        assert_eq!(resolve(false, false, Some(Motion::Always)), Motion::Always);
        assert_eq!(resolve(false, false, Some(Motion::Still)), Motion::Still);
        for (launcher, system) in [(true, false), (false, true), (true, true)] {
            assert_eq!(
                resolve(launcher, system, Some(Motion::Always)),
                Motion::Still
            );
        }
    }

    #[test]
    fn the_clock_stops_and_resumes_without_a_jump() {
        let start = Instant::now();
        let mut clock = Clock::default();
        clock.set(true, start);
        let s = Duration::from_secs(1);
        clock.set(false, start + 2 * s);
        assert_eq!(clock.at(start + 10 * s), 2 * s);
        clock.set(true, start + 10 * s);
        assert_eq!(clock.at(start + 11 * s), 3 * s);
    }
}
