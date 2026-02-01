use serde::{Deserialize, Serialize};

/// Backend kind for LLM providers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    HuggingFace,
    Minimax,
}

/// Unified LLM response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<Usage>,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    pub request_id: Option<String>,
    pub organization_id: Option<String>,
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

impl Usage {
    #[inline]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let read = self.cache_read_tokens? as f64;
        let creation = self.cache_creation_tokens? as f64;
        let total = read + creation;
        if total > 0.0 {
            Some((read / total) * 100.0)
        } else {
            None
        }
    }

    #[inline]
    pub fn is_cache_hit(&self) -> Option<bool> {
        Some(self.cache_read_tokens? > 0)
    }

    #[inline]
    pub fn is_cache_miss(&self) -> Option<bool> {
        Some(self.cache_creation_tokens? > 0 && self.cache_read_tokens? == 0)
    }

    #[inline]
    pub fn total_cache_tokens(&self) -> usize {
        let read = self.cache_read_tokens.unwrap_or(0);
        let creation = self.cache_creation_tokens.unwrap_or(0);
        read + creation
    }

    #[inline]
    pub fn cache_savings_ratio(&self) -> Option<f64> {
        let read = self.cache_read_tokens? as f64;
        let prompt = self.prompt_tokens as f64;
        if prompt > 0.0 {
            Some(read / prompt)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LLMErrorMetadata {
    pub provider: Option<String>,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub organization_id: Option<String>,
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
