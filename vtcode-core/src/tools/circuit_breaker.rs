use hashbrown::HashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vtcode_commons::ErrorCategory;

use crate::metrics::MetricsCollector;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CircuitState {
    #[default]
    Closed, // Normal operation, allow all calls
    Open,     // Failing, reject all calls immediately
    HalfOpen, // Testing, allow limited calls to check recovery
}

impl CircuitState {
    /// Returns valid transitions from this state
    #[inline]
    const fn valid_transitions(&self) -> &'static [CircuitState] {
        match self {
            CircuitState::Closed => &[CircuitState::Open],
            CircuitState::Open => &[CircuitState::HalfOpen],
            CircuitState::HalfOpen => &[CircuitState::Closed, CircuitState::Open],
        }
    }

    /// Check if transition to target state is valid
    #[inline]
    fn can_transition_to(&self, target: CircuitState) -> bool {
        self.valid_transitions().contains(&target)
    }
}

#[derive(Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub reset_timeout: Duration, // Initial/Base timeout
    pub min_backoff: Duration,   // Minimum wait time
    pub max_backoff: Duration,   // Maximum wait time
    pub backoff_factor: f64,     // Multiplier (e.g., 2.0 for exponential)
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(60),
            min_backoff: Duration::from_secs(10), // Start with 10s
            max_backoff: Duration::from_secs(300), // Cap at 5m
            backoff_factor: 2.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ToolCircuitState {
    status: CircuitState,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    current_backoff: Duration, // Current backoff duration for this tool
    circuit_opened_at: Option<Instant>, // When circuit first opened (for diagnostics)
    open_count: u32,           // How many times circuit has opened
    denied_requests: u32,
    last_denied_at: Option<Instant>,
    last_error_category: Option<ErrorCategory>,
}

impl ToolCircuitState {
    /// Transition to a new state with debug assertion for valid transitions
    #[inline]
    fn transition_to(&mut self, new_state: CircuitState) {
        debug_assert!(
            self.status.can_transition_to(new_state),
            "Invalid circuit state transition: {:?} -> {:?}",
            self.status,
            new_state
        );
        self.status = new_state;
    }

    /// Reset state on successful recovery (from HalfOpen or Open)
    #[inline]
    fn reset_on_success(&mut self) {
        self.status = CircuitState::Closed;
        self.failure_count = 0;
        self.last_failure_time = None;
        self.current_backoff = Duration::ZERO;
        self.circuit_opened_at = None;
        self.last_error_category = None;
    }
}

#[derive(Debug, Clone)]
pub struct ToolCircuitDiagnostics {
    pub tool_name: String,
    pub status: CircuitState,
    pub failure_count: u32,
    pub current_backoff: Duration,
    pub remaining_backoff: Option<Duration>,
    pub opened_at: Option<Instant>,
    pub open_count: u32,
    pub is_open: bool,
    pub denied_requests: u32,
    pub last_denied_at: Option<Instant>,
    pub last_error_category: Option<ErrorCategory>,
}

#[derive(Debug, Clone, Default)]
pub struct CircuitBreakerSnapshot {
    pub diagnostics: Vec<ToolCircuitDiagnostics>,
    pub open_circuits: Vec<String>,
    pub open_count: usize,
}

