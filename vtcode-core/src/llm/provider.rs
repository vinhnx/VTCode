//! Universal LLM provider abstraction with API-specific role handling
//!
//! This module provides a unified interface for different LLM providers (OpenAI, Anthropic, Gemini)
//! while properly handling their specific requirements for message roles and tool calling.
//!
//! ## Message Role Mapping
//!
//! Different LLM providers have varying support for message roles, especially for tool calling:
//!
//! ### OpenAI API
//! - **Full Support**: `system`, `user`, `assistant`, `tool`
//! - **Tool Messages**: Must include `tool_call_id` to reference the original tool call
//! - **Tool Calls**: Only `assistant` messages can contain `tool_calls`
//!
//! ### Anthropic API
//! - **Standard Roles**: `user`, `assistant`
//! - **System Messages**: Can be hoisted to system parameter or treated as user messages
//! - **Tool Responses**: Converted to `user` messages (no separate tool role)
//! - **Tool Choice**: Supports `auto`, `any`, `tool`, `none` modes
//!
//! ### Gemini API
//! - **Conversation Roles**: Only `user` and `model` (not `assistant`)
//! - **System Messages**: Handled separately as `systemInstruction` parameter
//! - **Tool Responses**: Converted to `user` messages with `functionResponse` format
//! - **Function Calls**: Uses `functionCall` in `model` messages
//!
//! ## Best Practices
//!
//! 1. Always use `MessageRole::tool_response()` constructor for tool responses
//! 2. Validate messages using `validate_for_provider()` before sending
//! 3. Use appropriate role mapping methods for each provider
//! 4. Handle provider-specific constraints (e.g., Gemini's system instruction requirement)
//!
//! ## Example Usage
//!
//! ```rust
//! use vtcode_core::llm::provider::{Message, MessageRole};
//!
//! // Create a proper tool response message
//! let tool_response = Message::tool_response(
//!     "call_123".to_string(),
//!     "Tool execution completed successfully".to_string()
//! );
//!
//! // Validate for specific provider
//! tool_response.validate_for_provider("openai").unwrap();
//! ```

use async_stream::try_stream;
use async_trait::async_trait;
use std::pin::Pin;

pub use crate::llm::interface::{
    FinishReason, FunctionCall, FunctionDefinition, LLMRequest, LLMResponse, Message, MessageRole,
    ParallelToolConfig, SpecificFunctionChoice, SpecificToolChoice, ToolCall, ToolChoice,
    ToolDefinition, Usage,
};

#[derive(Debug, Clone)]
pub enum LLMStreamEvent {
    Token {
        delta: String,
    },
    Reasoning {
        delta: String,
    },
    Completed {
        response: crate::llm::interface::LLMResponse,
    },
}

pub type LLMStream = Pin<Box<dyn futures::Stream<Item = Result<LLMStreamEvent, LLMError>> + Send>>;

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

    /// Generate completion
    async fn generate(
        &self,
        request: crate::llm::interface::LLMRequest,
    ) -> Result<crate::llm::interface::LLMResponse, LLMError>;

    /// Stream completion (optional)
    async fn stream(
        &self,
        request: crate::llm::interface::LLMRequest,
    ) -> Result<LLMStream, LLMError> {
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
    fn validate_request(&self, request: &crate::llm::interface::LLMRequest)
    -> Result<(), LLMError>;
}

#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("Authentication failed: {0}")]
    Authentication(String),
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Provider error: {0}")]
    Provider(String),
}

// Implement conversion from provider::LLMError to llm::types::LLMError
impl From<LLMError> for crate::llm::types::LLMError {
    fn from(err: LLMError) -> crate::llm::types::LLMError {
        match err {
            LLMError::Authentication(msg) => crate::llm::types::LLMError::ApiError(msg),
            LLMError::RateLimit => crate::llm::types::LLMError::RateLimit,
            LLMError::InvalidRequest(msg) => crate::llm::types::LLMError::InvalidRequest(msg),
            LLMError::Network(msg) => crate::llm::types::LLMError::NetworkError(msg),
            LLMError::Provider(msg) => crate::llm::types::LLMError::ApiError(msg),
        }
    }
}
