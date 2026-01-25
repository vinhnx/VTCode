//! Configuration helper utilities for ToolRegistry.

use std::env;

use crate::config::TimeoutsConfig;

use super::AdaptiveTimeoutTuning;

pub(super) fn tool_rate_limit_from_env() -> Option<usize> {
    env::var("VTCODE_TOOL_CALLS_PER_MIN")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

pub(super) fn load_adaptive_tuning_from_config(
    timeouts: &TimeoutsConfig,
) -> AdaptiveTimeoutTuning {
    AdaptiveTimeoutTuning::from_config(timeouts)
}
