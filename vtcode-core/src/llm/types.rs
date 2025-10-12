use serde::{Deserialize, Serialize};

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

/// Unified LLM response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
    pub reasoning: Option<String>,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cached_prompt_tokens: Option<usize>,
    pub cache_creation_tokens: Option<usize>,
    pub cache_read_tokens: Option<usize>,
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
