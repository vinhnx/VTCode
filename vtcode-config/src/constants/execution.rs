/// Default timeout for agent loop execution (10 minutes)
/// Used when no timeout is specified or when 0 is passed
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

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
