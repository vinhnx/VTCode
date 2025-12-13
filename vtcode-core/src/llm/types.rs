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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LLMErrorMetadata {
    pub provider: Option<String>,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub retry_after: Option<String>,
    pub message: Option<String>,
}

/// LLM error types with optional provider metadata
#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("API error: {message}")]
    ApiError {
        message: String,
        metadata: Option<LLMErrorMetadata>,
    },
    #[error("Network error: {message}")]
    NetworkError {
        message: String,
        metadata: Option<LLMErrorMetadata>,
    },
    #[error("Rate limit exceeded")]
    RateLimit { metadata: Option<LLMErrorMetadata> },
    #[error("Invalid request: {message}")]
    InvalidRequest {
        message: String,
        metadata: Option<LLMErrorMetadata>,
    },
}
