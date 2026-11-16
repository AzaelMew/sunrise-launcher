//! How the scenery moves: the player's or the system's reduced motion ([`motion`]), and the
//! animation's own [`Clock`], which stops while the window is behind others.
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How the scenery moves: not at all under reduced motion (the player's choice in Settings, else the
/// system's: Windows' "Show animations", macOS's "Reduce motion") or with
/// `SUNRISE_LAUNCHER_MOTION=off`; while the window is in front (and 10 s after); or always, with
/// `SUNRISE_LAUNCHER_MOTION=always`, for recordings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Motion {
    Still,
    InFront,
    Always,
}

/// The player's choice (`Settings::reduce_motion`): 0 follows the system, 1 still, 2 moves.
static PREFERENCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Takes the player's choice: `None` follows the system, `Some(true)` holds everything still.
pub fn set_preference(reduce: Option<bool>) {
    let v = match reduce {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    PREFERENCE.store(v, std::sync::atomic::Ordering::Relaxed);
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
    if let Some(forced) = *FORCED {
        return forced;
    }
    let still = match PREFERENCE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => system_reduces_motion(),
    };
    if still {
        Motion::Still
    } else {
        Motion::InFront
    }
}

/// The system's reduced motion, asked again at most every 2 s, so turning it on or off applies
/// without restarting the launcher.
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
fn ask_system() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REDUCE: AtomicBool = AtomicBool::new(false);
    static ASKING: AtomicBool = AtomicBool::new(false);
    if !ASKING.swap(true, Ordering::AcqRel) {
        std::thread::spawn(|| {
            let off = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "enable-animations"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false");
            REDUCE.store(off, Ordering::Relaxed);
            ASKING.store(false, Ordering::Release);
        });
    }
    REDUCE.load(Ordering::Relaxed)
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
