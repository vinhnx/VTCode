use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        self.circuit_opened_at = None;
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
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            tool_states: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Check if a request for a specific tool is allowed to proceed.
    /// Returns true if allowed, false if the circuit is open for this tool.
    ///
    /// Uses optimistic read-first approach:
    /// 1. Try read lock first (allows concurrent reads)
    /// 2. Only upgrade to write lock if state transition is needed
    pub fn allow_request_for_tool(&self, tool_name: &str) -> bool {
        // Fast path: try read lock first for Closed/HalfOpen states
        {
            let states = self.tool_states.read();
            if let Some(state) = states.get(tool_name) {
                match state.status {
                    CircuitState::Closed | CircuitState::HalfOpen => return true,
                    CircuitState::Open => {
                        // Check if we might need to transition - if not, return early
                        if let Some(last_failure) = state.last_failure_time {
                            let backoff = if state.current_backoff.as_secs() == 0 {
                                self.config.reset_timeout
                            } else {
                                state.current_backoff
                            };
                            if last_failure.elapsed() < backoff {
                                return false; // Still in backoff, no transition needed
                            }
                            // Fall through to write lock for transition
                        } else {
                            return false;
                        }
                    }
                }
            } else {
                // Tool not in map = Closed state (default)
                return true;
            }
        }

        // Slow path: need write lock for state transition (Open -> HalfOpen)
        let mut states = self.tool_states.write();
        let state = states.entry(tool_name.to_string()).or_default();

        // Re-check after acquiring write lock (another thread may have transitioned)
        match state.status {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last_failure) = state.last_failure_time {
                    let backoff = if state.current_backoff.as_secs() == 0 {
                        self.config.reset_timeout
                    } else {
                        state.current_backoff
                    };

                    if last_failure.elapsed() >= backoff {
                        state.transition_to(CircuitState::HalfOpen);
                        return true;
                    }
                }
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
    /// If `is_argument_error` is true, this is an LLM mistake (bad args), not a tool failure,
    /// and should not count toward the circuit breaker threshold.
    ///
    /// State transitions on failure:
    /// - Closed -> Open (when threshold reached)
    /// - HalfOpen -> Open (probe failed, increase backoff)
    /// - Open -> Open (no change, just update timestamp)
    pub fn record_failure_for_tool(&self, tool_name: &str, is_argument_error: bool) {
        // Don't count LLM argument errors toward circuit breaker - these are model mistakes
        if is_argument_error {
            tracing::debug!(
                tool = %tool_name,
                "Argument error - not counting toward circuit breaker"
            );
            return;
        }

        let mut states = self.tool_states.write();
        let state = states.entry(tool_name.to_string()).or_default();
        state.last_failure_time = Some(Instant::now());

        match state.status {
            CircuitState::Closed => {
                state.failure_count += 1;
                if state.failure_count >= self.config.failure_threshold {
                    state.transition_to(CircuitState::Open);
                    state.current_backoff = self.config.min_backoff;
                    state.circuit_opened_at = Some(Instant::now());
                    state.open_count += 1;

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
        let states = self.tool_states.read();
        states
            .iter()
            .filter(|(_, state)| matches!(state.status, CircuitState::Open))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Get diagnostic information for a specific tool
    pub fn get_diagnostics(&self, tool_name: &str) -> ToolCircuitDiagnostics {
        let states = self.tool_states.read();
        let state = states.get(tool_name);

        if let Some(s) = state {
            ToolCircuitDiagnostics {
                tool_name: tool_name.to_string(),
                status: s.status,
                failure_count: s.failure_count,
                current_backoff: s.current_backoff,
                remaining_backoff: if matches!(s.status, CircuitState::Open) {
                    s.last_failure_time
                        .and_then(|last| s.current_backoff.checked_sub(last.elapsed()))
                } else {
                    None
                },
                opened_at: s.circuit_opened_at,
                open_count: s.open_count,
                is_open: matches!(s.status, CircuitState::Open),
            }
        } else {
            ToolCircuitDiagnostics {
                tool_name: tool_name.to_string(),
                status: CircuitState::Closed,
                failure_count: 0,
                current_backoff: Duration::ZERO,
                remaining_backoff: None,
                opened_at: None,
                open_count: 0,
                is_open: false,
            }
        }
    }

    /// Get diagnostics for all tools
    pub fn get_all_diagnostics(&self) -> Vec<ToolCircuitDiagnostics> {
        let states = self.tool_states.read();
        states
            .iter()
            .map(|(name, state)| ToolCircuitDiagnostics {
                tool_name: name.clone(),
                status: state.status,
                failure_count: state.failure_count,
                current_backoff: state.current_backoff,
                remaining_backoff: if matches!(state.status, CircuitState::Open) {
                    state
                        .last_failure_time
                        .and_then(|last| state.current_backoff.checked_sub(last.elapsed()))
                } else {
                    None
                },
                opened_at: state.circuit_opened_at,
                open_count: state.open_count,
                is_open: matches!(state.status, CircuitState::Open),
            })
            .collect()
    }

    /// Check if recovery pause should be triggered based on open circuit count
    pub fn should_pause_for_recovery(&self, max_open_circuits: usize) -> bool {
        let states = self.tool_states.read();
        let open_count = states
            .iter()
            .filter(|(_, state)| matches!(state.status, CircuitState::Open))
            .count();
        open_count >= max_open_circuits
    }

    /// Get count of currently open circuits
    pub fn open_circuit_count(&self) -> usize {
        let states = self.tool_states.read();
        states
            .iter()
            .filter(|(_, state)| matches!(state.status, CircuitState::Open))
            .count()
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}
