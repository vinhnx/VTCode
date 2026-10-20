use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use vtcode_config::types::{ReasoningEffortLevel, VerbosityLevel};

use super::{Message, ToolDefinition};

/// Fallback model configuration for Anthropic server-side fallback.
/// The explicit-list form of `fallbacks`, sent with the
/// `server-side-fallback-2026-06-01` beta header (the `"default"` keyword form
/// uses `server-side-fallback-2026-07-01`; pairing a header with the other form
/// is rejected).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FallbackModel {
    /// The model identifier to fall back to (e.g., "claude-opus-5")
    pub(crate) model: String,
    /// Optional max_tokens override for this fallback attempt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    /// Optional thinking configuration override for this fallback attempt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<AnthropicThinkingConfig>,
}

/// Anthropic thinking configuration for fallback models.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AnthropicThinkingConfig {
    #[default]
    Disabled,
    Enabled {
        budget_tokens: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        display: Option<String>,
    },
    Adaptive {
        #[serde(skip_serializing_if = "Option::is_none")]
        display: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheProfile {
    BudgetContinuation,
    /// Cache only stable instructions for an independent auxiliary request.
    /// Native GPT-5.6+ uses explicit-only caching; other routes ignore this profile.
    StablePrefix,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicThinkingModeOverride {
    #[default]
    Inherit,
    Disabled,
    Adaptive,
    ManualBudget(u32),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicThinkingDisplayOverride {
    #[default]
    Inherit,
    Summarized,
    Omitted,
    Updates,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicOptionalStringOverride {
    #[default]
    Inherit,
    Omit,
    Explicit(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicOptionalU32Override {
    #[default]
    Inherit,
    Omit,
    Explicit(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnthropicRequestOverrides {
    /// Explicit request speed takes precedence over provider configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<vtcode_config::AnthropicSpeed>,
    #[serde(default)]
    pub(crate) thinking_mode: AnthropicThinkingModeOverride,
    #[serde(default)]
    pub(crate) thinking_display: AnthropicThinkingDisplayOverride,
    #[serde(default)]
    pub(crate) effort: AnthropicOptionalStringOverride,
    #[serde(default)]
    pub(crate) task_budget_tokens: AnthropicOptionalU32Override,
}

/// Universal LLM request structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]

pub struct LLMRequest {
    /// Conversation history shared via `Arc` so per-turn request construction
    /// and continuation bookkeeping are O(1) instead of deep-copying the
    /// entire history (which grows unbounded over a session).
    pub messages: Arc<Vec<Message>>,
    /// Shared via `Arc<str>` (single allocation) so per-turn request
    /// construction can reuse the prompt envelope's `Arc<str>` via a cheap
    /// refcount bump instead of cloning the full system prompt string.
    pub system_prompt: Option<Arc<str>>,
    pub tools: Option<Arc<Vec<ToolDefinition>>>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,

    /// Optional structured output JSON schema to request from providers that support it
    /// For Anthropic this will be sent as `output_config.format = { type: "json_schema", schema: ... }`
    pub output_format: Option<Value>,

    /// Tool choice configuration based on official API docs
    /// Supports: "auto" (default), "none", "any", or specific tool selection
    pub tool_choice: Option<ToolChoice>,

    /// Whether to enable parallel tool calls (OpenAI specific)
    pub parallel_tool_calls: Option<bool>,

    /// Parallel tool use configuration following Anthropic best practices
    pub parallel_tool_config: Option<Box<ParallelToolConfig>>,

    /// Reasoning effort level for models that support it (none, minimal, low, medium, high, xhigh)
    /// Applies to: Claude, GPT-5 family, Gemini, Qwen3, DeepSeek with reasoning capability
    pub reasoning_effort: Option<ReasoningEffortLevel>,

    /// Effort level for overall token usage (low, medium, high, xhigh, max)
    /// Applies to: Anthropic adaptive-thinking models such as Claude Opus 4.7
    /// Controls how many tokens Claude uses when responding, trading off between
    /// response thoroughness and token efficiency.
    pub effort: Option<String>,

    /// Verbosity level for output text (low, medium, high)
    /// Applies to: GPT-5.4-family Responses workflows and other models that support verbosity control
    pub verbosity: Option<VerbosityLevel>,

    /// Advanced generation parameters
    pub do_sample: Option<bool>,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
    /// Optional budget for extended thinking (Anthropic specific)
    /// Minimum value: 1024
    pub thinking_budget: Option<u32>,

    /// Optional beta headers for Anthropic (and potentially others)
    pub betas: Option<Vec<String>>,

    /// Optional provider-specific context management configuration (Anthropic compaction/editing).
    pub context_management: Option<Value>,

    /// Optional coding agent specific settings
    pub coding_agent_settings: Option<Box<CodingAgentSettings>>,

    /// Optional turn metadata for git context (remote URLs, commit hash, etc.)
    /// This is sent as X-Turn-Metadata header to providers that support it
    pub metadata: Option<Value>,

    /// Optional Responses API continuity pointer for server-side context chaining.
    /// Used by providers that support stateful response continuation.
    pub previous_response_id: Option<String>,

    /// Optional Responses API storage flag.
    /// When set, providers that support `store` pass this through directly.
    pub response_store: Option<bool>,

    /// Optional Responses API include fields (e.g. reasoning encrypted content).
    /// Passed through only for providers/APIs that support include selectors.
    pub responses_include: Option<Vec<String>>,

    /// Optional native OpenAI `service_tier` request parameter.
    /// Passed through only for native OpenAI endpoints that support service tiers.
    pub service_tier: Option<String>,

    /// Optional provider routing hint for prompt cache stickiness.
    /// OpenAI uses this value to improve routing locality for repeated prefixes.
    pub prompt_cache_key: Option<String>,

    /// Optional request-scoped prompt cache profile for provider-specific TTL overrides.
    pub prompt_cache_profile: Option<PromptCacheProfile>,

    /// Optional explicit fallback models for Anthropic server-side fallback.
    /// Overrides `provider.anthropic.fallbacks`; sent with the
    /// `server-side-fallback-2026-06-01` beta header. Subject to the same
    /// gates as the config list (first-party endpoint, a model that supports
    /// server-side fallbacks, no credit-token retry) and the same entry
    /// validation; an empty list sends nothing.
    pub fallbacks: Option<Vec<FallbackModel>>,

    /// Optional opaque credit token from a refused request's `stop_details.fallback_credit_token`.
    /// Echoed on the retry to avoid paying the prompt-cache cost twice.
    /// Sent with the `fallback-credit-2026-07-01` beta header. The refused request must have
    /// carried that header or `server-side-fallback-2026-07-01`, which grants the same
    /// `stop_details` fields.
    pub fallback_credit_token: Option<String>,

    /// Optional Anthropic-specific request overrides used when request semantics must
    /// not inherit VT Code's provider defaults, such as the Anthropic compatibility server.
    pub anthropic_request_overrides: Option<AnthropicRequestOverrides>,
}

impl LLMRequest {
    /// Budget a short auxiliary answer separately from any hidden reasoning.
    ///
    /// Select low effort only when advertised; otherwise retain the provider's
    /// default. Reasoning routes receive 2,048 additional output tokens and no
    /// sampling controls. This is a bounded allowance, not a completion guarantee.
    /// Reapply after changing `model`, including on a fallback route.
    pub fn set_auxiliary_output_budget(
        &mut self,
        provider: &(impl super::LLMProvider + ?Sized),
        visible_output_tokens: u32,
    ) {
        let levels = provider.supported_reasoning_efforts(&self.model);
        let reasoning = provider.supports_reasoning(&self.model) || !levels.is_empty();
        self.reasoning_effort = levels.contains(&"low").then_some(ReasoningEffortLevel::Low);
        self.max_tokens = Some(visible_output_tokens.max(1).saturating_add(if reasoning { 2048 } else { 0 }));
        self.verbosity = Some(VerbosityLevel::Low);
        if reasoning {
            self.temperature = None;
            self.top_p = None;
            self.top_k = None;
            self.do_sample = None;
            self.presence_penalty = None;
            self.frequency_penalty = None;
        }
    }
}

/// Per-model sampling parameter overrides resolved from a custom provider
/// profile. Every field defaults to `None`, meaning the agent loop's global
/// configuration value applies unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SamplingOverrides {
    /// Sampling temperature (0.0-2.0).
    pub temperature: Option<f32>,
    /// Nucleus sampling mass (0.0-1.0).
    pub top_p: Option<f32>,
    /// Top-k token cutoff (>= 0).
    pub top_k: Option<i32>,
    /// Presence penalty (-2.0-2.0).
    pub presence_penalty: Option<f32>,
    /// Frequency penalty (-2.0-2.0).
    pub frequency_penalty: Option<f32>,
    /// Maximum output tokens (> 0).
    pub max_tokens: Option<u32>,
    /// Reasoning effort for models that accept it.
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    /// Whether this model's wire format rejects sampling parameters while
    /// reasoning is active (e.g. custom endpoints speaking the Anthropic
    /// Messages shape).
    pub suppresses_sampling_with_reasoning: bool,
    /// Set by providers that resolve overrides from an explicit user profile;
    /// callers use it to trust these values over name-based heuristics.
    pub profile_aware: bool,
}

impl SamplingOverrides {
    /// Whether sampling parameters must be dropped for this request: either
    /// the model's wire format rejects sampling while reasoning is active, or
    /// a built-in Anthropic/MiniMax-shaped backend (`native_match`) enforces
    /// the same rule outside of profile-aware routing.
    pub fn suppresses_sampling(&self, native_match: bool, reasoning_active: bool) -> bool {
        reasoning_active && (self.suppresses_sampling_with_reasoning || (!self.profile_aware && native_match))
    }
}

/// Optional overrides for standalone Responses compaction requests.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ResponsesCompactionOptions {
    /// Optional custom instructions appended to the derived replay instructions.
    pub instructions: Option<String>,
    /// Optional output token limit for the compaction response.
    pub max_output_tokens: Option<u32>,
    /// Optional reasoning effort override for the compaction pass.
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    /// Optional verbosity override for the compaction output text settings.
    pub verbosity: Option<VerbosityLevel>,
    /// Optional include selectors override.
    pub responses_include: Option<Vec<String>>,
    /// Optional storage override.
    pub response_store: Option<bool>,
    /// Optional native OpenAI service tier override.
    pub service_tier: Option<String>,
    /// Optional prompt cache routing override.
    pub prompt_cache_key: Option<String>,
}

/// Settings to refine model behavior for coding agent tasks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodingAgentSettings {
    /// Optimize for long context by hoisting the largest user message.
    /// Hoisting reorders earlier turns, so the Anthropic builder skips it on
    /// preserved-thinking models (Claude Sonnet 5.5, Claude Opus 5.5, Claude
    /// Fable 5.1).
    pub(crate) long_context_optimization: bool,
}

/// Tool choice configuration that works across different providers
/// Based on OpenAI, Anthropic, and Gemini API specifications
/// Follows Anthropic's tool use best practices for optimal performance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum ToolChoice {
    /// Let the model decide whether to call tools ("auto")
    /// Default behavior - allows model to use tools when appropriate
    #[default]
    Auto,

    /// Force the model to not call any tools ("none")
    /// Useful for pure conversational responses without tool usage
    None,

    /// Force the model to call at least one tool ("any")
    /// Ensures tool usage even when model might prefer direct response
    Any,

    /// Force the model to call a specific tool
    /// Useful for directing model to use particular functionality
    Specific(SpecificToolChoice),

    /// OpenAI Responses native allowed-tools filtering.
    ///
    /// This is advisory provider-side filtering only; runtime dispatch remains
    /// authoritative. Providers without native support should degrade this to
    /// the equivalent auto/required tool choice while keeping the full
    /// catalogue visible.
    AllowedTools(AllowedToolsChoice),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AllowedToolsMode {
    Auto,
    Required,
}

impl AllowedToolsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Required => "required",
        }
    }

    fn anthropic_mode(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Required => "any",
        }
    }

    fn gemini_mode(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Required => "any",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedToolsChoice {
    pub mode: AllowedToolsMode,
    pub tools: Vec<String>,
}

/// Specific tool choice for forcing a particular function call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub(crate) tool_type: String, // "function"

    pub(crate) function: SpecificFunctionChoice,
}

/// Specific function choice details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunctionChoice {
    pub(crate) name: String,
}

impl ToolChoice {
    /// Create auto tool choice (default behavior)
    pub fn auto() -> Self {
        Self::Auto
    }

    /// Create none tool choice (disable tool calling)
    pub fn none() -> Self {
        Self::None
    }

    /// Create any tool choice (force at least one tool call)
    pub(crate) fn any() -> Self {
        Self::Any
    }

    /// Create specific function tool choice
    pub(crate) fn function(name: String) -> Self {
        Self::Specific(SpecificToolChoice {
            tool_type: "function".to_owned(),
            function: SpecificFunctionChoice { name },
        })
    }

    pub fn allowed_tools_auto(tools: Vec<String>) -> Self {
        Self::AllowedTools(AllowedToolsChoice { mode: AllowedToolsMode::Auto, tools })
    }

    /// Check if this tool choice allows parallel tool use
    /// Based on Anthropic's parallel tool use guidelines
    pub fn allows_parallel_tools(&self) -> bool {
        match self {
            // Auto allows parallel tools by default
            Self::Auto => true,
            // Any forces at least one tool, may allow parallel
            Self::Any => true,
            Self::AllowedTools(choice) => matches!(choice.mode, AllowedToolsMode::Auto),
            // Specific forces one particular tool, typically no parallel
            Self::Specific(_) => false,
            // None disables tools entirely
            Self::None => false,
        }
    }

    /// Get human-readable description of tool choice behavior
    pub fn description(&self) -> &'static str {
        match self {
            Self::Auto => "Model decides when to use tools (allows parallel)",
            Self::None => "No tools will be used",
            Self::Any => "At least one tool must be used (allows parallel)",
            Self::AllowedTools(_) => "Model decides when to use the currently allowed tool subset",
            Self::Specific(_) => "Specific tool must be used (no parallel)",
        }
    }

    /// OpenAI-compatible providers that share the same tool_choice format
    const OPENAI_STYLE_PROVIDERS: &'static [&'static str] = &[
        "openai",
        "deepseek",
        "huggingface",
        "mistral",
        "openrouter",
        "zai",
        "moonshot",
        "stepfun",
        "evolink",
        "lmstudio",
        "llamacpp",
        "meta",
    ];

    /// Convert to provider-specific format
    #[inline]
    pub(crate) fn to_provider_format(&self, provider: &str) -> Value {
        if Self::OPENAI_STYLE_PROVIDERS.contains(&provider) {
            return self.to_openai_format();
        }

        match provider {
            "anthropic" => self.to_anthropic_format(),
            "gemini" => self.to_gemini_format(),
            _ => self.to_openai_format(), // Default to OpenAI format
        }
    }

    #[inline]
    pub(crate) fn to_responses_format(&self) -> Value {
        match self {
            Self::Specific(choice) => json!({"type": "function", "name": &choice.function.name}),
            _ => self.to_openai_format(),
        }
    }

    #[inline]
    fn to_openai_format(&self) -> Value {
        match self {
            Self::Auto => json!("auto"),
            Self::None => json!("none"),
            Self::Any => json!("required"),
            Self::Specific(choice) => json!(choice),
            Self::AllowedTools(choice) => json!(choice.mode.as_str()),
        }
    }

    #[inline]
    fn to_anthropic_format(&self) -> Value {
        match self {
            Self::Auto => json!({"type": "auto"}),
            Self::None => json!({"type": "none"}),
            Self::Any => json!({"type": "any"}),
            Self::Specific(choice) => json!({"type": "tool", "name": &choice.function.name}),
            Self::AllowedTools(choice) => json!({"type": choice.mode.anthropic_mode()}),
        }
    }

    #[inline]
    fn to_gemini_format(&self) -> Value {
        match self {
            Self::Auto => json!({"mode": "auto"}),
            Self::None => json!({"mode": "none"}),
            Self::Any => json!({"mode": "any"}),
            Self::AllowedTools(choice) => json!({"mode": choice.mode.gemini_mode()}),
            Self::Specific(choice) => {
                json!({"mode": "any", "allowed_function_names": [&choice.function.name]})
            }
        }
    }
}

/// Configuration for parallel tool use behavior
/// Based on Anthropic's parallel tool use guidelines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelToolConfig {
    /// Whether to disable parallel tool use
    /// When true, forces sequential tool execution
    pub(crate) disable_parallel_tool_use: bool,

    /// Maximum number of tools to execute in parallel
    /// None means no limit (provider default)
    pub(crate) max_parallel_tools: Option<usize>,

    /// Whether to encourage parallel tool use in prompts
    pub(crate) encourage_parallel: bool,
}

impl Default for ParallelToolConfig {
    fn default() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: Some(5), // Reasonable default
            encourage_parallel: true,
        }
    }
}

impl ParallelToolConfig {
    /// Create configuration optimized for Anthropic models
    pub fn anthropic_optimized() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: None, // Let Anthropic decide
            encourage_parallel: true,
        }
    }

    /// Create configuration for sequential tool use
    pub(crate) fn sequential_only() -> Self {
        Self {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(1),
            encourage_parallel: false,
        }
    }
}
