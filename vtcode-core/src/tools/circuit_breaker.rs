use std::collections::HashMap;
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

#[derive(Debug, Clone, Default)]
struct ToolCircuitState {
    status: CircuitState,
    failure_count: u32,
    last_failure_time: Option<Instant>,
}

impl Default for CircuitState {
    fn default() -> Self {
        CircuitState::Closed
    }
}

/// Per-tool circuit breaker that tracks failure state independently for each tool.
/// This prevents one misbehaving tool from disabling all tools in the system.
#[derive(Clone)]
pub struct CircuitBreaker {
    /// Per-tool state tracking
    tool_states: Arc<Mutex<HashMap<String, ToolCircuitState>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            tool_states: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Check if a request for a specific tool is allowed to proceed.
    /// Returns true if allowed, false if the circuit is open for this tool.
    pub fn allow_request_for_tool(&self, tool_name: &str) -> bool {
        let mut states = self.tool_states.lock().unwrap();
        let state = states.entry(tool_name.to_string()).or_default();

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

    /// Legacy method for backwards compatibility - always returns true (global check disabled).
    /// Use `allow_request_for_tool` for per-tool checking.
    #[deprecated(note = "Use allow_request_for_tool for per-tool circuit breaking")]
    pub fn allow_request(&self) -> bool {
        // Global circuit breaker is disabled - we now use per-tool tracking
        true
    }

    /// Record success for a specific tool
    pub fn record_success_for_tool(&self, tool_name: &str) {
        let mut states = self.tool_states.lock().unwrap();
        let state = states.entry(tool_name.to_string()).or_default();

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

    /// Legacy method - no-op. Use `record_success_for_tool` instead.
    #[deprecated(note = "Use record_success_for_tool for per-tool circuit breaking")]
    pub fn record_success(&self) {
        // No-op for backwards compatibility
    }

    /// Record failure for a specific tool.
    /// If `is_argument_error` is true, this is an LLM mistake (bad args), not a tool failure,
    /// and should not count toward the circuit breaker threshold.
    pub fn record_failure_for_tool(&self, tool_name: &str, is_argument_error: bool) {
        // Don't count LLM argument errors toward circuit breaker - these are model mistakes
        if is_argument_error {
            tracing::debug!(
                tool = %tool_name,
                "Argument error - not counting toward circuit breaker"
            );
            return;
        }

        let mut states = self.tool_states.lock().unwrap();
        let state = states.entry(tool_name.to_string()).or_default();
        state.last_failure_time = Some(Instant::now());

        match state.status {
            CircuitState::Closed => {
                state.failure_count += 1;
                if state.failure_count >= self.config.failure_threshold {
                    state.status = CircuitState::Open;
                    tracing::warn!(
                        tool = %tool_name,
                        failures = state.failure_count,
                        "Circuit breaker OPEN for tool"
                    );
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

    /// Legacy method - no-op. Use `record_failure_for_tool` instead.
    #[deprecated(note = "Use record_failure_for_tool for per-tool circuit breaking")]
    pub fn record_failure(&self) {
        // No-op for backwards compatibility
    }

    /// Get the circuit state for a specific tool
    pub fn state_for_tool(&self, tool_name: &str) -> CircuitState {
        let states = self.tool_states.lock().unwrap();
        states
            .get(tool_name)
            .map(|s| s.status)
            .unwrap_or(CircuitState::Closed)
    }

    /// Legacy method - returns Closed. Use `state_for_tool` instead.
    #[deprecated(note = "Use state_for_tool for per-tool circuit breaking")]
    pub fn state(&self) -> CircuitState {
        CircuitState::Closed
    }

    /// Reset the circuit breaker state for a specific tool
    pub fn reset_tool(&self, tool_name: &str) {
        let mut states = self.tool_states.lock().unwrap();
        states.remove(tool_name);
    }

    /// Reset all tool circuit breaker states
    pub fn reset_all(&self) {
        let mut states = self.tool_states.lock().unwrap();
        states.clear();
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}
