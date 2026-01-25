use async_stream::try_stream;
use async_trait::async_trait;

use super::{LLMRequest, LLMResponse, LLMStream, LLMStreamEvent};

/// Universal LLM provider trait
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Provider name (e.g., "gemini", "openai", "anthropic")
    fn name(&self) -> &str;

    /// Whether the provider has native streaming support
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Whether the provider surfaces structured reasoning traces for the given model
    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider accepts configurable reasoning effort for the model
    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports structured tool calling for the given model
    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    /// Whether the provider understands parallel tool configuration payloads
    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports structured output (JSON schema guarantees)
    fn supports_structured_output(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports prompt/context caching
    fn supports_context_caching(&self, _model: &str) -> bool {
        false
    }

    /// Whether the provider supports vision (image analysis) for given model
    fn supports_vision(&self, _model: &str) -> bool {
        false
    }

    /// Get the effective context window size for a model
    fn effective_context_size(&self, _model: &str) -> usize {
        // Default to 128k context window (common baseline)
        128_000
    }

    /// Generate completion
    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError>;

    /// Stream completion (optional)
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        // Default implementation falls back to non-streaming
        let response = self.generate(request).await?;
        let stream = try_stream! {
            yield LLMStreamEvent::Completed { response };
        };
        Ok(Box::pin(stream))
    }

    /// Get supported models
    fn supported_models(&self) -> Vec<String>;

    /// Validate request for this provider
    #[allow(clippy::result_large_err)]
    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LLMErrorMetadata {
    pub provider: &'static str,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub organization_id: Option<String>,
    pub retry_after: Option<String>,
    pub message: Option<String>,
}

impl LLMErrorMetadata {
    pub fn new(
        provider: &'static str,
        status: Option<u16>,
        code: Option<String>,
        request_id: Option<String>,
        organization_id: Option<String>,
        retry_after: Option<String>,
        message: Option<String>,
    ) -> Self {
        Self {
            provider,
            status,
            code,
            request_id,
            organization_id,
            retry_after,
            message,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[allow(clippy::result_large_err)]
pub enum LLMError {
    #[error("Authentication failed: {message}")]
    Authentication {
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
    #[error("Network error: {message}")]
    Network {
        message: String,
        metadata: Option<LLMErrorMetadata>,
    },
    #[error("Provider error: {message}")]
    Provider {
        message: String,
        metadata: Option<LLMErrorMetadata>,
    },
}

impl From<LLMError> for crate::llm::types::LLMError {
    fn from(err: LLMError) -> crate::llm::types::LLMError {
        let convert = |meta: Option<LLMErrorMetadata>| {
            meta.map(|m| crate::llm::types::LLMErrorMetadata {
                provider: Some(m.provider.to_string()),
                status: m.status,
                code: m.code,
                request_id: m.request_id,
                organization_id: m.organization_id,
                retry_after: m.retry_after,
                message: m.message,
            })
        };
        match err {
            LLMError::Authentication { message, metadata } => {
                crate::llm::types::LLMError::ApiError {
                    message,
                    metadata: convert(metadata),
                }
            }
            LLMError::RateLimit { metadata } => crate::llm::types::LLMError::RateLimit {
                metadata: convert(metadata),
            },
            LLMError::InvalidRequest { message, metadata } => {
                crate::llm::types::LLMError::InvalidRequest {
                    message,
                    metadata: convert(metadata),
                }
            }
            LLMError::Network { message, metadata } => crate::llm::types::LLMError::NetworkError {
                message,
                metadata: convert(metadata),
            },
            LLMError::Provider { message, metadata } => crate::llm::types::LLMError::ApiError {
                message,
                metadata: convert(metadata),
            },
        }
    }
}
