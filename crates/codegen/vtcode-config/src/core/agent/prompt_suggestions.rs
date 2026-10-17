//! Inline prompt-suggestion configuration.

use serde::{Deserialize, Serialize};

/// Inline prompt suggestion configuration for the chat composer.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentPromptSuggestionsConfig {
    /// Enable inline prompt suggestions in the chat composer.
    #[serde(default = "default_prompt_suggestions_enabled")]
    pub enabled: bool,

    /// Lightweight model to use for suggestions.
    /// Leave empty to auto-select an efficient sibling of the main model.
    #[serde(default)]
    pub model: String,

    /// Temperature for inline prompt suggestion generation.
    #[serde(default = "default_prompt_suggestions_temperature")]
    pub temperature: f32,

    /// Whether VT Code should remind users that LLM-backed suggestions consume tokens.
    #[serde(default = "default_prompt_suggestions_show_cost_notice")]
    pub show_cost_notice: bool,
}

impl Default for AgentPromptSuggestionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_prompt_suggestions_enabled(),
            model: String::new(),
            temperature: default_prompt_suggestions_temperature(),
            show_cost_notice: default_prompt_suggestions_show_cost_notice(),
        }
    }
}

#[inline]
const fn default_prompt_suggestions_enabled() -> bool {
    true
}

#[inline]
const fn default_prompt_suggestions_temperature() -> f32 {
    0.3
}

#[inline]
const fn default_prompt_suggestions_show_cost_notice() -> bool {
    true
}
