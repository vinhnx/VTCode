use serde::{Deserialize, Serialize};

/// OpenAI-specific provider configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OpenAIConfig {
    /// Enable Responses API WebSocket transport for non-streaming requests.
    /// This is an opt-in path designed for long-running, tool-heavy workflows.
    #[serde(default)]
    pub websocket_mode: bool,
}

/// Anthropic-specific provider configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicConfig {
    /// DEPRECATED: Model name validation has been removed. The Anthropic API validates
    /// model names directly, avoiding maintenance burden and allowing flexibility.
    /// This field is kept for backward compatibility but has no effect.
    #[deprecated(
        since = "0.75.0",
        note = "Model validation removed. API validates model names directly."
    )]
    #[serde(default)]
    pub skip_model_validation: bool,

    /// Enable extended thinking feature for Anthropic models
    /// When enabled, Claude uses internal reasoning before responding, providing
    /// enhanced reasoning capabilities for complex tasks.
    /// Only supported by Claude 4, Claude 4.5, and Claude 3.7 Sonnet models.
    /// Claude 4.6 uses adaptive thinking instead of extended thinking.
    /// Note: Extended thinking is now auto-enabled by default (31,999 tokens).
    /// Set MAX_THINKING_TOKENS=63999 environment variable for 2x budget on 64K models.
    /// See: https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking
    #[serde(default = "default_extended_thinking_enabled")]
    pub extended_thinking_enabled: bool,

    /// Beta header for interleaved thinking feature
    #[serde(default = "default_interleaved_thinking_beta")]
    pub interleaved_thinking_beta: String,

    /// Budget tokens for extended thinking (minimum: 1024, default: 31999)
    /// On 64K output models (Opus 4.5, Sonnet 4.5, Haiku 4.5): default 31,999, max 63,999
    /// On 32K output models (Opus 4): max 31,999
    /// Use MAX_THINKING_TOKENS environment variable to override.
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
    /// Supported by Claude Opus 4.5/4.6 (4.5 requires effort beta header)
    #[serde(default = "default_effort")]
    pub effort: String,

    /// Enable token counting via the count_tokens endpoint
    /// When enabled, the agent can estimate input token counts before making API calls
    /// Useful for proactive management of rate limits and costs
    #[serde(default = "default_count_tokens_enabled")]
    pub count_tokens_enabled: bool,
}

#[allow(deprecated)]
impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            skip_model_validation: false,
            extended_thinking_enabled: default_extended_thinking_enabled(),
            interleaved_thinking_beta: default_interleaved_thinking_beta(),
            interleaved_thinking_budget_tokens: default_interleaved_thinking_budget_tokens(),
            interleaved_thinking_type_enabled: default_interleaved_thinking_type(),
            tool_search: ToolSearchConfig::default(),
            effort: default_effort(),
            count_tokens_enabled: default_count_tokens_enabled(),
        }
    }
}

#[inline]
fn default_count_tokens_enabled() -> bool {
    false
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
fn default_extended_thinking_enabled() -> bool {
    true
}

#[inline]
fn default_interleaved_thinking_beta() -> String {
    "interleaved-thinking-2025-05-14".to_string()
}

#[inline]
fn default_interleaved_thinking_budget_tokens() -> u32 {
    31999
}

#[inline]
fn default_interleaved_thinking_type() -> String {
    "enabled".to_string()
}

#[inline]
fn default_effort() -> String {
    "low".to_string()
}

#[cfg(test)]
mod tests {
    use super::OpenAIConfig;

    #[test]
    fn openai_config_defaults_to_websocket_mode_disabled() {
        let config = OpenAIConfig::default();
        assert!(!config.websocket_mode);
    }

    #[test]
    fn openai_config_parses_websocket_mode_opt_in() {
        let parsed: OpenAIConfig =
            toml::from_str("websocket_mode = true").expect("config should parse");
        assert!(parsed.websocket_mode);
    }
}
