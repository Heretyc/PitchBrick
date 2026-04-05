/// Vocal Rest Timer — rolling 1-hour window tracker for green-range training time.
///
/// Tracks milliseconds spent in the "green" (target range) display state using
/// a sliding window. When accumulated training time exceeds a configurable
/// threshold, triggers overage mode (yellow display, alert chimes, tooltip).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Persistence types
// ---------------------------------------------------------------------------

/// A completed span of time the user spent in the green (target) range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GreenSpan {
    /// Milliseconds since Unix epoch when this span started.
    pub start_ms: u64,
    /// Duration of this span in milliseconds.
    pub duration_ms: u64,
}

/// On-disk format for `~/vocal_rest.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VocalRestFile {
    #[serde(default)]
    pub spans: Vec<GreenSpan>,
}

/// Embedded WAV file: a voice saying "give your voice a rest."
pub static REST_WAV: &[u8] = include_bytes!("../sounds/rest.wav");

// ---------------------------------------------------------------------------
// Core tracker
// ---------------------------------------------------------------------------

const ONE_HOUR_MS: u64 = 3_600_000;
const REST_SOUND_REPEAT: Duration = Duration::from_secs(60);
const FLUSH_INTERVAL: Duration = Duration::from_secs(120);
const TOOLTIP_COOLDOWN: Duration = Duration::from_secs(3);

/// Returns current wall-clock time as milliseconds since Unix epoch.
fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct VocalRestTracker {
    /// Completed green spans within the rolling 1-hour window.
    spans: Vec<GreenSpan>,
    /// Monotonic start of the currently-open green span (if any).
    current_span_start: Option<Instant>,
    /// Wall-clock start of the currently-open green span (for persistence).
    current_span_epoch_ms: Option<u64>,
    /// Whether accumulated training time exceeds the threshold.
    pub in_overage: bool,
    /// When the rest sound was last played (for 60 s repeat).
    last_rest_sound_at: Option<Instant>,
    /// Last time we showed the red-replacement tooltip (3 s cooldown).
    last_red_tooltip: Option<Instant>,
    /// Last time we flushed to disk.
    last_flush: Instant,
}

impl VocalRestTracker {
    // -- Construction / persistence -----------------------------------------

