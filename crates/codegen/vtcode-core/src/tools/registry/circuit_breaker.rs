//! MP-3: Circuit Breaker Pattern for MCP Client Failures
//!
//! Implements a three-state circuit breaker (Closed, Open, HalfOpen) to prevent
//! cascading failures when MCP providers become unavailable.
//!
//! # Race Condition Prevention
//!
//! This implementation uses a `parking_lot::Mutex` to protect the entire state,
//! ensuring that check-and-transition operations are atomic. This prevents TOCTOU
//! (Time-of-Check-Time-of-Use) race conditions where multiple threads could
//! simultaneously observe the same state and attempt conflicting transitions.
//!
//! See "Rust Prevents Data Races, Not Race Conditions" for why this matters:
//! https://corrode.dev/blog/rust-prevents-data-races-not-race-conditions/

use crate::metrics::MetricsCollector;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vtcode_commons::error_category::ErrorCategory;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// Normal operation, requests flow through
    Closed = 0,
    /// Too many failures, requests blocked
    Open = 1,
    /// Testing recovery, limited requests allowed
    HalfOpen = 2,
}

use std::fs;
use std::path::PathBuf;

/// Internal state protected by mutex
#[derive(Debug, Clone)]
struct InternalState {
    /// Current circuit state
    status: CircuitState,
    /// Consecutive failure count
    consecutive_failures: u32,
    /// Success count in half-open state
    half_open_successes: u32,
    /// Last failure timestamp
    last_failure_time: Option<Instant>,
    /// Count of requests denied while the breaker is open
    blocked_requests: u32,
}

impl Default for InternalState {
    fn default() -> Self {
        Self {
            status: CircuitState::Closed,
            consecutive_failures: 0,
            half_open_successes: 0,
            last_failure_time: None,
            blocked_requests: 0,
        }
    }
}

/// Circuit breaker for MCP client failures
///
/// Uses mutex-protected state to ensure atomic check-and-transition operations,
/// preventing TOCTOU race conditions.
pub struct McpCircuitBreaker {
    /// Protected state - all reads and writes go through this mutex
    state: Mutex<InternalState>,
    /// Configuration
    config: CircuitBreakerConfig,
    /// Optional path for persisting state
    persistence_path: Option<PathBuf>,
    metrics: Option<Arc<MetricsCollector>>,
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
    /// Base timeout before attempting half-open (seconds)
    pub base_timeout: Duration,
    /// Maximum timeout (exponential backoff cap)
    pub max_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3, // Open after 3 consecutive failures
            success_threshold: 2, // Close after 2 consecutive successes
            base_timeout: Duration::from_secs(10),
            max_timeout: Duration::from_secs(60),
        }
    }
}

impl McpCircuitBreaker {
    /// Create a new circuit breaker with default configuration
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with default configuration and metrics hooks.
    pub fn with_metrics(metrics: Arc<MetricsCollector>) -> Self {
        Self::with_config_and_metrics(CircuitBreakerConfig::default(), metrics)
    }

