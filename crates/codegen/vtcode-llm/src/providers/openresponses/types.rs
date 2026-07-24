//! OpenResponses specification types.
//!
//! This module defines types that conform to the OpenResponses specification.
//! See <https://www.openresponses.org/specification> for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Item Types - Core units of context in OpenResponses
// ============================================================================

/// The type of an item in the OpenResponses API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    /// A message item (user, assistant, system, or developer).
    Message,
    /// A function call item.
    FunctionCall,
    /// A function call output item.
    FunctionCallOutput,
    /// A reasoning item.
    Reasoning,
    /// An item reference.
    ItemReference,
    /// Catch-all for unknown item types added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

/// Role for message items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Developer,
    /// Catch-all for unknown roles added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

/// Status for items that have a lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    InProgress,
    Completed,
    Failed,
    /// Catch-all for unknown statuses added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

/// Status for function calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionCallStatus {
    InProgress,
    Completed,
    Failed,
    /// Catch-all for unknown statuses added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

// ============================================================================
// Content Types - Building blocks for message content
// ============================================================================

/// Input text content for user/system/developer messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTextContent {
    #[serde(rename = "type")]
    content_type: String, // "input_text"
    text: String,
}

impl InputTextContent {
    fn new(text: impl Into<String>) -> Self {
        Self {
            content_type: "input_text".to_string(),
            text: text.into(),
        }
    }
}

/// Output text content for assistant messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTextContent {
    #[serde(rename = "type")]
    content_type: String, // "output_text"
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotations: Option<Vec<Annotation>>,
}

impl OutputTextContent {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            content_type: "output_text".to_string(),
            text: text.into(),
            annotations: None,
        }
    }
}

/// Refusal content for when the model refuses a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefusalContent {
    #[serde(rename = "type")]
    content_type: String, // "refusal"
    refusal: String,
}

/// Image detail level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Low,
    High,
    Auto,
    Original,
    /// Catch-all for unknown detail levels.
    #[serde(other)]
    Unknown,
}

/// Input image content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputImageContent {
    #[serde(rename = "type")]
    content_type: String, // "input_image"
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<ImageDetail>,
}

/// Input file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputFileContent {
    #[serde(rename = "type")]
    content_type: String, // "input_file"
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_url: Option<String>,
}

/// Annotation for citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Annotation {
    #[serde(rename = "url_citation")]
    UrlCitation {
        start_index: usize,
        end_index: usize,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Catch-all for unknown annotation types added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

/// Content part union for input messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputContent {
    Text(InputTextContent),
    Image(InputImageContent),
    File(InputFileContent),
}

/// Content part union for output messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputContent {
    Text(OutputTextContent),
    Refusal(RefusalContent),
}

// ============================================================================
// Item Params - Items that can be sent as input
// ============================================================================

/// User message item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessageItemParam {
    #[serde(rename = "type")]
    item_type: String, // "message"
    role: String, // "user"
    content: Vec<InputContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

impl UserMessageItemParam {
    fn new(text: impl Into<String>) -> Self {
        Self {
            item_type: "message".to_string(),
            role: "user".to_string(),
            content: vec![InputContent::Text(InputTextContent::new(text))],
            id: None,
            status: None,
        }
    }
}

/// System message item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessageItemParam {
    #[serde(rename = "type")]
    item_type: String, // "message"
    role: String, // "system"
    content: Vec<InputTextContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

impl SystemMessageItemParam {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            item_type: "message".to_string(),
            role: "system".to_string(),
            content: vec![InputTextContent::new(text)],
            id: None,
            status: None,
        }
    }
}

/// Developer message item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperMessageItemParam {
    #[serde(rename = "type")]
    item_type: String, // "message"
    role: String, // "developer"
    content: Vec<InputTextContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

/// Assistant message item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageItemParam {
    #[serde(rename = "type")]
    item_type: String, // "message"
    role: String, // "assistant"
    content: Vec<OutputContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

impl AssistantMessageItemParam {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            item_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![OutputContent::Text(OutputTextContent::new(text))],
            id: None,
            status: None,
        }
    }
}