/// Per-tool circuit breaker that tracks failure state independently for each tool.
/// This prevents one misbehaving tool from disabling all tools in the system.
///
/// Uses `parking_lot::RwLock` for better concurrent access:
/// - Read operations (allow_request, state checks) can proceed in parallel
/// - Write operations (record_success/failure) acquire exclusive access
/// - `parking_lot` is more efficient for short critical sections than std::Mutex
#[derive(Clone)]
pub struct CircuitBreaker {
    /// Per-tool state tracking with RwLock for better read concurrency
    tool_states: Arc<RwLock<HashMap<String, ToolCircuitState>>>,
    config: CircuitBreakerConfig,
    metrics: Option<Arc<MetricsCollector>>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self::build(config, None)
    }

    pub fn with_metrics(config: CircuitBreakerConfig, metrics: Arc<MetricsCollector>) -> Self {
        Self::build(config, Some(metrics))
    }

    fn build(config: CircuitBreakerConfig, metrics: Option<Arc<MetricsCollector>>) -> Self {
        Self {
            tool_states: Arc::new(RwLock::new(HashMap::new())),
            config,
            metrics,
        }
    }

    #[inline]
    fn record_half_open_metric(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.record_half_open();
        }
    }

    #[inline]
    fn record_breaker_denial_metric(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.record_breaker_denial();
        }
    }

    #[inline]
    fn record_circuit_open_metric(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.record_circuit_open();
        }
    }

    /// Check if a request for a specific tool is allowed to proceed.
    /// Returns true if allowed, false if the circuit is open for this tool.
    ///
    /// Uses optimistic read-first approach:
    /// 1. Try read lock first (allows concurrent reads)
    /// 2. Only upgrade to write lock if state transition is needed
    pub fn allow_request_for_tool(&self, tool_name: &str) -> bool {
        {
            let states = self.tool_states.read();
            if let Some(state) = states.get(tool_name) {
                match state.status {
                    CircuitState::Closed | CircuitState::HalfOpen => return true,
                    CircuitState::Open => {
                        if let Some(last_failure) = state.last_failure_time {
                            let backoff = if state.current_backoff == Duration::ZERO {
                                self.config.reset_timeout
                            } else {
                                state.current_backoff
                            };
                            if last_failure.elapsed() >= backoff {
                                // Fall through to the write lock so we can transition to HalfOpen.
                            }
                        }
                    }
                }
            } else {
                return true;
            }
        }

        let mut states = self.tool_states.write();
        let state = states.entry(tool_name.to_string()).or_default();

        match state.status {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last_failure) = state.last_failure_time {
                    let backoff = if state.current_backoff == Duration::ZERO {
                        self.config.reset_timeout
                    } else {
                        state.current_backoff
                    };

                    if last_failure.elapsed() >= backoff {
                        state.transition_to(CircuitState::HalfOpen);
                        self.record_half_open_metric();
                        return true;
                    }
                }
                state.denied_requests = state.denied_requests.saturating_add(1);
                state.last_denied_at = Some(Instant::now());
                self.record_breaker_denial_metric();
                false
            }
        }
    }

    /// Get remaining backoff time for a tool (if Open)
    pub fn remaining_backoff(&self, tool_name: &str) -> Option<Duration> {
        let states = self.tool_states.read();
        let state = states.get(tool_name)?;

        if state.status == CircuitState::Open
            && let Some(last) = state.last_failure_time
        {
            let backoff = state.current_backoff;
            let elapsed = last.elapsed();
            return backoff.checked_sub(elapsed);
        }
        None
    }

    /// Legacy method for backwards compatibility - always returns true (global check disabled).
    /// Use `allow_request_for_tool` for per-tool checking.
    #[deprecated(note = "Use allow_request_for_tool for per-tool circuit breaking")]
    pub fn allow_request(&self) -> bool {
        // Global circuit breaker is disabled - we now use per-tool tracking
        true
    }

    /// Record success for a specific tool
    ///
    /// State transitions on success:
    /// - HalfOpen -> Closed (probe succeeded)
    /// - Closed -> Closed (reset failure count)
    /// - Open -> Closed (forced recovery, e.g., manual reset)
    pub fn record_success_for_tool(&self, tool_name: &str) {
        let mut states = self.tool_states.write();
        let state = states.entry(tool_name.to_string()).or_default();

        match state.status {
            CircuitState::HalfOpen => {
                // Probe succeeded - use batched reset
                state.reset_on_success();
            }
            CircuitState::Closed => {
                // Reset failure count on success if we want purely consecutive failures
                state.failure_count = 0;
            }
            CircuitState::Open => {
                // Should not happen theoretically unless race condition or forced reset
                // Using direct assignment here since this is an exceptional recovery path
                state.reset_on_success();
            }
        }
    }

    /// Legacy method - no-op. Use `record_success_for_tool` instead.
    #[deprecated(note = "Use record_success_for_tool for per-tool circuit breaking")]
    pub fn record_success(&self) {
        // No-op for backwards compatibility
    }

    /// Record failure for a specific tool.
    /// Non-retryable validation, policy, and permission failures are ignored.
    ///
    /// State transitions on failure:
    /// - Closed -> Open (when threshold reached)
    /// - HalfOpen -> Open (probe failed, increase backoff)
    /// - Open -> Open (no change, just update timestamp)
    pub fn record_failure_category_for_tool(&self, tool_name: &str, category: ErrorCategory) {
        if !category.should_trip_circuit_breaker() {
            tracing::debug!(
                tool = %tool_name,
                category = %category,
                "Skipping circuit breaker failure accounting for non-circuit-breaking error"
            );
            return;
        }

        let mut states = self.tool_states.write();
        let state = states.entry(tool_name.to_string()).or_default();
        state.last_failure_time = Some(Instant::now());
        state.last_error_category = Some(category);

        match state.status {
            CircuitState::Closed => {
                state.failure_count += 1;
                if state.failure_count >= self.config.failure_threshold {
                    state.transition_to(CircuitState::Open);
                    state.current_backoff = self.config.min_backoff;
                    state.circuit_opened_at = Some(Instant::now());
                    state.open_count += 1;
                    self.record_circuit_open_metric();

                    tracing::warn!(
                        tool = %tool_name,
                        failures = state.failure_count,
                        backoff_sec = state.current_backoff.as_secs(),
                        open_count = state.open_count,
                        "Circuit breaker OPEN for tool"
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Probe failed, revert to Open and increase backoff
                state.transition_to(CircuitState::Open);
                state.circuit_opened_at = Some(Instant::now());
                state.open_count += 1;
                // Exponential backoff
                let next_backoff = state.current_backoff.as_secs_f64() * self.config.backoff_factor;
                state.current_backoff = Duration::from_secs_f64(next_backoff)
                    .min(self.config.max_backoff)
                    .max(self.config.min_backoff);
                self.record_circuit_open_metric();

                tracing::warn!(
                    tool = %tool_name,
                    backoff_sec = state.current_backoff.as_secs(),
                    open_count = state.open_count,
                    "Circuit breaker re-OPENED (probe failed)"
                );
            }
            CircuitState::Open => {
                // Already open, just update time - backoff stays same until probe attempt
            }
        }
    }

    /// Legacy helper that preserves the older boolean contract.
    pub fn record_failure_for_tool(&self, tool_name: &str, is_argument_error: bool) {
        let category = if is_argument_error {
            ErrorCategory::InvalidParameters
        } else {
            ErrorCategory::ExecutionError
        };
        self.record_failure_category_for_tool(tool_name, category);
    }

    /// Legacy method - no-op. Use `record_failure_for_tool` instead.
    #[deprecated(note = "Use record_failure_for_tool for per-tool circuit breaking")]
    pub fn record_failure(&self) {
        // No-op for backwards compatibility
    }

    /// Get the circuit state for a specific tool
    pub fn state_for_tool(&self, tool_name: &str) -> CircuitState {
        let states = self.tool_states.read();
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
        let mut states = self.tool_states.write();
        states.remove(tool_name);
    }

    /// Reset all tool circuit breaker states
    pub fn reset_all(&self) {
        let mut states = self.tool_states.write();
        states.clear();
    }

    /// Get list of tools with currently OPEN circuits
    pub fn get_open_circuits(&self) -> Vec<String> {
        self.snapshot().open_circuits
    }

    /// Get diagnostic information for a specific tool
    pub fn get_diagnostics(&self, tool_name: &str) -> ToolCircuitDiagnostics {
        self.snapshot()
            .diagnostics
            .into_iter()
            .find(|diag| diag.tool_name == tool_name)
            .unwrap_or_else(|| ToolCircuitDiagnostics {
                tool_name: tool_name.to_string(),
                status: CircuitState::Closed,
                failure_count: 0,
                current_backoff: Duration::ZERO,
                remaining_backoff: None,
                opened_at: None,
                open_count: 0,
                is_open: false,
                denied_requests: 0,
                last_denied_at: None,
                last_error_category: None,
            })
    }

    /// Get diagnostics for all tools
    pub fn get_all_diagnostics(&self) -> Vec<ToolCircuitDiagnostics> {
        self.snapshot().diagnostics
    }

    /// Get a full snapshot of circuit breaker state under a single read lock.
    pub fn snapshot(&self) -> CircuitBreakerSnapshot {
        let states = self.tool_states.read();
        let diagnostics: Vec<ToolCircuitDiagnostics> = states
            .iter()
            .map(|(name, state)| {
                let is_open = matches!(state.status, CircuitState::Open);
                ToolCircuitDiagnostics {
                    tool_name: name.clone(),
                    status: state.status,
                    failure_count: state.failure_count,
                    current_backoff: state.current_backoff,
                    remaining_backoff: if is_open {
                        state
                            .last_failure_time
                            .and_then(|last| state.current_backoff.checked_sub(last.elapsed()))
                    } else {
                        None
                    },
                    opened_at: state.circuit_opened_at,
                    open_count: state.open_count,
                    is_open,
                    denied_requests: state.denied_requests,
                    last_denied_at: state.last_denied_at,
                    last_error_category: state.last_error_category,
                }
            })
            .collect();

        let open_circuits: Vec<String> = diagnostics
            .iter()
            .filter(|diag| diag.is_open)
            .map(|diag| diag.tool_name.clone())
            .collect();

        CircuitBreakerSnapshot {
            diagnostics,
            open_count: open_circuits.len(),
            open_circuits,
        }
    }

    /// Check if recovery pause should be triggered based on open circuit count
    pub fn should_pause_for_recovery(&self, max_open_circuits: usize) -> bool {
        self.snapshot().open_count >= max_open_circuits
    }

    /// Get count of currently open circuits
    pub fn open_circuit_count(&self) -> usize {
        self.snapshot().open_count
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsCollector;

    #[test]
    fn invalid_parameters_do_not_open_circuit() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        });

        breaker.record_failure_category_for_tool("read_file", ErrorCategory::InvalidParameters);
        breaker.record_failure_category_for_tool("read_file", ErrorCategory::InvalidParameters);

        assert_eq!(breaker.state_for_tool("read_file"), CircuitState::Closed);
        assert_eq!(breaker.get_diagnostics("read_file").failure_count, 0);
    }

    #[test]
    fn denied_requests_are_recorded_for_open_circuit() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            min_backoff: Duration::from_secs(30),
            ..Default::default()
        });

        breaker.record_failure_category_for_tool("shell", ErrorCategory::ExecutionError);
        assert_eq!(breaker.state_for_tool("shell"), CircuitState::Open);
        assert!(!breaker.allow_request_for_tool("shell"));

        let diagnostics = breaker.get_diagnostics("shell");
        assert_eq!(diagnostics.denied_requests, 1);
        assert!(diagnostics.last_denied_at.is_some());
        assert_eq!(
            diagnostics.last_error_category,
            Some(ErrorCategory::ExecutionError)
        );
    }

    #[test]
    fn metrics_record_open_half_open_and_denials() {
        let metrics = Arc::new(MetricsCollector::new());
        let breaker = CircuitBreaker::with_metrics(
            CircuitBreakerConfig {
                failure_threshold: 1,
                min_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_secs(1),
                ..Default::default()
            },
            metrics.clone(),
        );

        breaker.record_failure_category_for_tool("shell", ErrorCategory::ExecutionError);
        assert!(!breaker.allow_request_for_tool("shell"));

        std::thread::sleep(Duration::from_millis(20));
        assert!(breaker.allow_request_for_tool("shell"));

        let execution = metrics.get_execution_metrics();
        assert_eq!(execution.circuit_open_events, 1);
        assert_eq!(execution.breaker_denials, 1);
        assert_eq!(execution.half_open_events, 1);
    }
}
