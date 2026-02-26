//! Resiliency helpers for ToolRegistry.

use std::time::Duration;

use super::{ToolLatencyStats, ToolRegistry, ToolTimeoutCategory};

impl ToolRegistry {
    fn scale_duration(duration: Duration, num: u32, denom: u32) -> Duration {
        if denom == 0 {
            return duration;
        }
        let millis = duration.as_millis();
        let scaled = millis
            .saturating_mul(num as u128)
            .saturating_div(denom as u128);
        Duration::from_millis(scaled as u64)
    }

    pub(super) fn effective_timeout(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        let base = self
            .timeout_policy
            .read()
            .ok()
            .and_then(|p| p.ceiling_for(category));
        let adaptive = self
            .resiliency
            .lock()
            .adaptive_timeout_ceiling
            .get(&category)
            .copied();

        match (base, adaptive) {
            (Some(b), Some(a)) if a.as_millis() > 0 => Some(std::cmp::min(b, a)),
            (Some(b), _) => Some(b),
            (None, Some(a)) if a.as_millis() > 0 => Some(a),
            _ => None,
        }
    }

    pub(super) fn decay_adaptive_timeout(&self, category: ToolTimeoutCategory) {
        let mut state = self.resiliency.lock();
        let tuning = state.adaptive_tuning;

        if let Some(adaptive) = state.adaptive_timeout_ceiling.get_mut(&category) {
            if adaptive.as_millis() == 0 {
                return;
            }
            let before = *adaptive;
            if let Some(base) = self
                .timeout_policy
                .read()
                .ok()
                .and_then(|p| p.ceiling_for(category))
            {
                if *adaptive < base {
                    let relaxed_ms =
                        ((*adaptive).as_millis() as f64 * (1.0 / tuning.decay_ratio)) as u128;
                    let relaxed = Duration::from_millis(relaxed_ms as u64);
                    *adaptive = std::cmp::min(relaxed, base);
                }
            } else {
                // If no base, relax upward modestly
                let relaxed = Duration::from_millis(
                    ((*adaptive).as_millis() as f64 * (1.0 / tuning.decay_ratio)) as u64,
                );
                *adaptive = relaxed;
            }

            let floor = Duration::from_millis(tuning.min_floor_ms);
            if *adaptive < floor {
                *adaptive = floor;
            }

            if *adaptive != before {
                tracing::debug!(
                    category = %category.label(),
                    previous_ms = %before.as_millis(),
                    new_ms = %adaptive.as_millis(),
                    decay_ratio = %tuning.decay_ratio,
                    "Adaptive timeout relaxed after success streak"
                );
            }
        }
    }

    pub(super) fn record_tool_failure(&self, category: ToolTimeoutCategory) -> bool {
        let mut state = self.resiliency.lock();
        state.success_trackers.insert(category, 0);
        let tracker = state.failure_trackers.entry(category).or_default();
        tracker.record_failure();
        tracker.should_circuit_break()
    }

    pub(super) fn reset_tool_failure(&self, category: ToolTimeoutCategory) {
        let mut state = self.resiliency.lock();
        if let Some(tracker) = state.failure_trackers.get_mut(&category) {
            tracker.reset();
        }
        state.success_trackers.insert(category, 0);
    }

    pub(super) fn record_tool_latency(&self, category: ToolTimeoutCategory, duration: Duration) {
        let mut state = self.resiliency.lock();
        let tuning = state.adaptive_tuning;

        let stats = state
            .latency_stats
            .entry(category)
            .or_insert_with(|| ToolLatencyStats::new(50));
        stats.record(duration);

        if let Some(p95) = stats.percentile(0.95) {
            if let Some(ceiling) = self
                .timeout_policy
                .read()
                .ok()
                .and_then(|p| p.ceiling_for(category))
            {
                if p95 > ceiling {
                    tracing::warn!(
                        category = %category.label(),
                        p95_ms = %p95.as_millis(),
                        ceiling_ms = %ceiling.as_millis(),
                        "Observed p95 tool latency exceeds configured ceiling; consider adjusting timeouts"
                    );
                    let adjusted = std::cmp::min(
                        ceiling,
                        std::cmp::max(
                            Duration::from_millis(tuning.min_floor_ms),
                            Self::scale_duration(p95, 11, 10),
                        ),
                    );
                    state.adaptive_timeout_ceiling.insert(category, adjusted);
                    tracing::debug!(
                        category = %category.label(),
                        new_ceiling_ms = %adjusted.as_millis(),
                        "Adaptive timeout ceiling applied from p95 latency"
                    );
                }
            } else {
                // No ceiling configured; derive one from p95 with headroom
                let derived = std::cmp::max(
                    Duration::from_millis(tuning.min_floor_ms),
                    Self::scale_duration(p95, 12, 10),
                );
                state.adaptive_timeout_ceiling.insert(category, derived);
                tracing::debug!(
                    category = %category.label(),
                    new_ceiling_ms = %derived.as_millis(),
                    "Adaptive timeout ceiling derived from p95 latency without static ceiling"
                );
            }
        }
    }

    pub(super) fn should_circuit_break(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        self.resiliency
            .lock()
            .failure_trackers
            .get(&category)
            .filter(|tracker| tracker.should_circuit_break())
            .map(|tracker| tracker.backoff_duration())
    }
}
