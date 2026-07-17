//! Resilience primitives for tool execution.
//!
//! Collects the concerns that govern how tools recover from and bound
//! transient failures:
//!
//! - [`circuit_breaker`] - state-machine fault isolation (Closed/Open/HalfOpen)
//!   keyed by tool name. Tracks failure counts, applies exponential backoff,
//!   and short-circuits calls when a downstream is degraded.
//! - [`adaptive_rate_limiter`] - per-key token-bucket rate limiter with adaptive
//!   refill and priority weighting. Used to throttle tools whose cost varies
//!   with the call.
//! - [`rate_limiter`] - per-tool budgeted counter rate limiter. Provides the
//!   per-tool token bucket that the executor uses for synchronous admission.
//! - [`ToolResilience`] - the unified facade. New callers should use this
//!   rather than reaching for the individual primitives, so that all three
//!   invariants (admission, fault isolation, post-call accounting) are checked
//!   in one place.
//!
//! The primitives share callers (autonomous executor, tool pipeline, agent
//! error recovery) but address distinct failure modes; they are grouped here
//! so a maintainer can audit the full resilience toolkit in one place.

pub mod adaptive_rate_limiter;
pub mod circuit_breaker;
pub mod rate_limiter;

use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;

use vtcode_commons::ErrorCategory;

use self::adaptive_rate_limiter::{AdaptiveRateLimiter, Priority};
use self::circuit_breaker::CircuitBreaker;

/// Outcome of a tool call, used by [`ToolResilience::record_outcome`] to update
/// the circuit breaker.
#[derive(Debug, Clone, Copy)]
pub enum CallOutcome {
    /// The call succeeded. Resets the failure counter for the tool.
    Success,
    /// The call failed due to a non-retryable error (invalid arguments, planning workflow
    /// denial, permission denial). Does not trip the circuit breaker.
    InvalidArgument,
    /// The call failed due to a retryable execution error. May trip the circuit
    /// breaker after `failure_threshold` consecutive occurrences.
    ExecutionError,
    /// The call failed due to cancellation (Drop guard, timeout, user abort).
    /// Treated like an execution error for breaker accounting.
    Cancelled,
}

impl CallOutcome {
    /// Map this outcome to the `ErrorCategory` used by the circuit breaker for
    /// non-success outcomes. Returns `None` for [`CallOutcome::Success`] because
    /// success is handled by a separate code path (`record_success`).
    fn to_error_category(self) -> Option<ErrorCategory> {
        match self {
            CallOutcome::Success => None,
            CallOutcome::InvalidArgument => Some(ErrorCategory::InvalidParameters),
            CallOutcome::ExecutionError | CallOutcome::Cancelled => {
                Some(ErrorCategory::ExecutionError)
            }
        }
    }
}

/// Unified facade for tool resilience. Wraps the adaptive rate limiter and the
/// circuit breaker so callers can use a single API for admission, fault
/// isolation, and post-call accounting.
///
/// # Example
///
/// ```ignore
/// use vtcode_core::tools::resilience::{GLOBAL_TOOL_RESILIENCE, CallOutcome, Priority};
///
/// // On entry:
/// GLOBAL_TOOL_RESILIENCE
///     .try_acquire("read_file", Priority::Normal)
///     .map_err(|wait| anyhow!("rate limited; retry after {wait:?}"))?;
///
/// // On exit:
/// match result {
///     Ok(_) => GLOBAL_TOOL_RESILIENCE.record_success("read_file"),
///     Err(e) if e.is_argument_error() => {
///         GLOBAL_TOOL_RESILIENCE.record_outcome("read_file", CallOutcome::InvalidArgument);
///     }
///     Err(_) => {
///         GLOBAL_TOOL_RESILIENCE.record_outcome("read_file", CallOutcome::ExecutionError);
///     }
/// }
/// ```
pub struct ToolResilience {
    rate_limiter: AdaptiveRateLimiter,
    circuit_breaker: CircuitBreaker,
}

