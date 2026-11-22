//! The launcher's own measurements, in the benchmark report format (docs/benchmark-report.md):
//! start to the first frame, the release list fetch, installs and preparing, Play to the game's
//! first frame, its memory, UI frame times and size. Each session rewrites one report in
//! `benchmarks/` of the launcher's data folder, a local log the player can attach to an issue;
//! nothing leaves the machine.
//!
//! `sunrise-launcher benchmark` measures the same things on purpose: several starts (the first is
//! the cold one), page switching, repeated release list fetches, idle CPU.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sunrise_launcher_bench::{self as sunrise_bench, App, Frames, Hardware, Kind, Report};

/// The first thing `main` does; every start time counts from here.
pub fn process_started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

/// Frames further apart than this are the UI resting, not UI frame times.
const IDLE_GAP: Duration = Duration::from_millis(250);

pub use sunrise_launcher::backend::GAME_FIRST_FRAME;

pub struct Perf {
    report: Report,
    path: PathBuf,
    values: BTreeMap<String, Vec<f64>>,
    first_frame: Option<Instant>,
    last_frame: Option<Instant>,
    ui_frames: Vec<f32>,
    /// The frames' own work (`draws`), read by a benchmark window while it measures.
    pub draws: Option<serde_json::Value>,
    saved: Instant,
    dirty: bool,
    /// Off for the windows `sunrise-launcher benchmark` starts: they report to it instead.
    log: bool,
    /// Machine probes run after the first frame, never while opening the window.
    hardware: Option<Receiver<Machine>>,
}

#[derive(Debug)]
struct Machine {
    hardware: Hardware,
    power: Option<String>,
    load_average: Option<f64>,
}

impl Machine {
    fn detect() -> Self {
        Self {
            hardware: Hardware::detect(),
            power: sunrise_bench::power_source().map(str::to_owned),
            load_average: sunrise_bench::load_average(),
        }
    }
}

impl Perf {
    pub fn new(scenario: &str) -> Perf {
        let hardware = Hardware {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            ..Hardware::default()
        };
        let mut report = Report::with_hardware(Kind::Launcher, app(), hardware);
        report.conditions.scenario = Some(scenario.to_owned());
        let path = crate::backend::data_dir().join("benchmarks").join(format!(
            "launcher-{}-{scenario}.json",
            report
                .created
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
        ));
        let mut perf = Perf {
            report,
            path,
            values: BTreeMap::new(),
            first_frame: None,
            last_frame: None,
            ui_frames: Vec::new(),
            draws: None,
            saved: Instant::now(),
            dirty: false,
            log: true,
            hardware: None,
        };
        if let Some(bytes) = binary_bytes() {
            perf.record("launcher.binary_bytes", bytes);
        }
        perf
    }

    pub fn keep_local_log(&mut self, log: bool) {
        self.log = log;
    }

    /// One more value of a measurement; the report keeps their summary.
    pub fn record(&mut self, name: &str, value: f64) {
        self.values.entry(name.to_owned()).or_default().push(value);
        self.dirty = true;
    }

    pub fn record_since(&mut self, name: &str, since: Instant) {
        self.record(name, since.elapsed().as_secs_f64() * 1000.0);
    }

    /// A frame is being drawn at `now`. The first one records the start time and starts the
    /// machine probes, off the UI thread; later ones add the time since the last frame to the
    /// session's UI frame times, unless the UI was resting in between ([`IDLE_GAP`]). Returns
    /// that time, resting or not, for a benchmark to judge ([`Bench::frame`]); none for the
    /// first frame.
    pub fn frame(&mut self, now: Instant) -> Option<Duration> {
        let gap = self.last_frame.map(|t| now.saturating_duration_since(t));
        if self.first_frame.is_none() {
            self.first_frame = Some(now);
            self.record_since("launcher.start_ms", process_started());
            let (sender, receiver) = std::sync::mpsc::channel();
            self.hardware = Some(receiver);
            std::thread::spawn(move || {
                // The window may close before the optional report details finish.
                let _ = sender.send(Machine::detect());
            });
        } else if let Some(gap) = gap.filter(|g| *g < IDLE_GAP) {
            self.ui_frames.push(gap.as_secs_f32() * 1000.0);
            // A sample the saved report doesn't have yet: the periodic or final save writes it.
            self.dirty = true;
        }
        self.last_frame = Some(now);
        gap
    }

    /// The window has drawn its first frame a while ago and none since `quiet`: nothing moves,
    /// so a little work on the UI thread holds up no frame.
    pub fn resting(&self, quiet: Duration) -> bool {
        self.first_frame
            .is_some_and(|first| first.elapsed() > Duration::from_secs(2))
            && self.last_frame.is_some_and(|last| last.elapsed() >= quiet)
    }

