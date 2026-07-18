//! Canonical retry policy shared across the workspace.
//!
//! This module owns the retry *policy math*: attempt budgets, exponential
//! backoff with an optional deterministic jitter, and category-based retry
//! decisions built on [`ErrorCategory::is_retryable`]. Domain-specific
//! adapters (typed error downcasts, tool-aware timeout rules, LLM
//! `Retry-After` extraction) live in `vtcode-core::retry` as an extension
//! trait over this policy.
//!
//! Wire-level HTTP clients that only need "should I retry this call?" use
//! [`RetryPolicy::classify_anyhow`] / [`RetryPolicy::classify_status`];
//! richer loops use [`RetryPolicy::decision_for_category`].

use std::time::Duration;

use crate::error_category::{ErrorCategory, classify_anyhow_error};

/// Typed retry policy shared across runtime layers.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of total attempts, including the initial call.
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: f64,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, initial_delay: Duration, max_delay: Duration, multiplier: f64) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            initial_delay,
            max_delay,
            multiplier: multiplier.max(1.0),
            jitter: 0.0,
        }
    }

    pub fn from_retries(max_retries: u32, initial_delay: Duration, max_delay: Duration, multiplier: f64) -> Self {
        Self::new(max_retries.saturating_add(1), initial_delay, max_delay, multiplier)
    }

    /// Millisecond-based constructor for wire clients.
    ///
    /// Uses a 2.0 multiplier and no jitter, so
    /// [`Self::delay_for_attempt`] reproduces the classic
    /// `base_ms << attempt` doubling curve capped at `max_delay_ms`.
    pub fn simple(max_retries: u32, base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self::from_retries(max_retries, Duration::from_millis(base_delay_ms), Duration::from_millis(max_delay_ms), 2.0)
    }

    pub fn delay_for_attempt(&self, attempt_index: u32) -> Duration {
        let multiplier = self.multiplier.powi(attempt_index as i32);
        let base_delay = Duration::try_from_secs_f64(self.initial_delay.as_secs_f64() * multiplier)
            .unwrap_or(self.max_delay)
            .min(self.max_delay);

        if !self.jitter.is_finite() || self.jitter <= 0.0 {
            return base_delay;
        }

        #[allow(clippy::cast_sign_loss)]
        let max_jitter_ms = (base_delay.as_millis() as f64 * self.jitter)
            .round()
            .clamp(0.0, u64::MAX as f64) as u64;
        if max_jitter_ms == 0 {
            return base_delay;
        }

        let offset = (u64::from(attempt_index) * 31) % max_jitter_ms.saturating_add(1);
        base_delay.saturating_add(Duration::from_millis(offset))
    }

    pub fn decision_for_category(
        &self,
        category: ErrorCategory,
        attempt_index: u32,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        let has_remaining_attempts = attempt_index.saturating_add(1) < self.max_attempts;
        if !category.is_retryable() || !has_remaining_attempts {
            return RetryDecision {
                category,
                retryable: false,
                delay: None,
                retry_after,
            };
        }

        let delay = retry_after.unwrap_or_else(|| self.delay_for_attempt(attempt_index));
        RetryDecision {
            category,
            retryable: true,
            delay: Some(delay),
            retry_after,
        }
    }

    /// Classify an `anyhow::Error` for retry eligibility.
    ///
    /// Attempt-agnostic: `retryable` reflects only the error category, not
    /// the remaining attempt budget. Wire clients that manage their own
    /// attempt counting use this; loops that want budget-aware decisions
    /// use [`Self::decision_for_category`].
    pub fn classify_anyhow(&self, error: &anyhow::Error) -> RetryDecision {
        let category = classify_anyhow_error(error);
        RetryDecision {
            category,
            retryable: category.is_retryable(),
            delay: None,
            retry_after: None,
        }
    }

    /// Classify an HTTP status code for retry eligibility.
    ///
    /// Attempt-agnostic, like [`Self::classify_anyhow`].
    pub fn classify_status(&self, status: u16) -> RetryDecision {
        let category = match status {
            429 => ErrorCategory::RateLimit,
            500 | 502 | 504 => ErrorCategory::Network,
            503 => ErrorCategory::ServiceUnavailable,
            401 | 403 => ErrorCategory::Authentication,
            _ => ErrorCategory::ExecutionError,
        };
        RetryDecision {
            category,
            retryable: category.is_retryable(),
            delay: None,
            retry_after: None,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::from_retries(2, Duration::from_secs(1), Duration::from_secs(60), 2.0)
    }
}

