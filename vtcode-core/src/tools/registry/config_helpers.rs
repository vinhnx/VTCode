//! Configuration helper utilities for ToolRegistry.

use crate::config::TimeoutsConfig;

use super::AdaptiveTimeoutTuning;

pub(super) fn load_adaptive_tuning_from_config(timeouts: &TimeoutsConfig) -> AdaptiveTimeoutTuning {
    AdaptiveTimeoutTuning::from_config(timeouts)
}
