//! Model family definitions and capability groupings.
//!
//! A model family groups models that share certain characteristics like
//! context windows, supported features, and prompting strategies.

use serde::{Deserialize, Serialize};

use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

/// Default context window for most models
pub const DEFAULT_CONTEXT_WINDOW: i64 = 128_000;

/// Large context window (for models like Gemini)
pub const LARGE_CONTEXT_WINDOW: i64 = 1_048_576;

/// Medium context window
pub const MEDIUM_CONTEXT_WINDOW: i64 = 200_000;

/// Shell tool type preference for a model family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ShellToolType {
    /// Use default shell tool behavior
    #[default]
    Default,
    /// Use shell command tool
    ShellCommand,
    /// Use local shell execution
    Local,
    /// Use unified exec pattern (Codex-style)
    UnifiedExec,
}

/// Truncation policy for model output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TruncationPolicy {
    /// Truncate by byte count
    Bytes(usize),
    /// Truncate by token count
    Tokens(usize),
    /// No truncation
    None,
}

impl Default for TruncationPolicy {
    fn default() -> Self {
        TruncationPolicy::Bytes(10_000)
    }
}

/// A model family groups models that share certain characteristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFamily {
    /// The full model slug used to derive this model family
    pub slug: String,

    /// The model family name (e.g., "gemini-2.5", "claude-opus")
    pub family: String,

    /// The provider this model belongs to
    pub provider: Provider,

    /// Maximum supported context window, if known
    pub context_window: Option<i64>,

    /// Token threshold for automatic compaction
    pub auto_compact_token_limit: Option<i64>,

    /// Whether the model supports reasoning summaries
    pub supports_reasoning_summaries: bool,

    /// Default reasoning effort for this model family
    pub default_reasoning_effort: Option<ReasoningEffortLevel>,

    /// Whether this model supports parallel tool calls
    pub supports_parallel_tool_calls: bool,

    /// Whether the model needs special apply_patch instructions
    pub needs_special_apply_patch_instructions: bool,

    /// Preferred shell tool type for this model family
    pub shell_type: ShellToolType,

    /// Truncation policy for model output
    pub truncation_policy: TruncationPolicy,

    /// Names of experimental tools supported by this model family
    pub experimental_supported_tools: Vec<String>,

    /// Percentage of context window considered usable for inputs
    pub effective_context_window_percent: i64,

    /// Whether the model supports verbosity settings
    pub support_verbosity: bool,

    /// Whether the model supports tool use
    pub supports_tool_use: bool,

    /// Whether the model supports streaming
    pub supports_streaming: bool,

    /// Whether the model supports thinking/reasoning output
    pub supports_thinking: bool,
}

impl Default for ModelFamily {
    fn default() -> Self {
        Self {
            slug: String::new(),
            family: String::new(),
            provider: Provider::default(),
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            auto_compact_token_limit: None,
            supports_reasoning_summaries: false,
            default_reasoning_effort: None,
            supports_parallel_tool_calls: false,
            needs_special_apply_patch_instructions: false,
            shell_type: ShellToolType::Default,
            truncation_policy: TruncationPolicy::default(),
            experimental_supported_tools: Vec::new(),
            effective_context_window_percent: 95,
            support_verbosity: false,
            supports_tool_use: true,
            supports_streaming: true,
            supports_thinking: false,
        }
    }
}

impl ModelFamily {
    /// Create a new model family with the given slug
    pub fn new(slug: impl Into<String>, family: impl Into<String>, provider: Provider) -> Self {
        Self {
            slug: slug.into(),
            family: family.into(),
            provider,
            ..Default::default()
        }
    }

    /// Get the auto-compact token limit, computing a default if not set
    pub fn auto_compact_token_limit(&self) -> Option<i64> {
        self.auto_compact_token_limit
            .or(self.context_window.map(Self::default_auto_compact_limit))
    }

    /// Compute the default auto-compact limit (90% of context window)
    const fn default_auto_compact_limit(context_window: i64) -> i64 {
        (context_window * 9) / 10
    }

    /// Get the model slug
    pub fn get_model_slug(&self) -> &str {
        &self.slug
    }

    /// Check if this family supports a specific feature
    pub fn supports_feature(&self, feature: &str) -> bool {
        match feature {
            "reasoning" | "thinking" => self.supports_thinking,
            "tool_use" | "tools" => self.supports_tool_use,
            "streaming" => self.supports_streaming,
            "parallel_tools" => self.supports_parallel_tool_calls,
            _ => self
                .experimental_supported_tools
                .contains(&feature.to_string()),
        }
    }
}

/// Macro to simplify model family definitions
#[macro_export]
macro_rules! model_family {
    (
        $slug:expr, $family:expr, $provider:expr $(, $key:ident : $value:expr )* $(,)?
    ) => {{
        #[allow(unused_mut)]
        let mut mf = $crate::models_manager::ModelFamily::new($slug, $family, $provider);
        $(
            mf.$key = $value;
        )*
        mf
    }};
}