    /// Create a new circuit breaker with custom configuration
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self::build(config, None, None)
    }

    /// Create a new circuit breaker with custom configuration and metrics hooks.
    pub fn with_config_and_metrics(
        config: CircuitBreakerConfig,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        Self::build(config, None, Some(metrics))
    }

    fn build(
        config: CircuitBreakerConfig,
        persistence_path: Option<PathBuf>,
        metrics: Option<Arc<MetricsCollector>>,
    ) -> Self {
        Self {
            state: Mutex::new(InternalState::default()),
            config,
            persistence_path,
            metrics,
        }
    }

    /// Create a new persistence-enabled circuit breaker
    #[allow(dead_code)]
    pub fn with_persistence(path: PathBuf) -> Self {
        let breaker = Self::build(CircuitBreakerConfig::default(), Some(path.clone()), None);

        // Try to load state
        if let Ok(data) = fs::read_to_string(&path)
            && let Ok(persisted) = serde_json::from_str::<PersistedState>(&data)
        {
            let mut state = breaker.state.lock();
            state.status = match persisted.state {
                0 => CircuitState::Closed,
                1 => CircuitState::Open,
                2 => CircuitState::HalfOpen,
                _ => CircuitState::Closed,
            };
            state.consecutive_failures = persisted.consecutive_failures;

            if let Some(epoch) = persisted.last_failure_epoch_secs {
                let now = Instant::now();
                let elapsed = Duration::from_secs(epoch);
                // Approximate - we can't perfectly restore Instant, but this is close enough
                state.last_failure_time = Some(now.checked_sub(elapsed).unwrap_or(now));
            }
        }
        breaker
    }

    #[inline]
    fn record_half_open_metric(&self) {
        MetricsCollector::record_circuit_breaker_metrics(&self.metrics, true, false, false);
    }

    #[inline]
    fn record_breaker_denial_metric(&self) {
        MetricsCollector::record_circuit_breaker_metrics(&self.metrics, false, true, false);
    }

    #[inline]
    fn record_circuit_open_metric(&self) {
        MetricsCollector::record_circuit_breaker_metrics(&self.metrics, false, false, true);
    }

    /// Persist current state to disk if path is configured.
    ///
    /// This method is called outside the lock to minimize critical section duration.
    fn persist(&self, state: &InternalState) {
        if let Some(path) = &self.persistence_path {
            let epoch = state.last_failure_time.map(|t| {
                // We can't get epoch from Instant, so we approximate
                // This is good enough for persistence across restarts
                t.elapsed().as_secs()
            });

            let persisted = PersistedState {
                state: state.status as u8,
                consecutive_failures: state.consecutive_failures,
                last_failure_epoch_secs: epoch,
            };

            if let Ok(data) = serde_json::to_string(&persisted) {
                // Best-effort write. Offload the blocking disk I/O to the
                // blocking thread pool so it never stalls the async runtime
                // that drives tool execution / streaming. The caller has
                // already finalized `state` and invokes this outside the lock.
                //
                // `spawn_blocking` panics if there is no Tokio runtime on the
                // current thread (e.g. a future sync caller, or persistence
                // enabled from a non-async context), so fall back to a direct
                // write in that case rather than crashing the breaker.
                let path = path.clone();
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        let _persist_task = handle.spawn_blocking(move || {
                            let _ = fs::write(&path, data);
                        });
                    }
                    Err(_) => {
                        let _ = fs::write(&path, data);
                    }
                }
            }
        }
    }

    /// Get current circuit state
    #[cfg(test)]
    pub fn state(&self) -> CircuitState {
        self.state.lock().status
    }

    /// Check if request should be allowed through
    ///
    /// Returns `true` if the request should be allowed, `false` otherwise.
    ///
    /// This method holds the lock for the entire check-and-transition operation,
    /// preventing TOCTOU race conditions. Disk I/O is performed outside the lock.
    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock();
        let mut should_persist = false;

        let result = match state.status {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Check if timeout has elapsed to transition to half-open
                if let Some(last_failure) = state.last_failure_time {
                    let timeout = self.calculate_timeout(&state);
                    if last_failure.elapsed() >= timeout {
                        // Transition to half-open - atomic with the check
                        state.status = CircuitState::HalfOpen;
                        state.half_open_successes = 0;
                        self.record_half_open_metric();
                        should_persist = true;
                        true
                    } else {
                        state.blocked_requests += 1;
                        self.record_breaker_denial_metric();
                        should_persist = true;
                        false
                    }
                } else {
                    state.blocked_requests += 1;
                    self.record_breaker_denial_metric();
                    should_persist = true;
                    false
                }
            }
        };

        // Persist outside the lock
        if should_persist {
            let state_clone = state.clone();
            drop(state); // Explicitly release lock before disk I/O
            self.persist(&state_clone);
        }

        result
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        let mut state = self.state.lock();
        let mut should_persist = false;

        match state.status {
            CircuitState::Closed => {
                // Reset failure counter on success
                state.consecutive_failures = 0;
            }
            CircuitState::HalfOpen => {
                state.half_open_successes += 1;
                if state.half_open_successes >= self.config.success_threshold {
                    // Enough successes, close the circuit
                    state.status = CircuitState::Closed;
                    state.consecutive_failures = 0;
                    state.half_open_successes = 0;
                    state.last_failure_time = None;
                    should_persist = true;
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but treat as half-open transition
                state.status = CircuitState::HalfOpen;
                state.half_open_successes = 1;
                should_persist = true;
            }
        }

        // Persist outside the lock
        if should_persist {
            let state_clone = state.clone();
            drop(state); // Explicitly release lock before disk I/O
            self.persist(&state_clone);
        }
    }

    /// Record a failed operation
    #[cfg(test)]
    pub fn record_failure(&self) {
        self.record_failure_category(ErrorCategory::ExecutionError);
    }

    /// Record a failed operation with its canonical error category.
    pub fn record_failure_category(&self, category: ErrorCategory) {
        if !category.should_trip_circuit_breaker() {
            return;
        }

        let mut state = self.state.lock();
        state.last_failure_time = Some(Instant::now());

        match state.status {
            CircuitState::Closed => {
                state.consecutive_failures += 1;
                if state.consecutive_failures >= self.config.failure_threshold {
                    // Too many failures, open the circuit
                    state.status = CircuitState::Open;
                    self.record_circuit_open_metric();
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open, go back to open
                state.status = CircuitState::Open;
                state.consecutive_failures += 1;
                state.half_open_successes = 0;
                self.record_circuit_open_metric();
            }
            CircuitState::Open => {
                // Already open, just increment failure count
                state.consecutive_failures += 1;
            }
        }

        // Always persist on failure update, outside the lock
        let state_clone = state.clone();
        drop(state); // Explicitly release lock before disk I/O
        self.persist(&state_clone);
    }

    /// Calculate timeout duration with exponential backoff
    fn calculate_timeout(&self, state: &InternalState) -> Duration {
        let failures = state.consecutive_failures;

        // Exponential backoff: base_timeout * 2^(failures - threshold)
        let multiplier = if failures > self.config.failure_threshold {
            2u32.saturating_pow(failures.saturating_sub(self.config.failure_threshold))
        } else {
            1
        };

        let timeout = self.config.base_timeout.saturating_mul(multiplier);
        timeout.min(self.config.max_timeout)
    }

    /// Get diagnostic information
    pub fn diagnostics(&self) -> CircuitBreakerDiagnostics {
        let state = self.state.lock();
        let timeout = self.calculate_timeout(&state);

        let retry_after = if state.status == CircuitState::Open {
            state.last_failure_time.and_then(|failure_time| {
                let elapsed = failure_time.elapsed();
                let timeout = self.calculate_timeout(&state);
                timeout.checked_sub(elapsed)
            })
        } else {
            None
        };

        CircuitBreakerDiagnostics {
            status: state.status,
            consecutive_failures: state.consecutive_failures,
            half_open_successes: state.half_open_successes,
            last_failure_time: state.last_failure_time,
            current_timeout: timeout,
            retry_after,
            blocked_requests: state.blocked_requests,
            is_blocking: state.status == CircuitState::Open,
        }
    }
}

