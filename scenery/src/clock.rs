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

#[repr(u8)]
#[derive(Clone, Copy)]
enum Preference {
    System = 0,
    Still = 1,
    Full = 2,
    ReducedGraphics = 3,
}

impl Preference {
    fn choose(reduce: Option<bool>, reduced_graphics: bool) -> Self {
        match (reduce, reduced_graphics) {
            (_, true) => Self::ReducedGraphics,
            (None, false) => Self::System,
            (Some(true), false) => Self::Still,
            (Some(false), false) => Self::Full,
        }
    }

    fn resolve(self, forced: Option<Motion>, system: impl FnOnce() -> bool) -> Motion {
        if matches!(self, Self::ReducedGraphics) {
            return Motion::Still;
        }
        if let Some(forced) = forced {
            return forced;
        }
        if matches!(self, Self::Still) || (matches!(self, Self::System) && system()) {
            Motion::Still
        } else {
            Motion::InFront
        }
    }
}

static PREFERENCE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(Preference::System as u8);

/// Sets the player's choices. Reduced graphics takes precedence over the motion preference
/// and diagnostic overrides. Otherwise `None` follows the system.
pub fn set_preference(reduce: Option<bool>, reduced_graphics: bool) {
    PREFERENCE.store(
        Preference::choose(reduce, reduced_graphics) as u8,
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
    let preference = match PREFERENCE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Preference::Still,
        2 => Preference::Full,
        3 => Preference::ReducedGraphics,
        _ => Preference::System,
    };
    preference.resolve(*FORCED, system_reduces_motion)
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
    fn reduced_graphics_overrides_motion_without_changing_its_precedence_when_off() {
        for reduce in [None, Some(false), Some(true)] {
            for forced in [None, Some(Motion::Always), Some(Motion::InFront)] {
                let reduced = Preference::choose(reduce, true);
                assert_eq!(reduced.resolve(forced, || unreachable!()), Motion::Still);
            }
        }
        let system = Preference::choose(None, false);
        assert_eq!(system.resolve(None, || true), Motion::Still);
        assert_eq!(system.resolve(None, || false), Motion::InFront);
        let still = Preference::choose(Some(true), false);
        assert_eq!(still.resolve(None, || unreachable!()), Motion::Still);
        assert_eq!(
            still.resolve(Some(Motion::Always), || unreachable!()),
            Motion::Always
        );
        let full = Preference::choose(Some(false), false);
        assert_eq!(full.resolve(None, || unreachable!()), Motion::InFront);
        assert_eq!(
            full.resolve(Some(Motion::Still), || unreachable!()),
            Motion::Still
        );
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
