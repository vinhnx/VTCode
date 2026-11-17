use crate::env_helpers::default_true;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub trajectory_enabled: bool,

    /// Enable real-time dashboards
    #[serde(default = "default_true")]
    pub dashboards_enabled: bool,

    /// Enable ATIF (Agent Trajectory Interchange Format) trajectory export.
    /// When enabled, sessions write an `atif-trajectory.json` alongside the
    /// existing `.jsonl` trajectory log.
    #[serde(default)]
    pub atif_enabled: bool,

    /// Emit performance events for file I/O, spawns, and UI latency
    #[serde(default = "default_true")]
    pub perf_events: bool,

    /// Maximum number of rotated trajectory log files to keep per workspace
    #[serde(default = "default_trajectory_max_files")]
    pub trajectory_max_files: usize,

    /// Maximum age in days for rotated trajectory log files
    #[serde(default = "default_trajectory_max_age_days")]
    pub trajectory_max_age_days: u64,

    /// Maximum total size in MB for all trajectory log files in a workspace
    #[serde(default = "default_trajectory_max_size_mb")]
    pub trajectory_max_size_mb: u64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            trajectory_enabled: true,
            atif_enabled: false,
            dashboards_enabled: true,
            perf_events: true,
            trajectory_max_files: default_trajectory_max_files(),
            trajectory_max_age_days: default_trajectory_max_age_days(),
            trajectory_max_size_mb: default_trajectory_max_size_mb(),
        }
    }
}

fn default_trajectory_max_files() -> usize {
    crate::constants::defaults::DEFAULT_TRAJECTORY_MAX_FILES
}

fn default_trajectory_max_age_days() -> u64 {
    crate::constants::defaults::DEFAULT_TRAJECTORY_MAX_AGE_DAYS
}

fn default_trajectory_max_size_mb() -> u64 {
    crate::constants::defaults::DEFAULT_TRAJECTORY_MAX_SIZE_MB
}