    /// Samples memory and writes the report when something changed, at most every few seconds.
    pub fn tick(&mut self) {
        self.poll_hardware();
        if self.saved.elapsed() < Duration::from_secs(5) {
            return;
        }
        if let Some(rss) = sunrise_bench::rss_bytes(std::process::id()) {
            self.values
                .entry("launcher.rss_mb".into())
                .or_default()
                .push(rss as f64 / f64::from(1 << 20));
        }
        if self.dirty {
            self.save();
        }
        self.saved = Instant::now();
    }

    pub fn report(&mut self) -> &Report {
        self.poll_hardware();
        for (name, values) in &self.values {
            self.report.metric(name, values);
        }
        self.report.frames = Frames::of("ui", 0, &self.ui_frames, &[], &[])
            .into_iter()
            .collect();
        self.report.created = sunrise_bench::utc_now();
        &self.report
    }

    /// Takes the machine's details into the report once their thread has them.
    fn poll_hardware(&mut self) {
        let Some(receiver) = &self.hardware else {
            return;
        };
        match receiver.try_recv() {
            Ok(machine) => {
                self.report.hardware = machine.hardware;
                self.report.conditions.power = machine.power;
                self.report.conditions.load_avg_1m = machine.load_average;
                self.hardware = None;
                self.dirty = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.hardware = None,
        }
    }

    pub fn save(&mut self) {
        self.dirty = false;
        if !self.log {
            return;
        }
        let json = self.report().to_json();
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.path, json + "\n");
    }
}

impl Drop for Perf {
    fn drop(&mut self) {
        if self.dirty {
            self.save();
        }
    }
}

/// `SUNRISE_LAUNCHER_FRAME_OVERLAY=1` (or `full`) paints GPUI's frame times over the window, in a
/// build with `--features frame-stats`: how long each frame's work took, while animating.
#[cfg(feature = "frame-stats")]
pub fn frame_overlay(window: &mut gpui::Window) {
    let mode = match std::env::var("SUNRISE_LAUNCHER_FRAME_OVERLAY").as_deref() {
        Ok("full") => gpui::DebugFrameOverlayMode::Full,
        Ok("" | "0") | Err(_) => return,
        Ok(_) => gpui::DebugFrameOverlayMode::Minimal,
    };
    window.set_debug_frame_overlay_mode(mode);
}

/// The work of the frames a window drew so far (`--features frame-stats`), in milliseconds:
/// building, laying out and painting them, which the frame intervals (`ui_ms`) hide under the
/// display's refresh. `late` frames took longer than a refresh; `missed` refreshes they cost.
#[cfg(feature = "frame-stats")]
pub fn draws(window: &gpui::Window) -> serde_json::Value {
    let frames = window.frame_duration_snapshot();
    let draw = &frames.draw_duration_histogram;
    let ms = |q: f64| draw.value_at_quantile(q) as f64 / 1e6;
    let (late, missed) = frames
        .by_refresh_interval
        .iter()
        .fold((0, 0), |(late, missed), f| {
            (late + f.frames_late, missed + f.missed_refreshes)
        });
    serde_json::json!({
        "frames": draw.len(),
        "p50_ms": ms(0.5),
        "p95_ms": ms(0.95),
        "p99_ms": ms(0.99),
        "max_ms": draw.max() as f64 / 1e6,
        "late": late,
        "missed": missed,
    })
}

/// What a window started by `sunrise-launcher benchmark` does next (`Launcher::drain`).
pub enum Step {
    Wait,
    /// Show this page (an index into `frame::Page::BENCH`, wrapping).
    Go(usize),
    /// Open the build picker and scroll its list, a wheel step per frame (`sheets::bench_wheel`),
    /// while [`Bench::scrolling`].
    Scroll,
    Quit,
}

const SWITCHING: Duration = Duration::from_secs(4);
/// How long the list is scrolled, from the first frame that drew it with its loaded rows.
const SCROLLING: Duration = Duration::from_secs(3);
/// How long the opened picker may wait for the release list and the rows it brings: a cold
/// start's fetch can take longer than the scrolling itself.
const LIST_WAIT: Duration = Duration::from_secs(30);
const IDLE: Duration = Duration::from_secs(3);
/// The environment variable that makes a launcher window measure itself into the file it names.
const BENCH_ENV: &str = "SUNRISE_LAUNCHER_BENCH";

/// How far one scripted wheel step moves the list.
const WHEEL_STEP: f32 = 24.;

