use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub trajectory_enabled: bool,

    /// Enable real-time dashboards
    #[serde(default = "default_true")]
    pub dashboards_enabled: bool,

    /// KPI sampling interval in milliseconds
    #[serde(default = "default_interval")]
    pub sample_interval_ms: u64,

    /// Retention window for historical benchmarking (days)
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,

    /// Emit bottleneck traces for slow paths
    #[serde(default)]
    pub bottleneck_tracing: bool,

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
            dashboards_enabled: true,
            sample_interval_ms: default_interval(),
            retention_days: default_retention_days(),
            bottleneck_tracing: true,
            perf_events: true,
            trajectory_max_files: default_trajectory_max_files(),
            trajectory_max_age_days: default_trajectory_max_age_days(),
            trajectory_max_size_mb: default_trajectory_max_size_mb(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_interval() -> u64 {
    1000
}

fn default_retention_days() -> u32 {
    14
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