/// Function call item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallItemParam {
    #[serde(rename = "type")]
    item_type: String, // "function_call"
    id: String,
    call_id: String,
    name: String,
    arguments: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<FunctionCallStatus>,
}

/// Function call output item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallOutputItemParam {
    #[serde(rename = "type")]
    item_type: String, // "function_call_output"
    call_id: String,
    output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<FunctionCallStatus>,
}

impl FunctionCallOutputItemParam {
    fn new(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            item_type: "function_call_output".to_string(),
            call_id: call_id.into(),
            output: output.into(),
            id: None,
            status: None,
        }
    }
}

/// Reasoning summary content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSummaryContent {
    #[serde(rename = "type")]
    content_type: String, // "summary_text"
    text: String,
}

/// Reasoning item parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningItemParam {
    #[serde(rename = "type")]
    item_type: String, // "reasoning"
    summary: Vec<ReasoningSummaryContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encrypted_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<ReasoningUsage>,
}

/// Usage information for reasoning items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u32>,
}

/// Item reference parameter for referencing previous items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemReferenceParam {
    #[serde(rename = "type")]
    item_type: String, // "item_reference"
    id: String,
}

/// Union of all item parameters that can be sent as input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemParam {
    UserMessage(UserMessageItemParam),
    SystemMessage(SystemMessageItemParam),
    DeveloperMessage(DeveloperMessageItemParam),
    AssistantMessage(AssistantMessageItemParam),
    FunctionCall(FunctionCallItemParam),
    FunctionCallOutput(FunctionCallOutputItemParam),
    Reasoning(ReasoningItemParam),
    ItemReference(ItemReferenceParam),
}

// ============================================================================
// Tool Types
// ============================================================================

/// Function tool parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionToolParam {
    #[serde(rename = "type")]
    tool_type: String, // "function"
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict: Option<bool>,
}

impl FunctionToolParam {
    fn new(name: impl Into<String>) -> Self {
        Self {
            tool_type: "function".to_string(),
            name: name.into(),
            description: None,
            parameters: None,
            strict: None,
        }
    }

    fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn with_parameters(mut self, parameters: Value) -> Self {
        self.parameters = Some(parameters);
        self
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }
}

/// Tool choice values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceValue {
    Auto,
    None,
    Required,
    /// Catch-all for unknown tool choice values.
    #[serde(other)]
    Unknown,
}

/// Specific function choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunctionChoice {
    #[serde(rename = "type")]
    choice_type: String, // "function"
    name: String,
}

/// Tool choice parameter union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoiceParam {
    Value(ToolChoiceValue),
    Specific(SpecificFunctionChoice),
}

/// Incomplete details for partial responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncompleteDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    code: String,
    message: String,
}

/// Response status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Queued,
    #[default]
    InProgress,
    Completed,
    Failed,
    Cancelled,
    /// Catch-all for unknown statuses added by the OpenResponses spec.
    #[serde(other)]
    Unknown,
}

/// OpenResponses API response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenResponsesResponse {
    id: String,
    object: String, // "response"
    #[serde(default)]
    status: ResponseStatus,
    output: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<ResponseUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResponseError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    incomplete_details: Option<IncompleteDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
}

/// Response usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseUsage {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens_details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens_details: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_user_message_serialization() {
        let msg = UserMessageItemParam::new("Hello, world!");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "message");
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"][0]["type"], "input_text");
        assert_eq!(json["content"][0]["text"], "Hello, world!");
    }

    #[test]
    fn test_function_tool_param() {
        let tool = FunctionToolParam::new("get_weather")
            .with_description("Get the current weather")
            .with_parameters(json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                }
            }));

        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "function");
        assert_eq!(json["name"], "get_weather");
        assert!(json["description"].is_string());
    }

    #[test]
    fn test_function_call_output() {
        let output = FunctionCallOutputItemParam::new("call_123", r#"{"temperature": 72}"#);
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["type"], "function_call_output");
        assert_eq!(json["call_id"], "call_123");
    }
}
