//! Smoothed time-remaining estimates for loading dialogs and status strips.
//!
//! Determinate stages (downloads, toolchains, native builds) estimate remaining time from an
//! exponential moving average of their progress rate after a short warmup. Indeterminate
//! preparation (`PrepareStep::Prepare`) has no live percentage, so it falls back to the
//! machine's own previous preparation duration saved in `eta-timings.json`.

use crate::i18n::{tr, trf};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sunrise_launcher::backend::{self, PrepareStep};
use sunrise_launcher::browser::sunrise::InstallProgress;

/// Wait this long in a stage before showing an ETA so fast steps and bursty first chunks
/// don't flash noisy numbers.
const WARMUP: Duration = Duration::from_secs(2);

/// Minimum elapsed time between rate samples so bursty chunk callbacks don't skew the EMA.
const MIN_SAMPLE_DT: f64 = 0.15;

/// Hide the ETA if a determinate stage hasn't moved in this long.
const STALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Smoothing factor for the progress rate EMA (`fraction / second`).
const RATE_ALPHA: f64 = 0.25;

/// Smoothing factor for persisted historical durations (`PrepareStep::Prepare`).
const HISTORY_ALPHA: f64 = 0.35;

/// Bounds for determinate fraction display.
const MIN_FRACTION: f32 = 0.02;
const MAX_FRACTION: f32 = 0.995;

/// Discard absurdly large ETAs (> 3 hours).
const MAX_ETA_SECS: f64 = 3.0 * 3600.0;

/// Bounds for recording a historical preparation duration.
const MIN_RECORD_PREPARE_SECS: f64 = 2.0;
const MAX_RECORD_PREPARE_SECS: f64 = 30.0 * 60.0;

/// Tracks a single active loading stage and estimates how much time is left.
#[derive(Clone, Debug, Default)]
pub(crate) struct StageEta {
    stage: Option<u8>,
    started: Option<Instant>,
    last_sample: Option<(Instant, f32)>,
    last_advance: Option<Instant>,
    rate_ema: Option<f64>,
    last_fraction: f32,
}

impl StageEta {
    /// Feed the current stage key and optional determinate fraction (`0.0..=1.0`).
    /// Switching to a different `stage` resets the estimator cleanly.
    pub(crate) fn update(&mut self, stage: u8, fraction: Option<f32>, now: Instant) {
        let clamped = fraction
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(0.0, 1.0));
        if self.stage != Some(stage) {
            self.stage = Some(stage);
            self.started = Some(now);
            self.last_sample = clamped.map(|f| (now, f));
            self.last_advance = Some(now);
            self.rate_ema = None;
            self.last_fraction = clamped.unwrap_or(0.0);
            return;
        }
        let Some(f) = clamped else {
            return;
        };
        if f > self.last_fraction + 1e-4 {
            if let Some((prev_time, prev_f)) = self.last_sample {
                let dt = now.saturating_duration_since(prev_time).as_secs_f64();
                let df = (f - prev_f) as f64;
                if dt >= MIN_SAMPLE_DT && df > 0.0 {
                    let inst_rate = df / dt;
                    self.rate_ema = Some(match self.rate_ema {
                        Some(prev) => RATE_ALPHA * inst_rate + (1.0 - RATE_ALPHA) * prev,
                        None => inst_rate,
                    });
                    self.last_sample = Some((now, f));
                }
            } else {
                self.last_sample = Some((now, f));
            }
            self.last_fraction = f;
            self.last_advance = Some(now);
        }
    }

    /// Estimated time remaining for a determinate stage, or `None` while warming up, stalled,
    /// nearly complete, or indeterminate.
    pub(crate) fn remaining(&self, now: Instant) -> Option<Duration> {
        let started = self.started?;
        let elapsed = now.saturating_duration_since(started);
        if elapsed < WARMUP {
            return None;
        }
        let last_advance = self.last_advance?;
        if now.saturating_duration_since(last_advance) >= STALL_TIMEOUT {
            return None;
        }
        if self.last_fraction < MIN_FRACTION || self.last_fraction >= MAX_FRACTION {
            return None;
        }
        let rate = self.rate_ema.unwrap_or_else(|| {
            let secs = elapsed.as_secs_f64();
            if secs > 0.0 {
                self.last_fraction as f64 / secs
            } else {
                0.0
            }
        });
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        let remaining_secs = (1.0 - self.last_fraction as f64) / rate;
        if !remaining_secs.is_finite() || remaining_secs > MAX_ETA_SECS {
            return None;
        }
        Some(Duration::from_secs_f64(remaining_secs.max(0.0)))
    }
}