impl Default for McpCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnostic information about circuit breaker state
#[derive(Debug, Clone)]
pub struct CircuitBreakerDiagnostics {
    pub status: CircuitState,
    pub consecutive_failures: u32,
    #[allow(dead_code)]
    pub half_open_successes: u32,
    pub last_failure_time: Option<Instant>,
    pub current_timeout: Duration,
    pub retry_after: Option<Duration>,
    #[allow(dead_code)]
    pub blocked_requests: u32,
    #[allow(dead_code)]
    pub is_blocking: bool,
}

/// Persistable state of the circuit breaker
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedState {
    state: u8,
    consecutive_failures: u32,
    last_failure_epoch_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsCollector;
    use std::thread;

    #[test]
    fn test_circuit_breaker_closed_state() {
        let breaker = McpCircuitBreaker::new();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let config = CircuitBreakerConfig { failure_threshold: 3, ..Default::default() };
        let breaker = McpCircuitBreaker::with_config(config);

        // Record failures up to threshold
        breaker.record_failure(); // 1
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure(); // 2
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure(); // 3 - should open
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request()); // Should block requests
        assert!(breaker.diagnostics().blocked_requests > 0);
    }

    #[test]
    fn test_circuit_breaker_half_open_transition() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            base_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let breaker = McpCircuitBreaker::with_config(config);

        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for timeout
        thread::sleep(Duration::from_millis(150));

        // Should transition to half-open
        assert!(breaker.allow_request());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            base_timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let breaker = McpCircuitBreaker::with_config(config);

        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait and transition to half-open
        thread::sleep(Duration::from_millis(60));
        assert!(breaker.allow_request());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Record successes to close
        breaker.record_success(); // 1
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        breaker.record_success(); // 2 - should close
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_failure_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            base_timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let breaker = McpCircuitBreaker::with_config(config);

        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();

        // Transition to half-open
        thread::sleep(Duration::from_millis(60));
        breaker.allow_request();
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Failure in half-open should reopen
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_exponential_backoff() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            base_timeout: Duration::from_secs(10),
            max_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let breaker = McpCircuitBreaker::with_config(config);

        // Record multiple failures
        for _ in 0..5 {
            breaker.record_failure();
        }

        let diag = breaker.diagnostics();
        // After 5 failures (3 above threshold), timeout should be base * 2^3 = 80s, capped at 60s
        assert_eq!(diag.current_timeout, Duration::from_secs(60));
    }

    #[test]
    fn authentication_failure_does_not_trip_breaker() {
        let breaker = McpCircuitBreaker::new();
        breaker.record_failure_category(ErrorCategory::Authentication);

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert_eq!(breaker.diagnostics().consecutive_failures, 0);
    }

    #[test]
    fn reliability_metrics_capture_open_half_open_and_denials() {
        let metrics = Arc::new(MetricsCollector::new());
        let breaker = McpCircuitBreaker::with_config_and_metrics(
            CircuitBreakerConfig {
                failure_threshold: 1,
                base_timeout: Duration::from_millis(10),
                ..Default::default()
            },
            metrics.clone(),
        );

        breaker.record_failure_category(ErrorCategory::ExecutionError);
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request());

        thread::sleep(Duration::from_millis(20));
        assert!(breaker.allow_request());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        let execution = metrics.get_execution_metrics();
        assert_eq!(execution.circuit_open_events, 1);
        assert_eq!(execution.breaker_denials, 1);
        assert_eq!(execution.half_open_events, 1);
    }

    #[test]
    fn concurrent_requests_do_not_cause_inconsistent_state() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            base_timeout: Duration::from_millis(50),
            ..Default::default()
        };
        let breaker = Arc::new(McpCircuitBreaker::with_config(config));
        let request_count = Arc::new(AtomicUsize::new(0));

        // Open the circuit
        breaker.record_failure();
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for timeout with generous margin (2x the backoff) to avoid flaky tests
        thread::sleep(Duration::from_millis(100));

        // Spawn multiple threads that all try to make requests
        let mut handles = vec![];
        for _ in 0..10 {
            let breaker = breaker.clone();
            let request_count = request_count.clone();
            handles.push(thread::spawn(move || {
                if breaker.allow_request() {
                    request_count.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // The circuit should be in HalfOpen state (or Closed if all succeeded)
        // All threads should see consistent state - no panics or inconsistencies
        let final_state = breaker.state();
        assert!(
            final_state == CircuitState::HalfOpen || final_state == CircuitState::Closed,
            "Circuit should be in HalfOpen or Closed state, got {final_state:?}"
        );

        // All threads that called allow_request should have gotten a consistent answer
        let count = request_count.load(Ordering::SeqCst);
        assert!(count >= 1, "At least one thread should have succeeded");
    }
}
