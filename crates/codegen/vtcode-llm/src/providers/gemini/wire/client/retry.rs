use std::time::Duration;

/// Retry configuration for streaming operations
#[derive(Debug, Clone)]
pub struct RetryConfig {
    max_attempts: u32,
    initial_delay: Duration,
    max_delay: Duration,
    backoff_multiplier: f64,
    retryable_errors: Vec<String>,
}

const RETRYABLE_ERRORS: &[&str] = &["timeout", "connection", "rate_limit", "server_error", "network"];

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            retryable_errors: RETRYABLE_ERRORS.iter().map(|s| (*s).into()).collect(),
        }
    }
}
