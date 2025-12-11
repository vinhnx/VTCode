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
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            trajectory_enabled: true,
            dashboards_enabled: true,
            sample_interval_ms: default_interval(),
            retention_days: default_retention_days(),
            bottleneck_tracing: true,
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
