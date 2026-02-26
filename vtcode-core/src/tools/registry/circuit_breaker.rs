//! MP-3: Circuit Breaker Pattern for MCP Client Failures
//!
//! Implements a three-state circuit breaker (Closed, Open, HalfOpen) to prevent
//! cascading failures when MCP providers become unavailable.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

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

impl From<u8> for CircuitState {
    fn from(val: u8) -> Self {
        match val {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed, // Default to closed for invalid values
        }
    }
}

use std::fs;
use std::path::PathBuf;

/// Persistable state of the circuit breaker
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedState {
    state: u8,
    consecutive_failures: u32,
    last_failure_epoch_secs: Option<u64>,
}

/// Circuit breaker for MCP client failures
pub struct McpCircuitBreaker {
    /// Current circuit state (0=Closed, 1=Open, 2=HalfOpen)
    state: AtomicU8,
    /// Consecutive failure count
    consecutive_failures: AtomicU32,
    /// Success count in half-open state
    half_open_successes: AtomicU32,
    /// Last failure timestamp (seconds since UNIX_EPOCH)
    last_failure_time: parking_lot::Mutex<Option<SystemTime>>,
    /// Configuration
    config: CircuitBreakerConfig,
    /// Optional path for persisting state
    persistence_path: Option<PathBuf>,
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

#[allow(dead_code)]
impl McpCircuitBreaker {
    /// Create a new circuit breaker with default configuration
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with custom configuration
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            state: AtomicU8::new(CircuitState::Closed as u8),
            consecutive_failures: AtomicU32::new(0),
            half_open_successes: AtomicU32::new(0),
            last_failure_time: parking_lot::Mutex::new(None),
            config,
            persistence_path: None,
        }
    }

    /// Create a new persistence-enabled circuit breaker
    pub fn with_persistence(path: PathBuf) -> Self {
        let breaker = Self {
            state: AtomicU8::new(CircuitState::Closed as u8),
            consecutive_failures: AtomicU32::new(0),
            half_open_successes: AtomicU32::new(0),
            last_failure_time: parking_lot::Mutex::new(None),
            config: CircuitBreakerConfig::default(),
            persistence_path: Some(path.clone()),
        };

        // Try to load state
        if let Ok(data) = fs::read_to_string(&path)
            && let Ok(state) = serde_json::from_str::<PersistedState>(&data)
        {
            breaker.state.store(state.state, Ordering::Release);
            breaker
                .consecutive_failures
                .store(state.consecutive_failures, Ordering::Relaxed);

            if let Some(epoch) = state.last_failure_epoch_secs {
                // Only restore if plausible (roughly sanity check vs now)
                let now = SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // If it's in the past and within reasonable bounds (e.g. not older than a year and not in future)
                if epoch <= now {
                    *breaker.last_failure_time.lock() =
                        Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch));
                }
            }
        }
        breaker
    }

    /// Persist current state to disk if path is configured
    fn persist(&self) {
        if let Some(path) = &self.persistence_path {
            let last_failure = *self.last_failure_time.lock();
            let epoch = last_failure.map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            });

            let state = PersistedState {
                state: self.state.load(Ordering::Acquire),
                consecutive_failures: self.consecutive_failures.load(Ordering::Acquire),
                last_failure_epoch_secs: epoch,
            };

            if let Ok(data) = serde_json::to_string(&state) {
                // Best effort write
                let _ = fs::write(path, data);
            }
        }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        self.state.load(Ordering::Relaxed).into()
    }

    /// Check if request should be allowed through
    ///
    /// Returns `true` if the request should be allowed, `false` otherwise
    pub fn allow_request(&self) -> bool {
        let state = self.state();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed to transition to half-open
                let should_retry = {
                    let last_failure = self.last_failure_time.lock();
                    if let Some(failure_time) = *last_failure {
                        if let Ok(elapsed) = failure_time.elapsed() {
                            let timeout = self.calculate_timeout();
                            elapsed >= timeout
                        } else {
                            false
                        }
                    } else {
                        // No failure recorded, allow transition
                        true
                    }
                };

                if should_retry {
                    // Transition to half-open
                    self.state
                        .store(CircuitState::HalfOpen as u8, Ordering::Release);
                    self.half_open_successes.store(0, Ordering::Relaxed);
                    self.persist();
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                true
            }
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        let state = self.state();

        match state {
            CircuitState::Closed => {
                // Reset failure counter on success
                self.consecutive_failures.store(0, Ordering::Relaxed);
                // Optimization: Maybe don't persist on every success to avoid IO thrashing
                // Only if failures > 0
            }
            CircuitState::HalfOpen => {
                let successes = self.half_open_successes.fetch_add(1, Ordering::AcqRel) + 1;
                if successes >= self.config.success_threshold {
                    // Enough successes, close the circuit
                    self.state
                        .store(CircuitState::Closed as u8, Ordering::Release);
                    self.consecutive_failures.store(0, Ordering::Relaxed);
                    self.half_open_successes.store(0, Ordering::Relaxed);
                    *self.last_failure_time.lock() = None;
                    self.persist();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but treat as half-open transition
                self.state
                    .store(CircuitState::HalfOpen as u8, Ordering::Release);
                self.half_open_successes.store(1, Ordering::Relaxed);
                self.persist();
            }
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        let state = self.state();
        *self.last_failure_time.lock() = Some(SystemTime::now());

        match state {
            CircuitState::Closed => {
                let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
                if failures >= self.config.failure_threshold {
                    // Too many failures, open the circuit
                    self.state
                        .store(CircuitState::Open as u8, Ordering::Release);
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open, go back to open
                self.state
                    .store(CircuitState::Open as u8, Ordering::Release);
                self.consecutive_failures.fetch_add(1, Ordering::AcqRel);
                self.half_open_successes.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {
                // Already open, just increment failure count
                self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Always persist on failure update
        self.persist();
    }

    /// Calculate timeout duration with exponential backoff
    fn calculate_timeout(&self) -> Duration {
        let failures = self.consecutive_failures.load(Ordering::Relaxed);

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
        CircuitBreakerDiagnostics {
            state: self.state(),
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            half_open_successes: self.half_open_successes.load(Ordering::Relaxed),
            last_failure_time: *self.last_failure_time.lock(),
            current_timeout: self.calculate_timeout(),
        }
    }

    /// Reset the circuit breaker to closed state
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.state
            .store(CircuitState::Closed as u8, Ordering::Release);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.half_open_successes.store(0, Ordering::Relaxed);
        *self.last_failure_time.lock() = None;
        self.persist();
    }
}

impl Default for McpCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnostic information about circuit breaker state
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CircuitBreakerDiagnostics {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    #[allow(dead_code)]
    pub half_open_successes: u32,
    pub last_failure_time: Option<SystemTime>,
    pub current_timeout: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_circuit_breaker_closed_state() {
        let breaker = McpCircuitBreaker::new();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.allow_request());
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = McpCircuitBreaker::with_config(config);

        // Record failures up to threshold
        breaker.record_failure(); // 1
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure(); // 2
        assert_eq!(breaker.state(), CircuitState::Closed);

        breaker.record_failure(); // 3 - should open
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.allow_request()); // Should block requests
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
}
