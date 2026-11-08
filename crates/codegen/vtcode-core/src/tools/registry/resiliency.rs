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
    #[must_use]
    pub fn backoff_duration(&self) -> Duration {
        // Canonical 500ms-to-5s curve with the standard jitter. The old local
        // exponent cap is obsolete: the canonical curve clamps at the cap for
        // any attempt index.
        let attempt = self.consecutive_failures.saturating_sub(5);
        crate::retry::RetryPolicy::from_retries(u32::MAX, Duration::from_millis(500), Duration::from_millis(5000), 2.0)
            .with_jitter(crate::retry::RetryPolicy::DEFAULT_JITTER)
            .delay_for_attempt(attempt)
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
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(1031));

        for _ in 0..2 {
            tracker.record_failure();
        }
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(4093));

        tracker.record_failure();
        assert_eq!(tracker.backoff_duration(), Duration::from_millis(5124));
    }
}
