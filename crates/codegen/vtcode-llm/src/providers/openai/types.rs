//! OpenAI provider types and constants.
//!
//! This module contains shared types used across the OpenAI provider implementation.

use serde_json::Value;

/// Responses API availability state for a given model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponsesApiState {
    /// Responses API is required for this model (e.g., GPT-5 Codex).
    Required,
    /// Responses API is allowed but not required.
    Allowed,
    /// Responses API is disabled (use Chat Completions).
    Disabled,
}

/// Payload structure for OpenAI Responses API requests.
pub(crate) struct OpenAIResponsesPayload {
    /// The input messages/items for the request.
    pub(crate) input: Vec<Value>,
    /// Optional system instructions.
    pub(crate) instructions: Option<String>,
}

/// Maximum completion tokens field name for Chat Completions API.
pub(crate) const MAX_COMPLETION_TOKENS_FIELD: &str = "max_completion_tokens";
