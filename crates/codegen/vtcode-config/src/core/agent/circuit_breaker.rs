//! Tool circuit-breaker configuration.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CircuitBreakerConfig {
    /// Enable circuit breaker functionality
    #[serde(default = "default_circuit_breaker_enabled")]
    pub enabled: bool,

    /// Number of consecutive failures before opening circuit
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,

    /// Pause and ask user when circuit opens (vs auto-backoff)
    #[serde(default = "default_pause_on_open")]
    pause_on_open: bool,

    /// Number of open circuits before triggering pause
    #[serde(default = "default_max_open_circuits")]
    max_open_circuits: usize,

    /// Cooldown period between recovery prompts (seconds)
    #[serde(default = "default_recovery_cooldown")]
    pub recovery_cooldown: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: default_circuit_breaker_enabled(),
            failure_threshold: default_failure_threshold(),
            pause_on_open: default_pause_on_open(),
            max_open_circuits: default_max_open_circuits(),
            recovery_cooldown: default_recovery_cooldown(),
        }
    }
}
#[inline]
const fn default_circuit_breaker_enabled() -> bool {
    true // Default: enabled for resilient execution
}

#[inline]
const fn default_failure_threshold() -> u32 {
    7 // Open circuit after 7 consecutive failures
}

#[inline]
const fn default_pause_on_open() -> bool {
    true // Default: ask user for guidance on circuit breaker
}

#[inline]
const fn default_max_open_circuits() -> usize {
    3 // Pause when 3+ tools have open circuits
}

#[inline]
const fn default_recovery_cooldown() -> u64 {
    60 // Cooldown between recovery prompts (seconds)
}
