use serde::{Deserialize, Serialize};

/// Model-specific behavior configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    /// Enable loop hang detection to identify when model is stuck in repetitive behavior
    #[serde(default = "default_loop_detection_enabled")]
    pub skip_loop_detection: bool,

    /// Maximum number of identical tool calls (same tool + same arguments) before triggering loop detection
    #[serde(default = "default_loop_detection_threshold")]
    pub loop_detection_threshold: usize,

    /// Enable interactive prompt for loop detection instead of silently halting
    #[serde(default = "default_loop_detection_interactive")]
    pub loop_detection_interactive: bool,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            skip_loop_detection: default_loop_detection_enabled(),
            loop_detection_threshold: default_loop_detection_threshold(),
            loop_detection_interactive: default_loop_detection_interactive(),
        }
    }
}

fn default_loop_detection_enabled() -> bool {
    false
}

fn default_loop_detection_threshold() -> usize {
    3
}

fn default_loop_detection_interactive() -> bool {
    true
}