/// Result of classifying a failure for retry handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDecision {
    pub category: ErrorCategory,
    pub retryable: bool,
    pub delay: Option<Duration>,
    pub retry_after: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_allows_two_retries() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.initial_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(60));
    }

    #[test]
    fn classify_status_rate_limit() {
        let policy = RetryPolicy::default();
        let decision = policy.classify_status(429);
        assert!(decision.retryable);
        assert_eq!(decision.category, ErrorCategory::RateLimit);
    }

    #[test]
    fn classify_status_server_error() {
        let policy = RetryPolicy::default();
        let decision = policy.classify_status(503);
        assert!(decision.retryable);
        assert_eq!(decision.category, ErrorCategory::ServiceUnavailable);
    }

    #[test]
    fn classify_status_auth_not_retryable() {
        let policy = RetryPolicy::default();
        let decision = policy.classify_status(401);
        assert!(!decision.retryable);
        assert_eq!(decision.category, ErrorCategory::Authentication);
    }

    #[test]
    fn classify_anyhow_network_error() {
        let policy = RetryPolicy::default();
        let err = anyhow::anyhow!("connection refused");
        let decision = policy.classify_anyhow(&err);
        assert!(decision.retryable);
    }

    #[test]
    fn simple_policy_matches_bit_shift_doubling() {
        // Parity with the historical `base_ms << attempt` curve used by
        // wire clients before consolidation.
        let policy = RetryPolicy::simple(10, 1000, 5000);
        let legacy = |attempt: u32| -> u64 { 1000u64.saturating_mul(1u64 << attempt.min(16)).min(5000) };
        for attempt in 0..6 {
            assert_eq!(
                policy.delay_for_attempt(attempt),
                Duration::from_millis(legacy(attempt)),
                "delay mismatch at attempt {attempt}"
            );
        }
    }

    #[test]
    fn delay_for_attempt_clamps_overflowing_backoff_to_max_delay() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), f64::MAX);

        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(8));
    }

    #[test]
    fn delay_for_attempt_ignores_non_finite_jitter() {
        let mut policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        policy.jitter = f64::INFINITY;

        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(2));
    }

    #[test]
    fn delay_for_attempt_handles_huge_finite_jitter() {
        let mut policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);
        policy.jitter = f64::MAX;

        assert!(policy.delay_for_attempt(1) >= Duration::from_secs(2));
    }

    #[test]
    fn decision_for_category_respects_attempt_budget() {
        let policy = RetryPolicy::from_retries(1, Duration::from_secs(1), Duration::from_secs(8), 2.0);

        let first = policy.decision_for_category(ErrorCategory::Network, 0, None);
        assert!(first.retryable);
        assert_eq!(first.delay, Some(Duration::from_secs(1)));

        let exhausted = policy.decision_for_category(ErrorCategory::Network, 1, None);
        assert!(!exhausted.retryable);
        assert!(exhausted.delay.is_none());
    }

    #[test]
    fn decision_for_category_prefers_retry_after() {
        let policy = RetryPolicy::from_retries(3, Duration::from_secs(1), Duration::from_secs(8), 2.0);

        let decision = policy.decision_for_category(ErrorCategory::RateLimit, 0, Some(Duration::from_secs(7)));
        assert!(decision.retryable);
        assert_eq!(decision.delay, Some(Duration::from_secs(7)));
        assert_eq!(decision.retry_after, Some(Duration::from_secs(7)));
    }
}