/// A launcher window measuring itself: after its first frame it switches pages every
/// [`crate::wake::TICK`] for a few seconds, opens the build picker and, once the release list
/// has loaded and the picker has drawn its rows, scrolls the list for a few more; then it rests,
/// writes what it measured for `benchmark` and quits.
///
/// Its samples are frame intervals while something was due on screen: during page switching,
/// from each frame to the next, or from a switch to the first frame after it; while scrolling,
/// only frames on which the list moved. A slow frame in either phase is kept, however long; the
/// wait for the release list and the gaps of the resting phase are not samples at all.
pub struct Bench {
    out: PathBuf,
    began: Option<Instant>,
    page: usize,
    /// When the last page switch was, which paces the next: the pump also runs between ticks,
    /// when messages arrive.
    switched: Option<Instant>,
    /// The first switch the window hasn't drawn yet. A second switch before that frame
    /// doesn't move it, so a stall across both is timed from the first.
    awaiting: Option<Instant>,
    scroll: Option<Scroll>,
    idle_cpu: Option<(Instant, f64)>,
    pages_ms: Vec<f32>,
    scroll_ms: Vec<f32>,
}

struct Scroll {
    /// When the picker was opened, which bounds the wait for its rows ([`LIST_WAIT`]).
    opened: Instant,
    /// When the timed phase began: the first frame after which the list was drawn with its
    /// loaded rows. None while the picker waits for them.
    began: Option<Instant>,
    /// What the list waited for when last looked at, to say why it never came.
    waiting: Target,
    /// Why nothing was scrolled, once that is known.
    skipped: Option<Skip>,
    ended: bool,
    /// Heading for the bottom of the list, else back to the top.
    down: bool,
    /// The list's offset after the last frame, to tell a frame that moved it.
    last_offset: Option<f32>,
    /// The interval of the frame just drawn, until [`Bench::after_frame`] knows whether it
    /// moved the list.
    pending: Option<Duration>,
}

impl Scroll {
    fn opened(now: Instant) -> Scroll {
        Scroll {
            opened: now,
            began: None,
            waiting: Target::Loading,
            skipped: None,
            ended: false,
            down: true,
            last_offset: None,
            pending: None,
        }
    }

    /// Ends the phase with nothing measured, for `why` (the first reason found stays).
    fn skip(&mut self, why: Skip) {
        self.skipped.get_or_insert(why);
        self.ended = true;
    }
}

/// The build picker's list as the benchmark finds it after a frame (`sheets::bench_wheel`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// No profile is selected, so the picker has no list.
    NoProfile,
    /// The picker isn't the sheet on screen.
    Closed,
    /// The release list is still being fetched.
    Loading,
    /// Loaded, and the list has not one row.
    Empty,
    /// Loaded, but the window hasn't drawn the list with its loaded rows yet.
    LayingOut,
    /// Drawn with its loaded rows: its offset (GPUI's sign, negative below the top) and how far
    /// it can scroll.
    Laid { offset: f32, max: f32 },
}

/// Why a benchmark scrolled nothing, as the CLI says it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Skip {
    NoProfile,
    Closed,
    Empty,
    Fits,
    ReleasesLate,
    NotLaidOut,
    NeverMoved,
}

impl Skip {
    fn says(self) -> &'static str {
        match self {
            Skip::NoProfile => "no profile is selected, so the build picker has no list",
            Skip::Closed => "the build picker was closed before it could be scrolled",
            Skip::Empty => "the build list had no rows",
            Skip::Fits => "the build list fits without scrolling",
            Skip::ReleasesLate => "the release list didn't load in time",
            Skip::NotLaidOut => "the build picker didn't draw its rows in time",
            Skip::NeverMoved => "the build list didn't move",
        }
    }
}

impl Bench {
    /// Whether this launcher measures itself (`SUNRISE_LAUNCHER_BENCH` set).
    pub fn wanted() -> bool {
        std::env::var_os(BENCH_ENV).is_some()
    }

    pub fn from_env() -> Option<Bench> {
        let out = std::env::var_os(BENCH_ENV)?;
        Some(Bench::new(out.into()))
    }

    fn new(out: PathBuf) -> Bench {
        Bench {
            out,
            began: None,
            page: 0,
            switched: None,
            awaiting: None,
            scroll: None,
            idle_cpu: None,
            pages_ms: Vec::new(),
            scroll_ms: Vec::new(),
        }
    }

    /// A benchmark whose page switching is over, writing its result to `out`: the next tick
    /// opens the build picker. For tests that drive a real launcher window.
    #[cfg(test)]
    pub(crate) fn switched_for_test(out: PathBuf) -> Bench {
        let mut bench = Bench::new(out);
        bench.began = Some(Instant::now() - SWITCHING);
        bench
    }

