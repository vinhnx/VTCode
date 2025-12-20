use serde::{Deserialize, Serialize};

/// Anthropic-specific provider configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicConfig {
    /// Beta header for interleaved thinking feature
    #[serde(default = "default_interleaved_thinking_beta")]
    pub interleaved_thinking_beta: String,

    /// Budget tokens for interleaved thinking
    #[serde(default = "default_interleaved_thinking_budget_tokens")]
    pub interleaved_thinking_budget_tokens: u32,

    /// Type value for enabling interleaved thinking
    #[serde(default = "default_interleaved_thinking_type")]
    pub interleaved_thinking_type_enabled: String,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            interleaved_thinking_beta: default_interleaved_thinking_beta(),
            interleaved_thinking_budget_tokens: default_interleaved_thinking_budget_tokens(),
            interleaved_thinking_type_enabled: default_interleaved_thinking_type(),
        }
    }
}

#[inline]
fn default_interleaved_thinking_beta() -> String {
    "interleaved-thinking-2025-05-14".to_string()
}

#[inline]
fn default_interleaved_thinking_budget_tokens() -> u32 {
    12000
}

#[inline]
fn default_interleaved_thinking_type() -> String {
    "enabled".to_string()
}