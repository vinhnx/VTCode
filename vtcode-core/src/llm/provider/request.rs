use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use super::{Message, ToolDefinition};

/// Universal LLM request structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]

pub struct LLMRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<Arc<String>>,
    pub tools: Option<Arc<Vec<ToolDefinition>>>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,

    /// Optional structured output JSON schema to request from providers that support it
    /// For Anthropic this will be sent as `output_format: { type: "json_schema", schema: ... }`
    pub output_format: Option<Value>,

    /// Tool choice configuration based on official API docs
    /// Supports: "auto" (default), "none", "any", or specific tool selection
    pub tool_choice: Option<ToolChoice>,

    /// Whether to enable parallel tool calls (OpenAI specific)
    pub parallel_tool_calls: Option<bool>,

    /// Parallel tool use configuration following Anthropic best practices
    pub parallel_tool_config: Option<Box<ParallelToolConfig>>,

    /// Reasoning effort level for models that support it (none, low, medium, high)
    /// Applies to: Claude, GPT-5, GPT-5.1, Gemini, Qwen3, DeepSeek with reasoning capability
    pub reasoning_effort: Option<ReasoningEffortLevel>,

    /// Effort level for overall token usage (high, medium, low)
    /// Applies to: Claude Opus 4.5 (beta) and Claude Opus 4.6 (GA)
    /// Controls how many tokens Claude uses when responding, trading off between
    /// response thoroughness and token efficiency.
    pub effort: Option<String>,

    /// Verbosity level for output text (low, medium, high)
    /// Applies to: GPT-5.1 and other models that support verbosity control
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

    /// Optional provider-specific context management configuration (Anthropic compaction).
    pub context_management: Option<Value>,

    /// Optional prefill text for the assistant response (Anthropic prefilling)
    /// Incompatible with extended thinking
    pub prefill: Option<String>,

    /// Whether to enable character reinforcement (system prompt/prefill tagging)
    pub character_reinforcement: bool,

    /// Optional character name for reinforcement
    pub character_name: Option<String>,

    /// Optional coding agent specific settings
    pub coding_agent_settings: Option<Box<CodingAgentSettings>>,

    /// Optional turn metadata for git context (remote URLs, commit hash, etc.)
    /// This is sent as X-Turn-Metadata header to providers that support it
    pub metadata: Option<serde_json::Value>,

    /// Optional provider routing hint for prompt cache stickiness.
    /// OpenAI uses this value to improve routing locality for repeated prefixes.
    pub prompt_cache_key: Option<String>,
}

/// Settings to refine model behavior for coding agent tasks
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodingAgentSettings {
    /// Encourage the model to use XML tags for structured responses
    pub force_xml_tags: bool,
    /// Automatically prefill with <thought> to encourage reasoning
    pub prefill_thought: bool,
    /// Explicitly allow the model to say "I don't know" or "I am unsure"
    pub allow_uncertainty: bool,
    /// Enforce strict grounding to provided documents
    pub strict_grounding: bool,
    /// Optimize for long context by hoisting large messages and grounding in quotes
    pub long_context_optimization: bool,
    /// Wrap multiple file contexts in structured XML tags
    pub use_xml_document_format: bool,
    /// Inject instructions to find quotes before carrying out the task
    pub force_quote_grounding: bool,
    /// Optional specialized role for Claude (e.g., "Senior Software Architect")
    pub role_specialization: Option<String>,
    /// Enforce the use of <thinking> and <answer> tags for manual chain-of-thought
    pub enforce_structured_thought: bool,
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
}

/// Specific tool choice for forcing a particular function call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub tool_type: String, // "function"

    pub function: SpecificFunctionChoice,
}

/// Specific function choice details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunctionChoice {
    pub name: String,
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
    pub fn any() -> Self {
        Self::Any
    }

    /// Create specific function tool choice
    pub fn function(name: String) -> Self {
        Self::Specific(SpecificToolChoice {
            tool_type: "function".to_owned(),
            function: SpecificFunctionChoice { name },
        })
    }

    /// Check if this tool choice allows parallel tool use
    /// Based on Anthropic's parallel tool use guidelines
    pub fn allows_parallel_tools(&self) -> bool {
        match self {
            // Auto allows parallel tools by default
            Self::Auto => true,
            // Any forces at least one tool, may allow parallel
            Self::Any => true,
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
            Self::Specific(_) => "Specific tool must be used (no parallel)",
        }
    }

    /// OpenAI-compatible providers that share the same tool_choice format
    const OPENAI_STYLE_PROVIDERS: &'static [&'static str] = &[
        "openai",
        "deepseek",
        "huggingface",
        "openrouter",
        "xai",
        "zai",
        "moonshot",
        "lmstudio",
    ];

    /// Convert to provider-specific format
    #[inline]
    pub fn to_provider_format(&self, provider: &str) -> Value {
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
    fn to_openai_format(&self) -> Value {
        match self {
            Self::Auto => json!("auto"),
            Self::None => json!("none"),
            Self::Any => json!("required"),
            Self::Specific(choice) => json!(choice),
        }
    }

    #[inline]
    fn to_anthropic_format(&self) -> Value {
        match self {
            Self::Auto => json!({"type": "auto"}),
            Self::None => json!({"type": "none"}),
            Self::Any => json!({"type": "any"}),
            Self::Specific(choice) => json!({"type": "tool", "name": &choice.function.name}),
        }
    }

    #[inline]
    fn to_gemini_format(&self) -> Value {
        match self {
            Self::Auto => json!({"mode": "auto"}),
            Self::None => json!({"mode": "none"}),
            Self::Any => json!({"mode": "any"}),
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
    pub disable_parallel_tool_use: bool,

    /// Maximum number of tools to execute in parallel
    /// None means no limit (provider default)
    pub max_parallel_tools: Option<usize>,

    /// Whether to encourage parallel tool use in prompts
    pub encourage_parallel: bool,
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
    pub fn sequential_only() -> Self {
        Self {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(1),
            encourage_parallel: false,
        }
    }
}
