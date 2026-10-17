//! Small/lightweight model tier configuration.

use serde::{Deserialize, Serialize};

/// Small/lightweight model configuration for efficient operations
///
/// Following VT Code's pattern, use a smaller model (e.g., Haiku, GPT-4 Mini) for 50%+ of calls:
/// - Large file reads and parsing (>50KB)
/// - Web page summarization and analysis
/// - Git history and commit message processing
/// - One-word processing labels and simple classifications
///
/// Typically 70-80% cheaper than the main model while maintaining quality for these tasks.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSmallModelConfig {
    /// Enable small model tier for efficient operations
    #[serde(default = "default_small_model_enabled")]
    pub enabled: bool,

    /// Small model to use (e.g., claude-4-5-haiku, "gpt-4-mini", "gemini-2.0-flash")
    /// Leave empty to auto-select a lightweight sibling of the main model
    #[serde(default)]
    pub model: String,

    /// Temperature for small model responses
    #[serde(default = "default_small_model_temperature")]
    pub temperature: f32,

    /// Enable small model for large file reads (>50KB)
    #[serde(default = "default_small_model_for_large_reads")]
    pub use_for_large_reads: bool,

    /// Enable small model for web content summarization
    #[serde(default = "default_small_model_for_web_summary")]
    pub use_for_web_summary: bool,

    /// Enable small model for git history processing
    #[serde(default = "default_small_model_for_git_history")]
    pub use_for_git_history: bool,

    /// Enable small model for persistent memory classification and summary refresh
    #[serde(default = "default_small_model_for_memory")]
    pub use_for_memory: bool,
}

impl Default for AgentSmallModelConfig {
    fn default() -> Self {
        Self {
            enabled: default_small_model_enabled(),
            model: String::new(),
            temperature: default_small_model_temperature(),
            use_for_large_reads: default_small_model_for_large_reads(),
            use_for_web_summary: default_small_model_for_web_summary(),
            use_for_git_history: default_small_model_for_git_history(),
            use_for_memory: default_small_model_for_memory(),
        }
    }
}

#[inline]
const fn default_small_model_enabled() -> bool {
    true // Enable by default following VT Code pattern
}

#[inline]
const fn default_small_model_temperature() -> f32 {
    0.3 // More deterministic for parsing/summarization
}

#[inline]
const fn default_small_model_for_large_reads() -> bool {
    true
}

#[inline]
const fn default_small_model_for_web_summary() -> bool {
    true
}

#[inline]
const fn default_small_model_for_git_history() -> bool {
    true
}

#[inline]
const fn default_small_model_for_memory() -> bool {
    true
}