    #[cfg(test)]
    pub(crate) fn scroll_samples(&self) -> &[f32] {
        &self.scroll_ms
    }

    /// Between [`Step::Scroll`] and the end of scrolling: a wheel step goes in on every frame.
    pub fn scrolling(&self) -> bool {
        self.scroll.as_ref().is_some_and(|s| !s.ended)
    }

    /// While scrolling, once the timed phase has begun: the list was drawn with its rows.
    pub fn timing_scroll(&self) -> bool {
        self.scroll
            .as_ref()
            .is_some_and(|s| !s.ended && s.began.is_some())
    }

    /// A frame drawn at `now`, `gap` after the one before ([`Perf::frame`]). Called while the
    /// frame is built, so it reads nothing on screen: whether a scrolling frame moved the list
    /// is told after it ([`Bench::after_frame`]). The frames that open the picker and wait for
    /// its rows are no samples.
    pub fn frame(&mut self, now: Instant, gap: Option<Duration>) {
        let Some(gap) = gap else {
            return;
        };
        match &mut self.scroll {
            None if self.began.is_some() => {
                // The first frame after a switch: how long the switch waited for it.
                let sample = match self.awaiting.take() {
                    Some(switched) => now.saturating_duration_since(switched),
                    None => gap,
                };
                self.pages_ms.push(sample.as_secs_f32() * 1000.0);
            }
            Some(scroll) if !scroll.ended && scroll.began.is_some() => {
                scroll.pending = Some(gap);
            }
            _ => {}
        }
    }

    /// After a frame, at `now`, how it left the build list, read outside the frame so reading
    /// it changes nothing the frame caches.
    ///
    /// The timed phase begins at the first frame that drew the list with its loaded rows; with
    /// no list, an empty one or one that fits, it ends at once with nothing measured. While
    /// timed, counts the frame as a scroll sample if it moved the list, and gives the wheel step
    /// for the next one, in GPUI's sign (negative scrolls down): [`WHEEL_STEP`] towards the end
    /// it is heading for, the last step only as far as that end, then back the other way. None
    /// while waiting for the rows, or when not scrolling.
    pub fn after_frame(&mut self, now: Instant, target: Target) -> Option<f32> {
        let scroll = self.scroll.as_mut().filter(|s| !s.ended)?;
        let (offset, max) = match target {
            Target::Laid { offset, max } => (offset, max),
            Target::Loading | Target::LayingOut => {
                if scroll.began.is_none() {
                    scroll.waiting = target;
                }
                // A frame that drew the list without its loaded rows is no sample.
                scroll.pending = None;
                return None;
            }
            Target::NoProfile | Target::Closed | Target::Empty => {
                scroll.skip(match target {
                    Target::NoProfile => Skip::NoProfile,
                    Target::Closed => Skip::Closed,
                    _ => Skip::Empty,
                });
                return None;
            }
        };
        if scroll.began.is_none() {
            if max < 0.5 {
                scroll.skip(Skip::Fits);
                return None;
            }
            scroll.began = Some(now);
            scroll.pending = None;
        }
        let moved = scroll
            .last_offset
            .is_some_and(|was| (was - offset).abs() >= 0.5);
        if let Some(gap) = scroll.pending.take().filter(|_| moved) {
            self.scroll_ms.push(gap.as_secs_f32() * 1000.0);
        }
        scroll.last_offset = Some(offset);
        wheel_step(offset, max, &mut scroll.down)
    }

