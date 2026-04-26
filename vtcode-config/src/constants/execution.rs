/// Default timeout for agent loop execution (10 minutes)
/// Used when no timeout is specified or when 0 is passed
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// Base throttle delay (ms) for the agent runner loop between repeated identical tool calls.
pub const LOOP_THROTTLE_BASE_MS: u64 = 75;
/// Base throttle delay (ms) for repeated calls inside the tool registry execution facade.
pub const LOOP_THROTTLE_REGISTRY_BASE_MS: u64 = 25;
/// Maximum throttle delay (ms) ceiling applied to both the runner loop and the registry facade.
pub const LOOP_THROTTLE_MAX_MS: u64 = 500;

/// Default tool execution ceiling in seconds (matches TimeoutsConfig::default_ceiling_seconds).
pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 180;
/// Maximum wait in seconds when a tool is rate-limited before surfacing an error.
pub const MAX_RATE_LIMIT_WAIT_SECS: u64 = 5;
/// Default OAuth / auth-flow timeout in seconds.
pub const DEFAULT_AUTH_FLOW_TIMEOUT_SECS: u64 = 300;

/// Maximum number of consecutive idle turns before the agent runner aborts.
pub const IDLE_TURN_LIMIT: usize = 3;

/// Maximum recent error records kept per agent session for recovery diagnostics.
pub const DEFAULT_MAX_RECENT_ERRORS: usize = 10;

/// Maximum number of simultaneously open tool circuit breakers before the agent pauses.
pub const DEFAULT_MAX_OPEN_CIRCUITS: usize = 3;

/// Maximum allowed timeout (1 hour)
/// Any user-specified timeout above this is capped
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// Minimum timeout (10 seconds)
/// Prevents unreasonably short timeouts that would cause failures
pub const MIN_TIMEOUT_SECS: u64 = 10;

/// Resolve timeout with deterministic bounds (never returns 0 or unbounded)
/// This pattern ensures execution always has a bounded duration.
///
/// # Arguments
/// * `user_timeout` - Optional user-specified timeout in seconds
///
/// # Returns
/// A bounded timeout value that is:
/// - DEFAULT_TIMEOUT_SECS if None or 0
/// - MAX_TIMEOUT_SECS if exceeds maximum
/// - The user value if within bounds
#[inline]
pub const fn resolve_timeout(user_timeout: Option<u64>) -> u64 {
    match user_timeout {
        None => DEFAULT_TIMEOUT_SECS,
        Some(0) => DEFAULT_TIMEOUT_SECS,
        Some(t) if t > MAX_TIMEOUT_SECS => MAX_TIMEOUT_SECS,
        Some(t) if t < MIN_TIMEOUT_SECS => MIN_TIMEOUT_SECS,
        Some(t) => t,
    }
}
