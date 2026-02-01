use async_stream::try_stream;
use async_trait::async_trait;

use super::{LLMRequest, LLMResponse, LLMStream, LLMStreamEvent};
pub use vtcode_commons::llm::{LLMError, LLMErrorMetadata};

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
            yield LLMStreamEvent::Completed { response: Box::new(response) };
        };
        Ok(Box::pin(stream))
    }

    /// Get supported models
    fn supported_models(&self) -> Vec<String>;

    /// Validate request for this provider
    #[allow(clippy::result_large_err)]
    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError>;
}