    pub fn tick(&mut self, perf: &Perf) -> Step {
        let Some(first) = perf.first_frame else {
            return Step::Wait;
        };
        let began = *self.began.get_or_insert_with(Instant::now);
        if began.elapsed() < SWITCHING {
            if self
                .switched
                .is_some_and(|at| at.elapsed() < crate::wake::TICK)
            {
                return Step::Wait;
            }
            let now = Instant::now();
            self.switched = Some(now);
            self.awaiting.get_or_insert(now);
            self.page += 1;
            return Step::Go(self.page);
        }
        match &mut self.scroll {
            // The last switch's frame hasn't come yet. It is a page sample, a stall included,
            // so the picker waits for it rather than being drawn in the same frame.
            None if self.awaiting.is_some() => return Step::Wait,
            None => {
                self.scroll = Some(Scroll::opened(Instant::now()));
                return Step::Scroll;
            }
            Some(scroll) if !scroll.ended => match scroll.began {
                None if scroll.opened.elapsed() < LIST_WAIT => return Step::Wait,
                None => scroll.skip(match scroll.waiting {
                    Target::Loading => Skip::ReleasesLate,
                    _ => Skip::NotLaidOut,
                }),
                Some(began) if began.elapsed() < SCROLLING => return Step::Wait,
                Some(_) => scroll.ended = true,
            },
            Some(_) => {}
        }
        let pid = std::process::id();
        let Some((rested, cpu)) = self.idle_cpu else {
            self.idle_cpu = Some((
                Instant::now(),
                sunrise_bench::cpu_seconds(pid).unwrap_or(0.0),
            ));
            return Step::Wait;
        };
        if rested.elapsed() < IDLE {
            return Step::Wait;
        }
        let cpu_idle_pct = sunrise_bench::cpu_seconds(pid)
            .map(|now| 100.0 * (now - cpu) / rested.elapsed().as_secs_f64());
        let epoch_ms = |at: Instant| {
            let ago = at.elapsed();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.saturating_sub(ago)).as_secs_f64() * 1000.0
        };
        let result = serde_json::json!({
            "first_frame_epoch_ms": epoch_ms(first),
            "pages_ms": self.pages_ms,
            // None when the list never moved: no scrolling was measured, not a fast one.
            "scroll_ms": (!self.scroll_ms.is_empty()).then_some(&self.scroll_ms),
            // And then why: no profile, no rows, rows that fit, or a list that never came.
            "scroll_skipped": self.scroll_ms.is_empty().then(|| {
                self.scroll
                    .as_ref()
                    .and_then(|s| s.skipped)
                    .unwrap_or(Skip::NeverMoved)
                    .says()
            }),
            "rss_mb": sunrise_bench::rss_bytes(pid).map(|b| b as f64 / f64::from(1 << 20)),
            "cpu_idle_pct": cpu_idle_pct,
            "draws": perf.draws,
        });
        let _ = std::fs::write(&self.out, result.to_string());
        Step::Quit
    }
}

/// `sunrise-launcher benchmark`: starts the launcher `repeats` times (the first start is the cold
/// one), each switching pages then resting, times the release list fetch, and writes one report.
pub fn benchmark(repeats: usize) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut report = Report::new(Kind::Launcher, app());
    report.conditions.scenario = Some("benchmark".into());
    report.conditions.repeats = Some(repeats as u32);
    report.conditions.power = sunrise_bench::power_source().map(str::to_owned);
    report.conditions.load_avg_1m = sunrise_bench::load_average();
    let (mut starts, mut rss, mut cpu) = (Vec::new(), Vec::new(), Vec::new());
    // A `--features frame-stats` build's frame work (`draws`); none in other builds.
    let (mut draw_p50, mut draw_p95, mut draw_p99, mut late) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let scratch =
        std::env::temp_dir().join(format!("sunrise-launcher-bench-{}", std::process::id()));
    for repeat in 0..repeats {
        let _ = std::fs::remove_file(&scratch);
        let spawned = std::time::SystemTime::now();
        let mut child = std::process::Command::new(&exe)
            .env(BENCH_ENV, &scratch)
            .spawn()
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                return Err(
                    "the launcher window drew nothing for 90 s: is the screen locked or asleep?"
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let text = std::fs::read_to_string(&scratch)
            .map_err(|_| "the launcher window quit without measuring".to_owned())?;
        let _ = std::fs::remove_file(&scratch);
        let result: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let spawned_ms = spawned
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            * 1000.0;
        if let Some(first) = result["first_frame_epoch_ms"].as_f64() {
            starts.push(first - spawned_ms);
        }
        rss.extend(result["rss_mb"].as_f64());
        cpu.extend(result["cpu_idle_pct"].as_f64());
        let draws = &result["draws"];
        for (key, values) in [
            ("p50_ms", &mut draw_p50),
            ("p95_ms", &mut draw_p95),
            ("p99_ms", &mut draw_p99),
            ("late", &mut late),
        ] {
            values.extend(draws[key].as_f64());
        }
        let times = |key: &str| -> Vec<f32> {
            result[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_f64())
                .map(|v| v as f32)
                .collect()
        };
        let (pages, scroll) = (times("pages_ms"), times("scroll_ms"));
        report
            .frames
            .extend(Frames::of("pages", repeat as u32, &pages, &[], &[]));
        report
            .frames
            .extend(Frames::of("scroll", repeat as u32, &scroll, &[], &[]));
        eprintln!(
            "start {}: {:.0} ms to the first frame; pages {}; scrolling {}",
            repeat + 1,
            starts.last().unwrap_or(&0.0),
            summary(&pages),
            if scroll.is_empty() {
                format!(
                    "nothing: {}",
                    result["scroll_skipped"]
                        .as_str()
                        .unwrap_or(Skip::NeverMoved.says())
                )
            } else {
                summary(&scroll)
            },
        );
        if let Some(frames) = draws["frames"].as_u64() {
            eprintln!(
                "  frame work: {frames} frames, median {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.1} ms; {} late",
                draws["p50_ms"].as_f64().unwrap_or(0.),
                draws["p95_ms"].as_f64().unwrap_or(0.),
                draws["p99_ms"].as_f64().unwrap_or(0.),
                draws["max_ms"].as_f64().unwrap_or(0.),
                draws["late"].as_u64().unwrap_or(0),
            );
        }
    }
    report.metric("launcher.cold_start_ms", &starts[..starts.len().min(1)]);
    report.metric(
        "launcher.warm_start_ms",
        starts.get(1..).unwrap_or_default(),
    );
    report.metric("launcher.rss_mb", &rss);
    report.metric("launcher.cpu_idle_pct", &cpu);
    if !draw_p50.is_empty() {
        report.metric("launcher.frame_work_p50_ms", &draw_p50);
        report.metric("launcher.frame_work_p95_ms", &draw_p95);
        report.metric("launcher.frame_work_p99_ms", &draw_p99);
        report.metric("launcher.late_frames", &late);
    }
    let mut fetches = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        crate::backend::releases()?;
        fetches.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    report.metric("launcher.release_list_ms", &fetches);
    if let Some(bytes) = binary_bytes() {
        report.metric("launcher.binary_bytes", &[bytes]);
    }
    report
        .write_to(&crate::backend::data_dir().join("benchmarks"))
        .map_err(|e| e.to_string())
}

