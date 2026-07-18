//! Timeout configuration helpers for ToolRegistry.

use crate::config::TimeoutsConfig;

use super::{ToolRegistry, ToolTimeoutPolicy};

impl ToolRegistry {
    pub fn apply_timeout_policy(&self, timeouts: &TimeoutsConfig) {
        let policy = ToolTimeoutPolicy::from_config(timeouts);

        // Validate the policy before applying
        match policy.validate() {
            Ok(()) => {
                *self.timeout_policy.write() = policy;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Invalid timeout configuration detected, using defaults"
                );
                *self.timeout_policy.write() = ToolTimeoutPolicy::default();
            }
        }

        self.resiliency.lock().adaptive_tuning = super::config_helpers::load_adaptive_tuning_from_config(timeouts);
    }

    pub fn timeout_policy(&self) -> ToolTimeoutPolicy {
        self.timeout_policy.read().clone()
    }

    pub fn rate_limit_per_minute(&self) -> Option<usize> {
        self.execution_history.rate_limit_per_minute()
    }
}