/// Estimated time remaining for an indeterminate stage whose typical duration on this machine
/// is known (such as `PrepareStep::Prepare`). Drops the estimate once `elapsed >= expected`
/// instead of counting upward or sticking at zero.
pub(crate) fn remaining_from_expected(
    started: Instant,
    expected: Duration,
    now: Instant,
) -> Option<Duration> {
    let elapsed = now.saturating_duration_since(started);
    if elapsed < WARMUP || elapsed >= expected {
        return None;
    }
    Some(expected.saturating_sub(elapsed))
}

/// Distinct stage key for each `InstallProgress` phase so transitions reset `StageEta`.
pub(crate) fn install_stage_key(step: InstallProgress) -> u8 {
    match step {
        InstallProgress::Waiting => 0,
        InstallProgress::PreparingBuild => 1,
        InstallProgress::DownloadingBuild(_) => 2,
        InstallProgress::Minecraft(PrepareStep::Files, _) => 3,
        InstallProgress::Minecraft(PrepareStep::Java, _) => 4,
        InstallProgress::Minecraft(PrepareStep::Sounds, _) => 5,
        InstallProgress::Minecraft(PrepareStep::Prepare, _) => 6,
        InstallProgress::Checking => 7,
        InstallProgress::Downloading(_) => 8,
        InstallProgress::Preparing => 9,
        InstallProgress::Tools { .. } => 10,
        InstallProgress::Rust(_) => 11,
        InstallProgress::Building(_) => 12,
        InstallProgress::Publishing => 13,
        InstallProgress::Reviewing => 14,
    }
}

/// Format a remaining duration in calm, coarsely rounded words so it never jitters every second.
pub(crate) fn format_eta(remaining: Duration) -> String {
    let secs = remaining.as_secs_f64().round() as u64;
    if secs < 5 {
        return tr("A few seconds left").into();
    }
    if secs <= 52 {
        let rounded = (((secs + 2) / 5) * 5).clamp(5, 50);
        return trf(
            "About {seconds} sec left",
            &[("seconds", &rounded.to_string())],
        );
    }
    if secs <= 89 {
        return tr("About 1 min left").into();
    }
    let total_mins = (secs + 30) / 60;
    if total_mins < 60 {
        return trf(
            "About {minutes} min left",
            &[("minutes", &total_mins.to_string())],
        );
    }
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if mins == 0 {
        trf("About {hours} hr left", &[("hours", &hours.to_string())])
    } else {
        trf(
            "About {hours} hr {minutes} min left",
            &[
                ("hours", &hours.to_string()),
                ("minutes", &mins.to_string()),
            ],
        )
    }
}

/// Append a formatted ETA to a status line when one is available.
pub(crate) fn with_eta(label: String, remaining: Option<Duration>) -> String {
    match remaining {
        Some(duration) => format!("{label} · {}", format_eta(duration)),
        None => label,
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Timings {
    #[serde(default)]
    prepare_secs: Option<f64>,
}

fn timings_path() -> PathBuf {
    backend::data_dir().join("eta-timings.json")
}

fn cached_timings() -> &'static Mutex<Option<Timings>> {
    static CACHE: OnceLock<Mutex<Option<Timings>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn load_from(path: &Path) -> Timings {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Timings>(&text).ok())
        .unwrap_or_default()
}

fn save_to(path: &Path, timings: &Timings) {
    let Ok(bytes) = serde_json::to_vec_pretty(timings) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, path).is_ok() {
        return;
    }
    let _ = fs::remove_file(&tmp);
    let _ = fs::write(path, &bytes);
}

fn blend_prepare_secs(existing: Option<f64>, elapsed: Duration) -> Option<f64> {
    let secs = elapsed.as_secs_f64();
    if !secs.is_finite() || !(MIN_RECORD_PREPARE_SECS..=MAX_RECORD_PREPARE_SECS).contains(&secs) {
        return existing;
    }
    Some(match existing {
        Some(prev) if prev.is_finite() && prev > 0.0 => {
            HISTORY_ALPHA * secs + (1.0 - HISTORY_ALPHA) * prev
        }
        _ => secs,
    })
}

/// Expected duration of `PrepareStep::Prepare` on this machine, from prior successful runs.
pub(crate) fn expected_prepare_duration() -> Option<Duration> {
    let mut guard = cached_timings().lock().ok()?;
    let timings = guard.get_or_insert_with(|| load_from(&timings_path()));
    timings
        .prepare_secs
        .filter(|s| s.is_finite() && *s >= MIN_RECORD_PREPARE_SECS)
        .map(Duration::from_secs_f64)
}

