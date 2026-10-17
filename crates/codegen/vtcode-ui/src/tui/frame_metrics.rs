//! Lean opt-in frame timing for TUI jank diagnosis.
//!
//! Enabled with `VTCODE_TUI_FRAME_METRICS=1`. When disabled every entry point
//! returns immediately so the hot path stays free of sampling work.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use std::sync::{LazyLock, Mutex};

const ENV_FLAG: &str = "VTCODE_TUI_FRAME_METRICS";
const RING_LEN: usize = 256;
const REPORT_INTERVAL: Duration = Duration::from_secs(5);
/// Matches `runner::drive::DRAW_WARN_MS`.
const SLOW_DRAW_MS: u128 = 8;
/// Matches `runner::drive::INPUT_TO_DRAW_WARN_MS`.
const SLOW_INPUT_TO_DRAW_MS: u128 = 16;

static ENABLED: AtomicBool = AtomicBool::new(false);

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Read the env flag once. Safe to call repeatedly; only the first call reads env.
pub fn initialize_from_env() {
    if enabled() {
        return;
    }
    let on = std::env::var_os(ENV_FLAG).is_some_and(|value| {
        let value = value.to_string_lossy();
        matches!(value.as_ref(), "1" | "true" | "yes" | "on")
    });
    ENABLED.store(on, Ordering::Relaxed);
}

struct Ring {
    /// Draw durations in microseconds, oldest first, fixed capacity.
    samples_us: [u32; RING_LEN],
    len: usize,
    next: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self { samples_us: [0; RING_LEN], len: 0, next: 0 }
    }
}

impl Ring {
    fn record(&mut self, duration: Duration) {
        let us = duration.as_micros().min(u32::MAX as u128) as u32;
        self.samples_us[self.next] = us;
        self.next = (self.next + 1) % RING_LEN;
        if self.len < RING_LEN {
            self.len += 1;
        }
    }

    /// Percentile in \[0, 100\] using nearest-rank. Empty ring returns 0.
    fn percentile_us(&self, pct: u8) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut sorted = [0u32; RING_LEN];
        sorted[..self.len].copy_from_slice(&self.samples_us[..self.len]);
        sorted[..self.len].sort_unstable();
        let rank = (usize::from(pct) * self.len).div_ceil(100);
        let idx = rank.saturating_sub(1).min(self.len - 1);
        sorted[idx]
    }
}

struct Counters {
    frames_drawn: u64,
    slow_draws: u64,
    slow_input_to_draw: u64,
    max_draw_us: u32,
    max_input_to_draw_us: u32,
    draw_ring: Ring,
    input_to_draw_ring: Ring,
    last_report: Option<Instant>,
}

impl Counters {
    fn new() -> Self {
        Self {
            frames_drawn: 0,
            slow_draws: 0,
            slow_input_to_draw: 0,
            max_draw_us: 0,
            max_input_to_draw_us: 0,
            draw_ring: Ring::default(),
            input_to_draw_ring: Ring::default(),
            last_report: None,
        }
    }
}

static COUNTERS: LazyLock<Mutex<Counters>> = LazyLock::new(|| Mutex::new(Counters::new()));
static FRAMES_SKIPPED: AtomicU64 = AtomicU64::new(0);

/// Record a completed draw. `input_to_draw` is the latency from the first input
/// event of the batch to draw completion, when that batch carried input.
pub fn record_draw(draw: Duration, input_to_draw: Option<Duration>) {
    if !enabled() {
        return;
    }
    let draw_us = draw.as_micros().min(u32::MAX as u128) as u32;
    let mut counters = match COUNTERS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    counters.frames_drawn = counters.frames_drawn.saturating_add(1);
    counters.max_draw_us = counters.max_draw_us.max(draw_us);
    counters.draw_ring.record(draw);
    if draw.as_millis() >= SLOW_DRAW_MS {
        counters.slow_draws = counters.slow_draws.saturating_add(1);
    }
    if let Some(input_to_draw) = input_to_draw {
        let itd_us = input_to_draw.as_micros().min(u32::MAX as u128) as u32;
        counters.max_input_to_draw_us = counters.max_input_to_draw_us.max(itd_us);
        counters.input_to_draw_ring.record(input_to_draw);
        if input_to_draw.as_millis() >= SLOW_INPUT_TO_DRAW_MS {
            counters.slow_input_to_draw = counters.slow_input_to_draw.saturating_add(1);
        }
    }

    let due = counters.last_report.is_none_or(|last| last.elapsed() >= REPORT_INTERVAL);
    if due {
        counters.last_report = Some(Instant::now());
        let draw_p50 = counters.draw_ring.percentile_us(50);
        let draw_p95 = counters.draw_ring.percentile_us(95);
        let itd_p50 = counters.input_to_draw_ring.percentile_us(50);
        let itd_p95 = counters.input_to_draw_ring.percentile_us(95);
        tracing::debug!(
            target: "vtcode.tui.latency",
            frames_drawn = counters.frames_drawn,
            frames_skipped = FRAMES_SKIPPED.load(Ordering::Relaxed),
            slow_draws = counters.slow_draws,
            slow_input_to_draw = counters.slow_input_to_draw,
            draw_p50_us = draw_p50,
            draw_p95_us = draw_p95,
            draw_max_us = counters.max_draw_us,
            input_to_draw_p50_us = itd_p50,
            input_to_draw_p95_us = itd_p95,
            input_to_draw_max_us = counters.max_input_to_draw_us,
            "tui frame metrics window"
        );
    }
}

/// Count a wakeup that decided no redraw was needed (dirty flag clear).
pub fn record_frame_skipped() {
    if !enabled() {
        return;
    }
    FRAMES_SKIPPED.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_percentiles_match_sorted_order() {
        let mut ring = Ring::default();
        for value in [10u32, 20, 30, 40] {
            ring.record(Duration::from_micros(u64::from(value)));
        }
        assert_eq!(ring.percentile_us(0), 10);
        assert_eq!(ring.percentile_us(50), 20);
        assert_eq!(ring.percentile_us(99), 40);
    }

    #[test]
    fn ring_wraps_without_growing() {
        let mut ring = Ring::default();
        for i in 0..(RING_LEN + 10) {
            ring.record(Duration::from_micros(i as u64));
        }
        assert_eq!(ring.len, RING_LEN);
    }

    #[test]
    fn record_paths_are_inert_when_sampling_disabled() {
        // Force-off so this test does not depend on the developer's env.
        ENABLED.store(false, Ordering::Relaxed);
        record_draw(Duration::from_millis(50), Some(Duration::from_millis(80)));
        record_frame_skipped();
        assert!(!enabled());
    }
}
