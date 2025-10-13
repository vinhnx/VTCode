//!
//! Legacy type exports for the LLM layer.
//!
//! Historically, downstream crates imported chat request/response structures
//! from this module. The reusable interface module now owns those definitions,
//! but we re-export them here to preserve the existing public surface until a
//! major version bump can land.

pub use crate::llm::interface::{
    FinishReason, FunctionCall, FunctionDefinition, LLMRequest, Message, MessageRole,
    ParallelToolConfig, SpecificFunctionChoice, SpecificToolChoice, ToolCall, ToolChoice,
    ToolDefinition,
};

pub use crate::llm::interface::LLMResponse as ProviderLLMResponse;
pub use crate::llm::interface::Usage as ProviderUsage;

/// Provider-facing response type kept for backward compatibility.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
    pub reasoning: Option<String>,
}

impl LLMResponse {
    /// Create a legacy response from a provider response and model identifier.
    pub fn from_provider(model: String, response: crate::llm::interface::LLMResponse) -> Self {
        Self {
            content: response.content.unwrap_or_default(),
            model,
            usage: response.usage.map(Into::into),
            reasoning: response.reasoning,
        }
    }
}

/// Legacy usage metrics compatible with downstream crates.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cached_prompt_tokens: Option<usize>,
    pub cache_creation_tokens: Option<usize>,
    pub cache_read_tokens: Option<usize>,
}

impl From<crate::llm::interface::Usage> for Usage {
    fn from(value: crate::llm::interface::Usage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens as usize,
            completion_tokens: value.completion_tokens as usize,
            total_tokens: value.total_tokens as usize,
            cached_prompt_tokens: value.cached_prompt_tokens.map(|v| v as usize),
            cache_creation_tokens: value.cache_creation_tokens.map(|v| v as usize),
            cache_read_tokens: value.cache_read_tokens.map(|v| v as usize),
        }
    }
}

impl From<Usage> for crate::llm::interface::Usage {
    fn from(value: Usage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens as u32,
            completion_tokens: value.completion_tokens as u32,
            total_tokens: value.total_tokens as u32,
            cached_prompt_tokens: value.cached_prompt_tokens.map(|v| v as u32),
            cache_creation_tokens: value.cache_creation_tokens.map(|v| v as u32),
            cache_read_tokens: value.cache_read_tokens.map(|v| v as u32),
        }
    }
}

/// Backend kind for LLM providers
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    Gemini,
    OpenAI,
    Anthropic,
    DeepSeek,
    OpenRouter,
    Ollama,
    XAI,
    ZAI,
    Moonshot,
}

/// LLM error types
#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}