/// Record a completed `PrepareStep::Prepare` duration to improve future estimates.
pub(crate) fn record_prepare_duration(elapsed: Duration) {
    let path = timings_path();
    let updated = {
        let Ok(mut guard) = cached_timings().lock() else {
            return;
        };
        let timings = guard.get_or_insert_with(|| load_from(&path));
        timings.prepare_secs = blend_prepare_secs(timings.prepare_secs, elapsed);
        timings.clone()
    };
    save_to(&path, &updated);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn determinate_eta_warms_up_and_smooths_rate() {
        let mut eta = StageEta::default();
        let t0 = Instant::now();
        eta.update(2, Some(0.0), t0);
        // Too early: still within warmup.
        eta.update(2, Some(0.10), t0 + Duration::from_secs(1));
        assert_eq!(eta.remaining(t0 + Duration::from_secs(1)), None);

        // At 2s with 20% done (rate = 0.1/s), 80% left -> ~8s.
        eta.update(2, Some(0.20), t0 + Duration::from_secs(2));
        let rem = eta.remaining(t0 + Duration::from_secs(2)).unwrap();
        assert!((rem.as_secs_f64() - 8.0).abs() < 0.2, "got {rem:?}");

        // Switching stage resets the estimator.
        eta.update(8, Some(0.05), t0 + Duration::from_secs(3));
        assert_eq!(eta.remaining(t0 + Duration::from_secs(3)), None);
    }

    #[test]
    fn determinate_eta_hides_when_stalled_or_nearly_done() {
        let mut eta = StageEta::default();
        let t0 = Instant::now();
        eta.update(2, Some(0.0), t0);
        eta.update(2, Some(0.5), t0 + Duration::from_secs(5));
        assert!(eta.remaining(t0 + Duration::from_secs(6)).is_some());
        // Stalled past STALL_TIMEOUT -> hidden.
        assert_eq!(eta.remaining(t0 + Duration::from_secs(25)), None);

        // Nearly complete -> hidden.
        eta.update(2, Some(0.999), t0 + Duration::from_secs(26));
        assert_eq!(eta.remaining(t0 + Duration::from_secs(26)), None);
    }

    #[test]
    fn expected_duration_eta_warms_up_and_drops_when_exceeded() {
        let t0 = Instant::now();
        let expected = Duration::from_secs(30);
        assert_eq!(
            remaining_from_expected(t0, expected, t0 + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            remaining_from_expected(t0, expected, t0 + Duration::from_secs(10)),
            Some(Duration::from_secs(20))
        );
        assert_eq!(
            remaining_from_expected(t0, expected, t0 + Duration::from_secs(30)),
            None
        );
        assert_eq!(
            remaining_from_expected(t0, expected, t0 + Duration::from_secs(45)),
            None
        );
    }

    #[test]
    fn format_eta_uses_calm_coarse_buckets() {
        assert_eq!(format_eta(Duration::from_secs(3)), "A few seconds left");
        assert_eq!(format_eta(Duration::from_secs(7)), "About 5 sec left");
        assert_eq!(format_eta(Duration::from_secs(18)), "About 20 sec left");
        assert_eq!(format_eta(Duration::from_secs(52)), "About 50 sec left");
        assert_eq!(format_eta(Duration::from_secs(60)), "About 1 min left");
        assert_eq!(format_eta(Duration::from_secs(150)), "About 3 min left");
        assert_eq!(format_eta(Duration::from_secs(3600)), "About 1 hr left");
        assert_eq!(
            format_eta(Duration::from_secs(4500)),
            "About 1 hr 15 min left"
        );
    }

    #[test]
    fn timings_round_trip_and_blend() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eta-timings.json");
        let first = blend_prepare_secs(None, Duration::from_secs(20));
        assert_eq!(first, Some(20.0));
        let second = blend_prepare_secs(first, Duration::from_secs(30)).unwrap();
        assert!((second - 23.5).abs() < 1e-6, "got {second}");
        // Out-of-range durations are ignored.
        assert_eq!(
            blend_prepare_secs(Some(second), Duration::from_millis(500)),
            Some(second)
        );
        save_to(
            &path,
            &Timings {
                prepare_secs: Some(second),
            },
        );
        let loaded = load_from(&path);
        assert!((loaded.prepare_secs.unwrap() - second).abs() < 1e-6);
    }
}