/// The scripted wheel's next step ([`Bench::wheel`]), turning `down` round at either end.
fn wheel_step(offset: f32, max: f32, down: &mut bool) -> Option<f32> {
    if max < 0.5 {
        return None;
    }
    let travelled = (-offset).clamp(0., max);
    if *down && travelled >= max - 0.5 {
        *down = false;
    } else if !*down && travelled <= 0.5 {
        *down = true;
    }
    Some(match *down {
        true => -(max - travelled).min(WHEEL_STEP),
        false => travelled.min(WHEEL_STEP),
    })
}

/// Frame times as the CLI prints them: how many, their median and p95.
fn summary(frames: &[f32]) -> String {
    let mut sorted = frames.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |q: f32| {
        sorted
            .get(((sorted.len() as f32 - 1.) * q) as usize)
            .copied()
            .unwrap_or(0.)
    };
    format!(
        "{} frames, median {:.1} ms, p95 {:.1} ms",
        sorted.len(),
        at(0.5),
        at(0.95)
    )
}

fn app() -> App {
    App {
        name: "sunrise-launcher".into(),
        version: crate::backend::VERSION.into(),
        ..App::default()
    }
}

fn binary_bytes() -> Option<f64> {
    Some(std::fs::metadata(std::env::current_exe().ok()?).ok()?.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn performance_tracking_starts_without_hardware_probes() {
        let mut perf = Perf::new("test");
        perf.keep_local_log(false);
        assert!(perf.hardware.is_none());
        assert_eq!(perf.report.hardware.os, std::env::consts::OS);
        assert!(perf.report.hardware.cpu.is_none());
        assert!(perf.report.conditions.power.is_none());
    }

    /// A perf that has drawn its first frame, without the machine probes a real one starts.
    fn drawn() -> Perf {
        let mut perf = Perf::new("test");
        perf.keep_local_log(false);
        perf.first_frame = Some(Instant::now());
        perf
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn laid(offset: f32, max: f32) -> Target {
        Target::Laid { offset, max }
    }

    /// Ends switching, the last switch drawn, then opens the picker.
    fn to_scrolling(bench: &mut Bench, perf: &Perf) {
        bench.began = Some(Instant::now() - SWITCHING);
        bench.awaiting = None;
        assert!(matches!(bench.tick(perf), Step::Scroll));
        assert!(bench.scrolling());
        assert!(!bench.timing_scroll(), "not before the list has its rows");
    }

    /// Ends scrolling, if it is still on, and resting; the next tick writes the result.
    fn finish(bench: &mut Bench, perf: &Perf) -> serde_json::Value {
        let scroll = bench.scroll.as_mut().unwrap();
        if !scroll.ended {
            scroll.began = Some(Instant::now() - SCROLLING);
            assert!(matches!(bench.tick(perf), Step::Wait), "scrolling ends");
        }
        assert!(!bench.scrolling());
        if bench.idle_cpu.is_none() {
            assert!(matches!(bench.tick(perf), Step::Wait), "resting begins");
        }
        bench.idle_cpu.as_mut().unwrap().0 = Instant::now() - IDLE;
        assert!(
            matches!(bench.tick(perf), Step::Quit),
            "the result, then quit"
        );
        let result = serde_json::from_str(&std::fs::read_to_string(&bench.out).unwrap()).unwrap();
        let _ = std::fs::remove_file(&bench.out);
        result
    }

    fn out(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sunrise-bench-{name}-{}.json", std::process::id()))
    }

    #[test]
    fn page_switch_samples_keep_stalls_and_leave_out_idle_gaps() {
        let perf = drawn();
        let mut bench = Bench::new(out("pages"));
        // Before the script starts, a frame is no sample.
        bench.frame(Instant::now(), Some(ms(16)));
        assert!(matches!(bench.tick(&perf), Step::Go(1)));
        let switched = bench.switched.unwrap();
        // The UI had rested for 2 s, then the switch waited 300 ms for its frame: a stall of
        // 300 ms, not a 2.3 s gap.
        bench.frame(switched + ms(300), Some(ms(2300)));
        // Drawn continuously while the switch animates.
        bench.frame(switched + ms(316), Some(ms(16)));
        assert_eq!(bench.pages_ms, [300., 16.]);
    }

    #[test]
    fn a_stall_across_two_switches_is_timed_from_the_first() {
        let perf = drawn();
        let mut bench = Bench::new(out("two-switches"));
        assert!(matches!(bench.tick(&perf), Step::Go(1)));
        let first = bench.switched.unwrap();
        // The next switch is due before the window has drawn the first.
        bench.switched = Some(first - crate::wake::TICK);
        assert!(matches!(bench.tick(&perf), Step::Go(2)));
        assert!(
            bench.switched.unwrap() > first,
            "the pace follows the later switch"
        );
        // The frame finally comes 500 ms after the first switch.
        bench.frame(first + ms(500), Some(ms(900)));
        assert_eq!(bench.pages_ms, [500.], "not hidden behind the later switch");
    }

    #[test]
    fn the_picker_waits_for_the_last_switch_to_be_drawn() {
        let perf = drawn();
        let mut bench = Bench::new(out("boundary"));
        assert!(matches!(bench.tick(&perf), Step::Go(1)));
        let switched = bench.switched.unwrap();
        // Switching time is up before the window has drawn the last switch.
        bench.began = Some(Instant::now() - SWITCHING);
        for _ in 0..3 {
            assert!(matches!(bench.tick(&perf), Step::Wait), "the picker waits");
            assert!(!bench.scrolling());
        }
        // The frame comes 700 ms late: a page sample, the stall in full.
        bench.frame(switched + ms(700), Some(ms(900)));
        assert_eq!(bench.pages_ms, [700.]);
        assert!(matches!(bench.tick(&perf), Step::Scroll));
    }

    #[test]
    fn scrolling_is_timed_from_the_frame_that_drew_the_loaded_rows() {
        let mut perf = drawn();
        let mut bench = Bench::new(out("scroll"));
        to_scrolling(&mut bench, &perf);
        let now = Instant::now();
        // A cold start: the release list is still on its way, longer than the scroll phase.
        bench.scroll.as_mut().unwrap().opened = now - SCROLLING - ms(500);
        for target in [Target::Loading, Target::Loading, Target::LayingOut] {
            bench.frame(now, Some(ms(40)));
            assert_eq!(
                bench.after_frame(now, target),
                None,
                "nothing to scroll yet"
            );
            assert!(matches!(bench.tick(&perf), Step::Wait));
            assert!(bench.scrolling() && !bench.timing_scroll(), "still waiting");
        }
        // The first frame that drew the rows: the timing begins; the first step goes in.
        bench.frame(now, Some(ms(16)));
        assert_eq!(bench.after_frame(now, laid(0., 100.)), Some(-24.));
        assert!(bench.timing_scroll());
        assert!(matches!(bench.tick(&perf), Step::Wait), "timed from here");
        // That step moved it: a sample.
        bench.frame(now, Some(ms(17)));
        assert_eq!(bench.after_frame(now, laid(-24., 100.)), Some(-24.));
        // A frame that didn't move it isn't one.
        bench.frame(now, Some(ms(16)));
        bench.after_frame(now, laid(-24., 100.));
        // A real stall while scrolling is kept.
        bench.frame(now, Some(ms(400)));
        bench.after_frame(now, laid(-48., 100.));
        assert_eq!(
            bench.scroll_ms,
            [17., 400.],
            "the wait for the rows is none"
        );
        // Drawn while resting: no sample of either kind.
        perf.frame(now + ms(16));
        let result = finish(&mut bench, &perf);
        bench.frame(now, Some(ms(16)));
        assert_eq!(
            bench.after_frame(now, laid(-72., 100.)),
            None,
            "the wheel has stopped"
        );
        assert_eq!(bench.scroll_ms.len(), 2);
        assert_eq!(result["scroll_ms"], serde_json::json!([17., 400.]));
        assert!(result["scroll_skipped"].is_null());
        assert_eq!(result["pages_ms"], serde_json::json!([]));
        assert!(result["first_frame_epoch_ms"].as_f64().is_some());
    }

    #[test]
    fn no_list_an_empty_one_or_one_that_fits_ends_scrolling_at_once() {
        for (target, says) in [
            (Target::NoProfile, Skip::NoProfile),
            (Target::Closed, Skip::Closed),
            (Target::Empty, Skip::Empty),
            (laid(0., 0.), Skip::Fits),
        ] {
            let perf = drawn();
            let mut bench = Bench::new(out("still"));
            to_scrolling(&mut bench, &perf);
            let now = Instant::now();
            bench.frame(now, Some(ms(16)));
            assert_eq!(bench.after_frame(now, target), None, "{target:?}");
            assert!(!bench.scrolling(), "{target:?}: no frames asked for");
            let result = finish(&mut bench, &perf);
            assert!(
                result["scroll_ms"].is_null(),
                "{target:?}: no result, not an empty fast one"
            );
            assert_eq!(result["scroll_skipped"], says.says(), "{target:?}");
        }
    }

    #[test]
    fn a_list_that_never_comes_ends_with_why() {
        for (waiting, says) in [
            (Target::Loading, Skip::ReleasesLate),
            (Target::LayingOut, Skip::NotLaidOut),
        ] {
            let perf = drawn();
            let mut bench = Bench::new(out("late"));
            to_scrolling(&mut bench, &perf);
            assert_eq!(bench.after_frame(Instant::now(), waiting), None);
            bench.scroll.as_mut().unwrap().opened = Instant::now() - LIST_WAIT;
            assert!(matches!(bench.tick(&perf), Step::Wait));
            assert!(!bench.scrolling(), "{waiting:?}: given up on");
            let result = finish(&mut bench, &perf);
            assert!(result["scroll_ms"].is_null());
            assert_eq!(result["scroll_skipped"], says.says(), "{waiting:?}");
        }
    }

    #[test]
    fn the_wheel_reaches_each_end_before_turning_round() {
        let mut down = true;
        // A list that overflows by less than a step still moves, all the way.
        assert_eq!(wheel_step(0., 10., &mut down), Some(-10.));
        assert_eq!(wheel_step(-10., 10., &mut down), Some(10.));
        assert!(!down);
        assert_eq!(wheel_step(0., 10., &mut down), Some(-10.));
        assert!(down);
        // A longer one: full steps, then only as far as the bottom.
        let (mut offset, mut down) = (0f32, true);
        let mut steps = Vec::new();
        for _ in 0..6 {
            let step = wheel_step(offset, 60., &mut down).unwrap();
            steps.push(step);
            offset = (offset + step).clamp(-60., 0.);
        }
        assert_eq!(steps, [-24., -24., -12., 24., 24., 12.]);
        assert_eq!(wheel_step(0., 0., &mut down), None);
    }

    #[test]
    fn a_frame_sample_marks_the_report_unsaved() {
        let mut perf = drawn();
        let now = Instant::now();
        perf.last_frame = Some(now);
        perf.dirty = false;
        assert_eq!(perf.frame(now + ms(16)), Some(ms(16)));
        assert!(perf.dirty, "the next save writes it");
        perf.dirty = false;
        assert_eq!(perf.frame(now + ms(1016)), Some(ms(1000)));
        assert!(!perf.dirty, "a resting gap is no UI frame time");
    }

    #[test]
    fn background_hardware_details_preserve_recorded_metrics() {
        let mut perf = Perf::new("test");
        perf.keep_local_log(false);
        perf.record("launcher.start_ms", 125.);
        let (sender, receiver) = mpsc::channel();
        perf.hardware = Some(receiver);
        assert_eq!(perf.report().metrics["launcher.start_ms"].mean, 125.);
        sender
            .send(Machine {
                hardware: Hardware {
                    cpu: Some("Detected CPU".into()),
                    ..Hardware::default()
                },
                power: Some("ac".into()),
                load_average: Some(0.5),
            })
            .unwrap();
        let report = perf.report();
        assert_eq!(report.hardware.cpu.as_deref(), Some("Detected CPU"));
        assert_eq!(report.conditions.power.as_deref(), Some("ac"));
        assert_eq!(report.metrics["launcher.start_ms"].mean, 125.);
        assert!(perf.hardware.is_none());
    }
}
