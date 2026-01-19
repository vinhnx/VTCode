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

    /// Tool search configuration for dynamic tool discovery (advanced-tool-use beta)
    #[serde(default)]
    pub tool_search: ToolSearchConfig,

    /// Effort level for token usage (high, medium, low)
    /// Controls how many tokens Claude uses when responding, trading off between
    /// response thoroughness and token efficiency.
    /// Only supported by Claude Opus 4.5 (claude-opus-4-5-20251101)
    #[serde(default = "default_effort")]
    pub effort: String,

    /// Enable token counting via the count_tokens endpoint
    /// When enabled, the agent can estimate input token counts before making API calls
    /// Useful for proactive management of rate limits and costs
    #[serde(default)]
    pub count_tokens_enabled: bool,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            interleaved_thinking_beta: default_interleaved_thinking_beta(),
            interleaved_thinking_budget_tokens: default_interleaved_thinking_budget_tokens(),
            interleaved_thinking_type_enabled: default_interleaved_thinking_type(),
            tool_search: ToolSearchConfig::default(),
            effort: default_effort(),
            count_tokens_enabled: false,
        }
    }
}

/// Configuration for Anthropic's tool search feature (advanced-tool-use beta)
/// Enables dynamic tool discovery for large tool catalogs (up to 10k tools)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolSearchConfig {
    /// Enable tool search feature (requires advanced-tool-use-2025-11-20 beta)
    #[serde(default)]
    pub enabled: bool,

    /// Search algorithm: "regex" (Python regex patterns) or "bm25" (natural language)
    #[serde(default = "default_tool_search_algorithm")]
    pub algorithm: String,

    /// Automatically defer loading of all tools except core tools
    #[serde(default = "default_defer_by_default")]
    pub defer_by_default: bool,

    /// Maximum number of tool search results to return
    #[serde(default = "default_max_results")]
    pub max_results: u32,

    /// Tool names that should never be deferred (always available)
    #[serde(default)]
    pub always_available_tools: Vec<String>,
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithm: default_tool_search_algorithm(),
            defer_by_default: default_defer_by_default(),
            max_results: default_max_results(),
            always_available_tools: vec![],
        }
    }
}

#[inline]
fn default_tool_search_algorithm() -> String {
    "regex".to_string()
}

#[inline]
fn default_defer_by_default() -> bool {
    true
}

#[inline]
fn default_max_results() -> u32 {
    5
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

#[inline]
fn default_effort() -> String {
    "low".to_string()
}