    /// Creates a new empty tracker.
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            current_span_start: None,
            current_span_epoch_ms: None,
            in_overage: false,
            last_rest_sound_at: None,
            last_red_tooltip: None,
            last_flush: Instant::now(),
        }
    }

    /// Canonical file path: `~/vocal_rest.toml`.
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vocal_rest.toml")
    }

    /// Loads persisted spans from disk, pruning anything older than 1 hour.
    pub fn load(path: &Path) -> Self {
        let mut tracker = Self::new();
        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Ok(file) = toml::from_str::<VocalRestFile>(&contents) {
                tracker.spans = file.spans;
                tracker.prune_old_spans();
            }
        }
        tracker
    }

    /// Writes all current spans to disk using atomic temp-file + rename.
    pub fn flush(&self, path: &Path) {
        let mut all_spans = self.spans.clone();
        // Include the currently-open span if any.
        if let (Some(start), Some(epoch_start)) =
            (self.current_span_start, self.current_span_epoch_ms)
        {
            let elapsed = Instant::now().duration_since(start).as_millis() as u64;
            all_spans.push(GreenSpan {
                start_ms: epoch_start,
                duration_ms: elapsed,
            });
        }
        let file = VocalRestFile { spans: all_spans };
        let Ok(toml_str) = toml::to_string_pretty(&file) else {
            tracing::error!("vocal_rest: failed to serialize");
            return;
        };
        let tmp = path.with_extension("toml.tmp");
        if let Err(e) = std::fs::write(&tmp, &toml_str) {
            tracing::error!("vocal_rest: failed to write tmp: {}", e);
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            tracing::error!("vocal_rest: failed to rename: {}", e);
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Flushes to disk if at least 2 minutes have elapsed since last flush.
    pub fn flush_if_due(&mut self, path: &Path, now: Instant) {
        if now.duration_since(self.last_flush) >= FLUSH_INTERVAL {
            self.flush(path);
            self.last_flush = now;
        }
    }

    // -- Rolling window -----------------------------------------------------

    /// Removes spans whose end time is more than 1 hour in the past.
    pub fn prune_old_spans(&mut self) {
        let cutoff = now_epoch_ms().saturating_sub(ONE_HOUR_MS);
        self.spans
            .retain(|s| s.start_ms + s.duration_ms > cutoff);
    }

    /// Total green milliseconds in the rolling 1-hour window, including the
    /// currently-open span.
    pub fn accumulated_ms(&self, now: Instant) -> u64 {
        let cutoff = now_epoch_ms().saturating_sub(ONE_HOUR_MS);
        let completed: u64 = self
            .spans
            .iter()
            .filter(|s| s.start_ms + s.duration_ms > cutoff)
            .map(|s| s.duration_ms)
            .sum();
        let open = match self.current_span_start {
            Some(start) => now.duration_since(start).as_millis() as u64,
            None => 0,
        };
        completed + open
    }

    // -- Green state enter / exit -------------------------------------------

    /// Call when the raw display state becomes Green (and was not Green before).
    pub fn on_green_enter(&mut self, _now: Instant) {
        if self.current_span_start.is_none() {
            self.current_span_start = Some(Instant::now());
            self.current_span_epoch_ms = Some(now_epoch_ms());
        }
    }

    /// Call when the raw display state leaves Green (and was Green before).
    /// Closes the current span and pushes it to the completed list.
    pub fn on_green_exit(&mut self, now: Instant) {
        if let (Some(start), Some(epoch_start)) =
            (self.current_span_start, self.current_span_epoch_ms)
        {
            let duration_ms = now.duration_since(start).as_millis() as u64;
            if duration_ms > 0 {
                self.spans.push(GreenSpan {
                    start_ms: epoch_start,
                    duration_ms,
                });
            }
        }
        self.current_span_start = None;
        self.current_span_epoch_ms = None;
    }

    // -- Overage detection --------------------------------------------------

    /// Updates overage state based on accumulated time vs threshold.
    /// `threshold_minutes` of 0 means OFF — overage is always false.
    /// Returns `true` if we just *entered* overage this tick.
    pub fn update_overage(&mut self, now: Instant, threshold_minutes: u32) -> bool {
        let was_overage = self.in_overage;
        self.in_overage = if threshold_minutes == 0 {
            false
        } else {
            self.accumulated_ms(now) >= (threshold_minutes as u64) * 60_000
        };
        // Just entered overage.
        !was_overage && self.in_overage
    }

    // -- Rest sound timing ---------------------------------------------------

    /// Returns `true` if the rest sound should play now (on overage entry,
    /// then every 60 seconds while the user remains in green during overage).
    pub fn should_play_rest_sound(&mut self, now: Instant) -> bool {
        let due = match self.last_rest_sound_at {
            Some(t) => now.duration_since(t) >= REST_SOUND_REPEAT,
            None => true,
        };
        if due {
            self.last_rest_sound_at = Some(now);
        }
        due
    }

    // -- Red tooltip rate limiter -------------------------------------------

    /// Returns `true` if enough time has passed to show another rest tooltip
    /// (3-second cooldown).
    pub fn should_show_red_tooltip(&mut self, now: Instant) -> bool {
        let show = match self.last_red_tooltip {
            Some(t) => now.duration_since(t) >= TOOLTIP_COOLDOWN,
            None => true,
        };
        if show {
            self.last_red_tooltip = Some(now);
        }
        show
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tracker_zero_accumulated() {
        let tracker = VocalRestTracker::new();
        assert_eq!(tracker.accumulated_ms(Instant::now()), 0);
    }

    #[test]
    fn test_single_span_accumulation() {
        let mut tracker = VocalRestTracker::new();
        let epoch = now_epoch_ms();
        tracker.spans.push(GreenSpan {
            start_ms: epoch - 5000,
            duration_ms: 5000,
        });
        let acc = tracker.accumulated_ms(Instant::now());
        assert_eq!(acc, 5000);
    }

    #[test]
    fn test_prune_removes_old_spans() {
        let mut tracker = VocalRestTracker::new();
        // Span that ended 2 hours ago.
        tracker.spans.push(GreenSpan {
            start_ms: now_epoch_ms() - 7_200_000,
            duration_ms: 1000,
        });
        // Span that ended 30 minutes ago.
        tracker.spans.push(GreenSpan {
            start_ms: now_epoch_ms() - 1_800_000,
            duration_ms: 1000,
        });
        tracker.prune_old_spans();
        assert_eq!(tracker.spans.len(), 1);
        assert_eq!(tracker.spans[0].duration_ms, 1000);
    }

    #[test]
    fn test_persistence_round_trip() {
        let dir = std::env::temp_dir();
        let path = dir.join("pitchbrick_test_vocal_rest.toml");

        let mut tracker = VocalRestTracker::new();
        let epoch = now_epoch_ms();
        tracker.spans.push(GreenSpan {
            start_ms: epoch - 60_000,
            duration_ms: 30_000,
        });
        tracker.spans.push(GreenSpan {
            start_ms: epoch - 20_000,
            duration_ms: 10_000,
        });
        tracker.flush(&path);

        let loaded = VocalRestTracker::load(&path);
        assert_eq!(loaded.spans.len(), 2);
        assert_eq!(loaded.spans[0], tracker.spans[0]);
        assert_eq!(loaded.spans[1], tracker.spans[1]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_overage_detection() {
        let mut tracker = VocalRestTracker::new();
        let epoch = now_epoch_ms();
        // 31 minutes of green time.
        tracker.spans.push(GreenSpan {
            start_ms: epoch - 1_860_000,
            duration_ms: 1_860_000,
        });
        let now = Instant::now();
        let entered = tracker.update_overage(now, 30);
        assert!(entered);
        assert!(tracker.in_overage);
    }

    #[test]
    fn test_overage_stays_false_when_off() {
        let mut tracker = VocalRestTracker::new();
        let epoch = now_epoch_ms();
        tracker.spans.push(GreenSpan {
            start_ms: epoch - 1_860_000,
            duration_ms: 1_860_000,
        });
        let now = Instant::now();
        let entered = tracker.update_overage(now, 0);
        assert!(!entered);
        assert!(!tracker.in_overage);
    }

    #[test]
    fn test_rest_sound_repeat_timing() {
        let mut tracker = VocalRestTracker::new();
        let now = Instant::now();

        // First call: should play.
        assert!(tracker.should_play_rest_sound(now));
        // Immediately after: should not repeat.
        assert!(!tracker.should_play_rest_sound(now + Duration::from_secs(30)));
        // After 60 seconds: should play again.
        assert!(tracker.should_play_rest_sound(now + Duration::from_secs(61)));
    }

    #[test]
    fn test_tooltip_rate_limiting() {
        let mut tracker = VocalRestTracker::new();
        let now = Instant::now();

        assert!(tracker.should_show_red_tooltip(now));
        // Immediately after: should be blocked.
        assert!(!tracker.should_show_red_tooltip(now + Duration::from_millis(100)));
        // After 3 seconds: should be allowed again.
        assert!(tracker.should_show_red_tooltip(now + Duration::from_secs(4)));
    }

    #[test]
    fn test_green_enter_exit_creates_span() {
        let mut tracker = VocalRestTracker::new();
        let start = Instant::now();
        tracker.on_green_enter(start);
        assert!(tracker.current_span_start.is_some());

        let end = start + Duration::from_millis(5000);
        tracker.on_green_exit(end);
        assert!(tracker.current_span_start.is_none());
        assert_eq!(tracker.spans.len(), 1);
        assert!(tracker.spans[0].duration_ms >= 4900); // allow small clock drift
    }

    #[test]
    fn test_rest_wav_embedded() {
        // Verify the embedded WAV is non-empty and starts with RIFF header.
        assert!(REST_WAV.len() > 44, "WAV file should be larger than header");
        assert_eq!(&REST_WAV[0..4], b"RIFF", "WAV should start with RIFF");
    }
}
