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

    /// Manually enable reasoning support for models that are not natively recognized.
    /// Note: Setting this to false will NOT disable reasoning for known reasoning models (e.g. GPT-5).
    #[serde(default)]
    pub model_supports_reasoning: Option<bool>,

    /// Manually enable reasoning effort support for models that are not natively recognized.
    /// Note: Setting this to false will NOT disable reasoning effort for known models.
    #[serde(default)]
    pub model_supports_reasoning_effort: Option<bool>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            skip_loop_detection: default_loop_detection_enabled(),
            loop_detection_threshold: default_loop_detection_threshold(),
            loop_detection_interactive: default_loop_detection_interactive(),
            model_supports_reasoning: None,
            model_supports_reasoning_effort: None,
        }
    }
}

#[inline]
const fn default_loop_detection_enabled() -> bool {
    false
}

#[inline]
const fn default_loop_detection_threshold() -> usize {
    2
}

#[inline]
const fn default_loop_detection_interactive() -> bool {
    true
}
