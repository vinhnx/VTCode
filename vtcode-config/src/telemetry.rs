use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub trajectory_enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            trajectory_enabled: true,
        }
    }
}

fn default_true() -> bool {
    true
}