impl ToolResilience {
    /// Construct a new facade with the supplied adaptive rate limiter and
    /// circuit breaker.
    pub fn new(rate_limiter: AdaptiveRateLimiter, circuit_breaker: CircuitBreaker) -> Self {
        Self {
            rate_limiter,
            circuit_breaker,
        }
    }

    /// Try to acquire a token for the tool. Returns `Ok(())` when the call is
    /// allowed. When the tool is currently rate limited the suggested wait
    /// duration is returned in `Err`.
    pub fn try_acquire(&self, tool_name: &str, priority: Priority) -> Result<(), Duration> {
        // 1. Fault isolation first: a tool with an open circuit is rejected
        //    immediately, even if the rate limiter has tokens to spare.
        if !self.circuit_breaker.allow_request_for_tool(tool_name) {
            let backoff = self
                .circuit_breaker
                .remaining_backoff(tool_name)
                .unwrap_or_else(|| Duration::from_millis(100));
            return Err(backoff);
        }
        // 2. Rate limit. Configure the priority (idempotent).
        self.rate_limiter.set_priority(tool_name, priority);
        self.rate_limiter.try_acquire(tool_name)
    }

    /// Record a successful call. Closes the circuit if it was HalfOpen.
    pub fn record_success(&self, tool_name: &str) {
        self.circuit_breaker.record_success_for_tool(tool_name);
    }

    /// Record a non-success outcome. Categorical errors that should not trip
    /// the breaker (`InvalidArgument`) are routed through
    /// `CallOutcome::InvalidArgument`. See [`CallOutcome`].
    pub fn record_outcome(&self, tool_name: &str, outcome: CallOutcome) {
        match outcome.to_error_category() {
            // Success: reset the breaker (also covers HalfOpen -> Closed).
            None => self.circuit_breaker.record_success_for_tool(tool_name),
            // Failure: the breaker API is a no-op for non-circuit-breaking
            // categories, so InvalidArgument collapses to a harmless call.
            Some(category) => self
                .circuit_breaker
                .record_failure_category_for_tool(tool_name, category),
        }
    }

    /// Diagnostic snapshot of the circuit breaker.
    pub fn circuit_snapshot(&self) -> circuit_breaker::CircuitBreakerSnapshot {
        self.circuit_breaker.snapshot()
    }
}

/// Process-wide resilience facade. Constructed lazily from the shared adaptive
/// rate limiter and a default circuit breaker.
pub static GLOBAL_TOOL_RESILIENCE: Lazy<Arc<ToolResilience>> = Lazy::new(|| {
    Arc::new(ToolResilience::new(
        AdaptiveRateLimiter::default(),
        CircuitBreaker::default(),
    ))
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_records_success_and_failures() {
        let resilience = ToolResilience::new(
            AdaptiveRateLimiter::new(8.0, 4.0),
            CircuitBreaker::new(circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 2,
                ..Default::default()
            }),
        );

        // First two calls allowed; record two execution errors to open the circuit.
        resilience
            .try_acquire("alpha", Priority::Normal)
            .expect("first call allowed");
        resilience.record_outcome("alpha", CallOutcome::ExecutionError);

        resilience
            .try_acquire("alpha", Priority::Normal)
            .expect("second call allowed");
        resilience.record_outcome("alpha", CallOutcome::ExecutionError);

        // Third call must be rejected by the circuit breaker.
        let third = resilience.try_acquire("alpha", Priority::Normal);
        assert!(third.is_err(), "circuit should be open after 2 failures");
    }

    #[test]
    fn invalid_argument_does_not_trip_breaker() {
        let resilience = ToolResilience::new(
            AdaptiveRateLimiter::new(8.0, 4.0),
            CircuitBreaker::new(circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                ..Default::default()
            }),
        );

        for _ in 0..3 {
            resilience
                .try_acquire("beta", Priority::Normal)
                .expect("call allowed");
            resilience.record_outcome("beta", CallOutcome::InvalidArgument);
        }

        // After 3 invalid-argument failures, the circuit must still be Closed.
        assert!(resilience.try_acquire("beta", Priority::Normal).is_ok());
    }
}
