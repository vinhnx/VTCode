use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // Normal operation, allow all calls
    Open,     // Failing, reject all calls immediately
    HalfOpen, // Testing, allow limited calls to check recovery
}

#[derive(Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub reset_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(60),
        }
    }
}

struct InternalState {
    status: CircuitState,
    failure_count: u32,
    last_failure_time: Option<Instant>,
}

#[derive(Clone)]
pub struct CircuitBreaker {
    state: Arc<Mutex<InternalState>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(InternalState {
                status: CircuitState::Closed,
                failure_count: 0,
                last_failure_time: None,
            })),
            config,
        }
    }

    /// Check if a request is allowed to proceed.
    /// Returns true if allowed, false if the circuit is open.
    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap();

        match state.status {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed to try HalfOpen
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() >= self.config.reset_timeout {
                        state.status = CircuitState::HalfOpen;
                        return true; // Use this request as the probe
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // In a real implementation, we might limit concurrent HalfOpen probes.
                // For simplicity, we allow it; success/failure will decide next state.
                true
            }
        }
    }

    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();
        match state.status {
            CircuitState::HalfOpen => {
                // Probe succeeded, close the circuit
                state.status = CircuitState::Closed;
                state.failure_count = 0;
                state.last_failure_time = None;
            }
            CircuitState::Closed => {
                // Reset failure count on success if we want purely consecutive failures
                state.failure_count = 0;
            }
            CircuitState::Open => {
                // Should not happen theoretically unless race condition or forced reset
                state.status = CircuitState::Closed;
                state.failure_count = 0;
            }
        }
    }

    pub fn record_failure(&self) {
        let mut state = self.state.lock().unwrap();
        state.last_failure_time = Some(Instant::now());

        match state.status {
            CircuitState::Closed => {
                state.failure_count += 1;
                if state.failure_count >= self.config.failure_threshold {
                    state.status = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                // Probe failed, revert to Open
                state.status = CircuitState::Open;
            }
            CircuitState::Open => {
                // Already open, just update time
            }
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state.lock().unwrap().status
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}