/// Internal helper that returns a `ModelFamily` for the given model slug.
pub fn find_family_for_model(slug: &str) -> ModelFamily {
    // Gemini models
    if slug.starts_with("gemini-3") {
        return model_family!(
            slug, "gemini-3", Provider::Gemini,
            context_window: Some(LARGE_CONTEXT_WINDOW),
            supports_thinking: true,
            supports_parallel_tool_calls: true,
            supports_reasoning_summaries: true,
        );
    }
    if slug.starts_with("gemini") {
        return model_family!(
            slug, "gemini", Provider::Gemini,
            context_window: Some(LARGE_CONTEXT_WINDOW),
        );
    }

    // OpenAI models
    if slug.starts_with("gpt-5") {
        return model_family!(
            slug, "gpt-5", Provider::OpenAI,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_parallel_tool_calls: true,
        );
    }
    if slug.starts_with("codex") {
        return model_family!(
            slug, "codex", Provider::OpenAI,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
            supports_thinking: true,
            shell_type: ShellToolType::UnifiedExec,
        );
    }
    if slug.starts_with("gpt-oss") || slug.contains("gpt-oss") {
        return model_family!(
            slug, "gpt-oss", Provider::OpenAI,
            context_window: Some(96_000),
        );
    }
    if slug.starts_with("o3") || slug.starts_with("o4") {
        return model_family!(
            slug, "o-series", Provider::OpenAI,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
            supports_thinking: true,
            supports_reasoning_summaries: true,
            needs_special_apply_patch_instructions: true,
        );
    }

    // Anthropic models
    if slug.starts_with("claude-opus") || slug.contains("opus") {
        return model_family!(
            slug, "claude-opus", Provider::Anthropic,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
            supports_thinking: true,
            supports_parallel_tool_calls: true,
        );
    }
    if slug.starts_with("claude-sonnet") || slug.contains("sonnet") {
        return model_family!(
            slug, "claude-sonnet", Provider::Anthropic,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
            supports_thinking: true,
        );
    }
    if slug.starts_with("claude-haiku") || slug.contains("haiku") {
        return model_family!(
            slug, "claude-haiku", Provider::Anthropic,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
        );
    }
    if slug.starts_with("claude") {
        return model_family!(
            slug, "claude", Provider::Anthropic,
            context_window: Some(MEDIUM_CONTEXT_WINDOW),
        );
    }

    // DeepSeek models
    if slug.contains("deepseek") && slug.contains("reason") {
        return model_family!(
            slug, "deepseek-reasoner", Provider::DeepSeek,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_thinking: true,
        );
    }
    if slug.contains("deepseek") {
        return model_family!(
            slug, "deepseek", Provider::DeepSeek,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
        );
    }

    // Z.AI GLM models
    if slug.contains("glm-5") {
        return model_family!(
            slug, "glm-5", Provider::ZAI,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_thinking: true,
        );
    }
    if slug.contains("glm") {
        return model_family!(
            slug, "glm", Provider::ZAI,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
        );
    }

    // MiniMax models
    if slug.contains("minimax") {
        return model_family!(
            slug, "minimax", Provider::Minimax,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_thinking: true,
        );
    }

    // Moonshot/Kimi models
    if slug.contains("kimi") || slug.contains("moonshot") {
        return model_family!(
            slug, "kimi", Provider::Moonshot,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_thinking: slug.contains("thinking"),
        );
    }

    // Qwen models (via OpenRouter or Ollama)
    if slug.contains("qwen") {
        return model_family!(
            slug, "qwen", Provider::OpenRouter,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
            supports_thinking: slug.contains("thinking"),
        );
    }

    // Ollama local models
    if slug.starts_with("ollama/") || slug.contains(":") {
        return model_family!(
            slug, "ollama-local", Provider::Ollama,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
        );
    }

    // OpenRouter models (fallback for unrecognized patterns)
    if slug.contains("/") {
        return model_family!(
            slug, "openrouter", Provider::OpenRouter,
            context_window: Some(DEFAULT_CONTEXT_WINDOW),
        );
    }

    // Default fallback
    model_family!(
        slug, "unknown", Provider::default(),
        context_window: Some(DEFAULT_CONTEXT_WINDOW),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_family_detection() {
        let family = find_family_for_model("gemini-3-flash-preview");
        assert_eq!(family.family, "gemini-3");
        assert_eq!(family.provider, Provider::Gemini);
        assert!(family.context_window.unwrap() >= LARGE_CONTEXT_WINDOW);
    }

    #[test]
    fn test_gpt5_family_detection() {
        let family = find_family_for_model("gpt-5.3-codex");
        assert_eq!(family.family, "gpt-5");
        assert_eq!(family.provider, Provider::OpenAI);
        assert!(family.supports_thinking);
    }

    #[test]
    fn test_claude_family_detection() {
        let family = find_family_for_model("claude-opus-4.5");
        assert_eq!(family.family, "claude-opus");
        assert_eq!(family.provider, Provider::Anthropic);
    }

    #[test]
    fn test_auto_compact_limit() {
        let family = ModelFamily {
            context_window: Some(100_000),
            ..Default::default()
        };
        assert_eq!(family.auto_compact_token_limit(), Some(90_000));
    }

    #[test]
    fn test_supports_feature() {
        let family = ModelFamily {
            supports_thinking: true,
            supports_tool_use: true,
            ..Default::default()
        };
        assert!(family.supports_feature("thinking"));
        assert!(family.supports_feature("tool_use"));
        assert!(!family.supports_feature("unknown"));
    }
}
