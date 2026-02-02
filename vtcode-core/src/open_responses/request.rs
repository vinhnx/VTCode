//! Request object for Open Responses.
//!
//! The Request is the top-level object sent to the API,
//! containing input items, tool definitions, and model parameters.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{MessageRole, OutputItem};
use crate::llm::provider::ToolDefinition;

/// The main request object per the Open Responses specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The model to use for the request.
    pub model: String,

    /// The input items that form the context for the model.
    /// Per the spec, these are polymorphic items (messages, tool outputs, etc.).
    pub input: Vec<OutputItem>,

    /// Tools available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,

    /// Tool choice parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Whether to stream the response.
    #[serde(default)]
    pub stream: bool,

    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Maximum output tokens allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Maximum tool calls allowed in a single request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,

    /// Stop sequences for the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,

    /// Presence penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Logit bias for token sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<std::collections::HashMap<String, f64>>,

    /// Whether to return log probabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,

    /// Number of top log probabilities to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,

    /// User ID for tracking and rate limiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Service tier requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    /// Whether to store the request/response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// Metadata for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl Request {
    /// Creates a new request with the given model and input.
    pub fn new(model: impl Into<String>, input: Vec<OutputItem>) -> Self {
        Self {
            model: model.into(),
            input,
            tools: None,
            tool_choice: None,
            stream: false,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            max_tool_calls: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            user: None,
            service_tier: None,
            store: None,
            metadata: None,
        }
    }

    /// Convenience method to create a request from a single user message.
    pub fn from_message(model: impl Into<String>, text: impl Into<String>) -> Self {
        let item = OutputItem::completed_message(
            "msg_init",
            MessageRole::User,
            vec![super::ContentPart::input_text(text)],
        );
        Self::new(model, vec![item])
    }
}

/// Tool choice options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// Standard tool choice mode (auto, none, required).
    Mode(ToolChoiceMode),
    /// Specific tool to call.
    Tool(SpecificToolChoice),
}

/// Standard tool choice modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceMode {
    /// Model decides whether to call a tool.
    Auto,
    /// Model MUST NOT call any tools.
    None,
    /// Model MUST call at least one tool.
    Required,
}

/// Specific tool choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    /// The type of the tool, always "function".
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The name of the function to call.
    pub function: FunctionName,
}

/// Function name wrapper for tool choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionName {
    /// Name of the function.
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = Request::from_message("gpt-5", "Hello");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"gpt-5\""));
        assert!(json.contains("\"input\":["));
        assert!(json.contains("\"type\":\"message\""));
    }
}
