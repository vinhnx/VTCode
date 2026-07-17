//! Tool failure tracking and circuit breaker logic.
//!
//! This module provides resiliency patterns for tool execution,
//! including consecutive failure tracking and backoff mechanisms.

use hashbrown::HashMap;
use std::time::Duration;

use super::timeout::{AdaptiveTimeoutTuning, ToolLatencyStats, ToolTimeoutCategory};

/// Tracks consecutive failures for a tool to enable circuit breaking.
#[derive(Debug, Clone, Default)]
pub struct ToolFailureTracker {
    pub(super) consecutive_failures: u32,
}

impl ToolFailureTracker {
    /// Record a failure.
    pub fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    /// Reset the failure counter (on success).
    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Check if the circuit breaker should trip.
    pub fn should_circuit_break(&self) -> bool {
        self.consecutive_failures >= 5
    }

    /// Calculate backoff duration based on failure count.
    pub fn backoff_duration(&self) -> Duration {
        let base_ms = 500;
        let max_ms = 5_000;
        let failures = self.consecutive_failures.saturating_sub(5);
        let backoff_ms = base_ms * 2_u64.pow(failures.min(8));
        Duration::from_millis(backoff_ms.min(max_ms))
    }
}

/// Internal state for resiliency tracking across tool categories.
#[derive(Clone, Debug, Default)]
pub struct ResiliencyContext {
    pub(super) adaptive_timeout_ceiling: HashMap<ToolTimeoutCategory, Duration>,
    pub(super) failure_trackers: HashMap<ToolTimeoutCategory, ToolFailureTracker>,
    pub(super) success_trackers: HashMap<ToolTimeoutCategory, u32>,
    pub(super) latency_stats: HashMap<ToolTimeoutCategory, ToolLatencyStats>,
    pub(super) adaptive_tuning: AdaptiveTimeoutTuning,
}

#[cfg(test)]
mod tests {
    use super::ToolFailureTracker;
    use std::time::Duration;

    #[test]
    fn circuit_breaker_threshold_and_backoff_ramp() {
        let mut tracker = ToolFailureTracker::default();

        for _ in 0..5 {
            tracker.record_failure();
        }
        assert!(tracker.should_circuit_break());
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(500));

        tracker.record_failure();
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(1000));

        for _ in 0..2 {
            tracker.record_failure();
        }
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(4000));

        tracker.record_failure();
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(5000));
    }
}
