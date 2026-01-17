#![allow(clippy::result_large_err)]
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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::pin::Pin;

use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};

/// Universal LLM request structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LLMRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,

    /// Optional structured output JSON schema to request from providers that support it
    /// For Anthropic this will be sent as `output_format: { type: "json_schema", schema: ... }`
    pub output_format: Option<Value>,

    /// Tool choice configuration based on official API docs
    /// Supports: "auto" (default), "none", "any", or specific tool selection
    pub tool_choice: Option<ToolChoice>,

    /// Whether to enable parallel tool calls (OpenAI specific)
    pub parallel_tool_calls: Option<bool>,

    /// Parallel tool use configuration following Anthropic best practices
    pub parallel_tool_config: Option<ParallelToolConfig>,

    /// Reasoning effort level for models that support it (none, low, medium, high)
    /// Applies to: Claude, GPT-5, GPT-5.1, Gemini, Qwen3, DeepSeek with reasoning capability
    pub reasoning_effort: Option<ReasoningEffortLevel>,

    /// Verbosity level for output text (low, medium, high)
    /// Applies to: GPT-5.1 and other models that support verbosity control
    pub verbosity: Option<VerbosityLevel>,

    /// Advanced generation parameters
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
}

/// Tool choice configuration that works across different providers
/// Based on OpenAI, Anthropic, and Gemini API specifications
/// Follows Anthropic's tool use best practices for optimal performance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum ToolChoice {
    /// Let the model decide whether to call tools ("auto")
    /// Default behavior - allows model to use tools when appropriate
    #[default]
    Auto,

    /// Force the model to not call any tools ("none")
    /// Useful for pure conversational responses without tool usage
    None,

    /// Force the model to call at least one tool ("any")
    /// Ensures tool usage even when model might prefer direct response
    Any,

    /// Force the model to call a specific tool
    /// Useful for directing model to use particular functionality
    Specific(SpecificToolChoice),
}

/// Specific tool choice for forcing a particular function call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub tool_type: String, // "function"

    pub function: SpecificFunctionChoice,
}

/// Specific function choice details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunctionChoice {
    pub name: String,
}

impl ToolChoice {
    /// Create auto tool choice (default behavior)
    pub fn auto() -> Self {
        Self::Auto
    }

    /// Create none tool choice (disable tool calling)
    pub fn none() -> Self {
        Self::None
    }

    /// Create any tool choice (force at least one tool call)
    pub fn any() -> Self {
        Self::Any
    }

    /// Create specific function tool choice
    pub fn function(name: String) -> Self {
        Self::Specific(SpecificToolChoice {
            tool_type: "function".to_owned(),
            function: SpecificFunctionChoice { name },
        })
    }

    /// Check if this tool choice allows parallel tool use
    /// Based on Anthropic's parallel tool use guidelines
    pub fn allows_parallel_tools(&self) -> bool {
        match self {
            // Auto allows parallel tools by default
            Self::Auto => true,
            // Any forces at least one tool, may allow parallel
            Self::Any => true,
            // Specific forces one particular tool, typically no parallel
            Self::Specific(_) => false,
            // None disables tools entirely
            Self::None => false,
        }
    }

    /// Get human-readable description of tool choice behavior
    pub fn description(&self) -> &'static str {
        match self {
            Self::Auto => "Model decides when to use tools (allows parallel)",
            Self::None => "No tools will be used",
            Self::Any => "At least one tool must be used (allows parallel)",
            Self::Specific(_) => "Specific tool must be used (no parallel)",
        }
    }

    /// OpenAI-compatible providers that share the same tool_choice format
    const OPENAI_STYLE_PROVIDERS: &'static [&'static str] = &[
        "openai",
        "deepseek",
        "huggingface",
        "openrouter",
        "xai",
        "zai",
        "moonshot",
        "lmstudio",
    ];

    /// Convert to provider-specific format
    #[inline]
    pub fn to_provider_format(&self, provider: &str) -> Value {
        if Self::OPENAI_STYLE_PROVIDERS.contains(&provider) {
            return self.to_openai_format();
        }

        match provider {
            "anthropic" => self.to_anthropic_format(),
            "gemini" => self.to_gemini_format(),
            _ => self.to_openai_format(), // Default to OpenAI format
        }
    }

    #[inline]
    fn to_openai_format(&self) -> Value {
        match self {
            Self::Auto => json!("auto"),
            Self::None => json!("none"),
            Self::Any => json!("required"),
            Self::Specific(choice) => json!(choice),
        }
    }

    #[inline]
    fn to_anthropic_format(&self) -> Value {
        match self {
            Self::Auto => json!({"type": "auto"}),
            Self::None => json!({"type": "none"}),
            Self::Any => json!({"type": "any"}),
            Self::Specific(choice) => json!({"type": "tool", "name": &choice.function.name}),
        }
    }

    #[inline]
    fn to_gemini_format(&self) -> Value {
        match self {
            Self::Auto => json!({"mode": "auto"}),
            Self::None => json!({"mode": "none"}),
            Self::Any => json!({"mode": "any"}),
            Self::Specific(choice) => {
                json!({"mode": "any", "allowed_function_names": [&choice.function.name]})
            }
        }
    }
}

/// Configuration for parallel tool use behavior
/// Based on Anthropic's parallel tool use guidelines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelToolConfig {
    /// Whether to disable parallel tool use
    /// When true, forces sequential tool execution
    pub disable_parallel_tool_use: bool,

    /// Maximum number of tools to execute in parallel
    /// None means no limit (provider default)
    pub max_parallel_tools: Option<usize>,

    /// Whether to encourage parallel tool use in prompts
    pub encourage_parallel: bool,
}

impl Default for ParallelToolConfig {
    fn default() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: Some(5), // Reasonable default
            encourage_parallel: true,
        }
    }
}

impl ParallelToolConfig {
    /// Create configuration optimized for Anthropic models
    pub fn anthropic_optimized() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: None, // Let Anthropic decide
            encourage_parallel: true,
        }
    }

    /// Create configuration for sequential tool use
    pub fn sequential_only() -> Self {
        Self {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(1),
            encourage_parallel: false,
        }
    }
}

/// Content type for messages that can include both text and images
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        data: String,      // Base64 encoded image data
        mime_type: String, // MIME type (e.g., "image/png")
        #[serde(rename = "type")]
        content_type: String, // "image"
    },
}

impl ContentPart {
    pub fn text(text: String) -> Self {
        ContentPart::Text { text }
    }

    pub fn image(data: String, mime_type: String) -> Self {
        ContentPart::Image {
            data,
            mime_type,
            content_type: "image".to_owned(),
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentPart::Text { text } => Some(text),
            _ => None,
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, ContentPart::Image { .. })
    }
}

/// Universal message structure supporting both text and image content
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Message {
    #[serde(default)]
    pub role: MessageRole,
    /// Content can be a string (for backward compatibility) or an array of content parts
    #[serde(default)]
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Optional origin tool name for tracking which tool generated this message
    /// Used in tool-aware context retention to preserve results from recently-active tools
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_tool: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Legacy single text string
    Text(String),
    /// Multiple content parts (text and images)
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn text(text: String) -> Self {
        MessageContent::Text(text)
    }

    pub fn parts(parts: Vec<ContentPart>) -> Self {
        MessageContent::Parts(parts)
    }

    /// Returns a borrowed reference to the text content if this is a simple Text variant.
    /// For Parts variant, returns None (use as_text() for combined content).
    #[inline]
    pub fn as_text_borrowed(&self) -> Option<&str> {
        match self {
            MessageContent::Text(text) => Some(text.as_str()),
            MessageContent::Parts(_) => None,
        }
    }

    /// Returns the text content, avoiding allocation if possible.
    /// For Parts variant, joins all text parts with spaces.
    pub fn as_text(&self) -> std::borrow::Cow<'_, str> {
        match self {
            MessageContent::Text(text) => std::borrow::Cow::Borrowed(text),
            MessageContent::Parts(parts) => {
                // Optimize: Filter and collect text parts first
                let text_parts: Vec<&str> =
                    parts.iter().filter_map(|part| part.as_text()).collect();

                if text_parts.is_empty() {
                    return std::borrow::Cow::Borrowed("");
                }

                // Single part optimization - avoid allocation
                if text_parts.len() == 1 {
                    return std::borrow::Cow::Borrowed(text_parts[0]);
                }

                // Pre-calculate capacity to avoid reallocations
                let total_len = text_parts.iter().map(|s| s.len()).sum::<usize>()
                    + text_parts.len().saturating_sub(1); // spaces between parts
                let mut result = String::with_capacity(total_len);
                for (i, part) in text_parts.iter().enumerate() {
                    if i > 0 {
                        result.push(' ');
                    }
                    result.push_str(part);
                }
                std::borrow::Cow::Owned(result)
            }
        }
    }

    /// Returns trimmed text content. Avoids allocation when possible.
    pub fn trim(&self) -> std::borrow::Cow<'_, str> {
        match self {
            MessageContent::Text(text) => {
                let trimmed = text.trim();
                // Optimization: Only allocate if trim actually changed the string
                if trimmed.len() == text.len() {
                    std::borrow::Cow::Borrowed(text)
                } else {
                    std::borrow::Cow::Borrowed(trimmed)
                }
            }
            MessageContent::Parts(_) => {
                // For Parts, we need to get text first, then trim
                match self.as_text() {
                    std::borrow::Cow::Borrowed(s) => std::borrow::Cow::Borrowed(s.trim()),
                    std::borrow::Cow::Owned(s) => {
                        let trimmed = s.trim();
                        if trimmed.len() == s.len() {
                            std::borrow::Cow::Owned(s)
                        } else {
                            std::borrow::Cow::Owned(trimmed.to_owned())
                        }
                    }
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            MessageContent::Text(text) => text.is_empty(),
            MessageContent::Parts(parts) => {
                parts.is_empty()
                    || parts.iter().all(|part| match part {
                        ContentPart::Text { text } => text.is_empty(),
                        _ => false,
                    })
            }
        }
    }

    pub fn has_images(&self) -> bool {
        match self {
            MessageContent::Text(_) => false,
            MessageContent::Parts(parts) => parts.iter().any(|part| part.is_image()),
        }
    }

    pub fn get_images(&self) -> Vec<&ContentPart> {
        match self {
            MessageContent::Text(_) => vec![],
            MessageContent::Parts(parts) => parts.iter().filter(|part| part.is_image()).collect(),
        }
    }
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
}

impl From<String> for MessageContent {
    fn from(value: String) -> Self {
        MessageContent::Text(value)
    }
}

impl From<&str> for MessageContent {
    fn from(value: &str) -> Self {
        MessageContent::Text(value.to_owned())
    }
}

impl Message {
    /// Estimate the number of tokens in this message (rough approximation).
    pub fn estimate_tokens(&self) -> usize {
        let mut count = 0;

        // Role overhead (approximate)
        count += 4;

        // Content tokens
        match &self.content {
            MessageContent::Text(text) => count += crate::llm::utils::estimate_token_count(text),
            MessageContent::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentPart::Text { text } => {
                            count += crate::llm::utils::estimate_token_count(text)
                        }
                        ContentPart::Image { .. } => count += 1000, // Rough estimate for images
                    }
                }
            }
        }

        // Tool calls tokens
        if let Some(tool_calls) = &self.tool_calls {
            for call in tool_calls {
                count += 20; // Base overhead per call
                if let Some(func) = &call.function {
                    count += crate::llm::utils::estimate_token_count(&func.name);
                    count += crate::llm::utils::estimate_token_count(&func.arguments);
                }
                if let Some(sig) = &call.thought_signature {
                    count += crate::llm::utils::estimate_token_count(sig);
                }
            }
        }

        // Tool call ID (for responses)
        if let Some(id) = &self.tool_call_id {
            count += crate::llm::utils::estimate_token_count(id);
        }

        count
    }

    /// Helper to create a base message with common defaults.
    /// Public for use in provider implementations.
    #[inline]
    pub const fn base(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            origin_tool: None,
        }
    }

    /// Create a user message with text content
    #[inline]
    pub fn user(content: String) -> Self {
        Self::base(MessageRole::User, MessageContent::Text(content))
    }

    /// Create a user message with multiple content parts (text and images)
    #[inline]
    pub fn user_with_parts(content_parts: Vec<ContentPart>) -> Self {
        Self::base(MessageRole::User, MessageContent::Parts(content_parts))
    }

    /// Create an assistant message with text content
    #[inline]
    pub fn assistant(content: String) -> Self {
        Self::base(MessageRole::Assistant, MessageContent::Text(content))
    }

    /// Create an assistant message with multiple content parts
    #[inline]
    pub fn assistant_with_parts(content_parts: Vec<ContentPart>) -> Self {
        Self::base(MessageRole::Assistant, MessageContent::Parts(content_parts))
    }

    /// Create an assistant message with tool calls
    /// Based on OpenAI Cookbook patterns for function calling
    #[inline]
    pub fn assistant_with_tools(content: String, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls: Some(tool_calls),
            ..Self::base(MessageRole::Assistant, MessageContent::Text(content))
        }
    }

    /// Create an assistant message with tool calls and multiple content parts
    #[inline]
    pub fn assistant_with_tools_and_parts(
        content_parts: Vec<ContentPart>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            tool_calls: Some(tool_calls),
            ..Self::base(MessageRole::Assistant, MessageContent::Parts(content_parts))
        }
    }

    /// Create an assistant message with tool calls and reasoning details
    /// Used for preserving reasoning state in multi-turn conversations
    #[inline]
    pub fn assistant_with_tools_and_reasoning(
        content: String,
        tool_calls: Vec<ToolCall>,
        reasoning_details: Option<Vec<serde_json::Value>>,
    ) -> Self {
        Self {
            tool_calls: Some(tool_calls),
            reasoning_details,
            ..Self::base(MessageRole::Assistant, MessageContent::Text(content))
        }
    }

    /// Create a system message
    #[inline]
    pub fn system(content: String) -> Self {
        Self::base(MessageRole::System, MessageContent::Text(content))
    }

    /// Create a tool response message
    /// This follows the exact pattern from OpenAI Cookbook:
    /// ```json
    /// {
    ///   "role": "tool",
    ///   "tool_call_id": "call_123",
    ///   "content": "Function result"
    /// }
    /// ```
    #[inline]
    pub fn tool_response(tool_call_id: String, content: String) -> Self {
        Self {
            tool_call_id: Some(tool_call_id),
            ..Self::base(MessageRole::Tool, MessageContent::Text(content))
        }
    }

    /// Create a tool response message with function name (for compatibility)
    /// Some providers might need the function name in addition to tool_call_id
    #[inline]
    pub fn tool_response_with_name(
        tool_call_id: String,
        _function_name: String,
        content: String,
    ) -> Self {
        // We can store the function name in the content metadata or handle it provider-specifically
        Self::tool_response(tool_call_id, content)
    }

    /// Create a tool response message with origin tool tracking
    /// The origin_tool field helps with tool-aware context retention
    #[inline]
    pub fn tool_response_with_origin(
        tool_call_id: String,
        content: String,
        origin_tool: String,
    ) -> Self {
        Self {
            tool_call_id: Some(tool_call_id),
            origin_tool: Some(origin_tool),
            ..Self::base(MessageRole::Tool, MessageContent::Text(content))
        }
    }

    /// Create a user message with image from a local file
    pub async fn user_with_local_image<P: AsRef<std::path::Path>>(
        file_path: P,
    ) -> Result<Self, anyhow::Error> {
        let image_data = crate::utils::image_processing::read_image_file(file_path).await?;
        let image_part = ContentPart::image(image_data.base64_data, image_data.mime_type);
        Ok(Self::user_with_parts(vec![image_part]))
    }

    /// Create a user message with text and a local image
    pub async fn user_with_text_and_local_image<P: AsRef<std::path::Path>>(
        text: String,
        file_path: P,
    ) -> Result<Self, anyhow::Error> {
        let image_data = crate::utils::image_processing::read_image_file(file_path).await?;
        let text_part = ContentPart::text(text);
        let image_part = ContentPart::image(image_data.base64_data, image_data.mime_type);
        Ok(Self::user_with_parts(vec![text_part, image_part]))
    }

    /// Attach provider-visible reasoning trace for archival without affecting payloads.
    pub fn with_reasoning(mut self, reasoning: Option<String>) -> Self {
        self.reasoning = reasoning;
        self
    }

    /// Attach reasoning details for providers that support structured reasoning
    pub fn with_reasoning_details(
        mut self,
        reasoning_details: Option<Vec<serde_json::Value>>,
    ) -> Self {
        self.reasoning_details = reasoning_details;
        self
    }

    /// Validate this message for a specific provider
    /// Based on official API documentation constraints
    pub fn validate_for_provider(&self, provider: &str) -> Result<(), String> {
        // Check role-specific constraints
        self.role
            .validate_for_provider(provider, self.tool_call_id.is_some())?;

        // Check tool call constraints
        if let Some(tool_calls) = &self.tool_calls {
            if !self.role.can_make_tool_calls() {
                return Err(format!("Role {:?} cannot make tool calls", self.role));
            }

            if tool_calls.is_empty() {
                return Err("Tool calls array should not be empty".to_owned());
            }

            // Validate each tool call
            for tool_call in tool_calls {
                tool_call.validate()?;
            }
        }

        // Provider-specific validations based on official docs
        match provider {
            "openai" | "openrouter" | "zai" => {
                if self.role == MessageRole::Tool && self.tool_call_id.is_none() {
                    return Err(format!(
                        "{} requires tool_call_id for tool messages",
                        provider
                    ));
                }
            }
            "gemini" => {
                if self.role == MessageRole::Tool && self.tool_call_id.is_none() {
                    return Err(
                        "Gemini tool responses need tool_call_id for function name mapping"
                            .to_owned(),
                    );
                }
                // Gemini has additional constraints on content structure
                if self.role == MessageRole::System && !self.content.as_text().is_empty() {
                    // System messages should be handled as systemInstruction, not in contents
                }
            }
            "anthropic" => {
                // Anthropic is more flexible with tool message format
                // Tool messages are converted to user messages anyway
            }
            _ => {} // Generic validation already done above
        }

        Ok(())
    }

    /// Check if this message has tool calls
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .is_some_and(|calls| !calls.is_empty())
    }

    /// Get the tool calls if present
    pub fn get_tool_calls(&self) -> Option<&[ToolCall]> {
        self.tool_calls.as_deref()
    }

    /// Check if this is a tool response message
    pub fn is_tool_response(&self) -> bool {
        self.role == MessageRole::Tool
    }

    /// Get the text content of the message (for backward compatibility)
    pub fn get_text_content(&self) -> std::borrow::Cow<'_, str> {
        self.content.as_text()
    }

    /// Check if this message contains images
    pub fn has_images(&self) -> bool {
        self.content.has_images()
    }

    /// Get all images in this message
    pub fn get_images(&self) -> Vec<&ContentPart> {
        self.content.get_images()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MessageRole {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::System => write!(f, "system"),
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
            MessageRole::Tool => write!(f, "tool"),
        }
    }
}

impl MessageRole {
    /// Get the role string for Gemini API
    /// Note: Gemini API has specific constraints on message roles
    /// - Only accepts "user" and "model" roles in conversations
    /// - System messages are handled separately as system instructions
    /// - Tool responses are sent as "user" role with function response format
    pub fn as_gemini_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system", // Handled as systemInstruction, not in contents
            MessageRole::User => "user",
            MessageRole::Assistant => "model", // Gemini uses "model" instead of "assistant"
            MessageRole::Tool => "user", // Tool responses are sent as user messages with functionResponse
        }
    }

    /// Get the role string for OpenAI API
    /// OpenAI supports all standard role types including:
    /// - system, user, assistant, tool
    /// - function (legacy, now replaced by tool)
    pub fn as_openai_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool", // Full support for tool role with tool_call_id
        }
    }

    /// Get the role string for Anthropic API
    /// Anthropic has specific handling for tool messages:
    /// - Supports user, assistant roles normally
    /// - Tool responses are treated as user messages
    /// - System messages can be handled as system parameter or hoisted
    pub fn as_anthropic_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system", // Can be hoisted to system parameter
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "user", // Anthropic treats tool responses as user messages
        }
    }

    /// Get the role string for generic OpenAI-compatible providers
    /// Most providers follow OpenAI's role conventions
    pub fn as_generic_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        }
    }

    /// Check if this role supports tool calls
    /// Only Assistant role can initiate tool calls in most APIs
    pub fn can_make_tool_calls(&self) -> bool {
        matches!(self, MessageRole::Assistant)
    }

    /// Check if this role represents a tool response
    pub fn is_tool_response(&self) -> bool {
        matches!(self, MessageRole::Tool)
    }

    /// Validate message role constraints for a given provider
    /// Based on official API documentation requirements
    pub fn validate_for_provider(
        &self,
        provider: &str,
        has_tool_call_id: bool,
    ) -> Result<(), String> {
        match (self, provider) {
            (MessageRole::Tool, provider)
                if matches!(
                    provider,
                    "openai" | "openrouter" | "xai" | "deepseek" | "zai"
                ) && !has_tool_call_id =>
            {
                Err(format!("{} tool messages must have tool_call_id", provider))
            }
            (MessageRole::Tool, "gemini") if !has_tool_call_id => {
                Err("Gemini tool messages need tool_call_id for function mapping".to_owned())
            }
            _ => Ok(()),
        }
    }
}

/// Tool search algorithm for Anthropic's advanced-tool-use beta
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolSearchAlgorithm {
    /// Regex-based search using Python re.search() syntax
    #[default]
    Regex,
    /// BM25-based natural language search
    Bm25,
}

impl std::fmt::Display for ToolSearchAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regex => write!(f, "regex"),
            Self::Bm25 => write!(f, "bm25"),
        }
    }
}

impl std::str::FromStr for ToolSearchAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "regex" => Ok(Self::Regex),
            "bm25" => Ok(Self::Bm25),
            _ => Err(format!("Unknown tool search algorithm: {}", s)),
        }
    }
}

/// Universal tool definition that matches OpenAI/Anthropic/Gemini specifications
/// Based on official API documentation from Context7
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// The type of tool: "function", "apply_patch" (GPT-5.1), "shell" (GPT-5.1), or "custom" (GPT-5 freeform)
    /// Also supports Anthropic tool search types: "tool_search_tool_regex_20251119", "tool_search_tool_bm25_20251119"
    #[serde(rename = "type")]
    pub tool_type: String,

    /// Function definition containing name, description, and parameters
    /// Used for "function", "apply_patch", and "custom" types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionDefinition>,

    /// Shell tool configuration (GPT-5.1 specific)
    /// Describes shell command capabilities and constraints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<ShellToolDefinition>,

    /// Grammar definition for context-free grammar constraints (GPT-5 specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<GrammarDefinition>,

    /// When true and using Anthropic, mark the tool as strict for structured tool use validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,

    /// When true, the tool is deferred and only loaded when discovered via tool search (Anthropic advanced-tool-use beta)
    /// This enables dynamic tool discovery for large tool catalogs (10k+ tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
}

/// Shell tool definition for GPT-5.1 shell tool type
/// Allows controlled command-line interface interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellToolDefinition {
    /// Description of shell tool capabilities
    pub description: String,

    /// List of allowed commands (whitelist for safety)
    pub allowed_commands: Vec<String>,

    /// List of forbidden commands (blacklist for safety)
    pub forbidden_patterns: Vec<String>,

    /// Maximum command timeout in seconds
    pub timeout_seconds: u32,
}

/// Grammar definition for GPT-5 context-free grammar (CFG) constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarDefinition {
    /// The syntax of the grammar: "lark" or "regex"
    pub syntax: String,

    /// The grammar definition in the specified syntax
    pub definition: String,
}

impl Default for GrammarDefinition {
    fn default() -> Self {
        Self {
            syntax: "lark".into(),
            definition: String::new(),
        }
    }
}

impl Default for ShellToolDefinition {
    fn default() -> Self {
        Self {
            description: "Execute shell commands in the workspace".into(),
            allowed_commands: vec![
                "ls".into(),
                "find".into(),
                "grep".into(),
                "cargo".into(),
                "git".into(),
                "python".into(),
                "node".into(),
            ],
            forbidden_patterns: vec!["rm -rf".into(), "sudo".into(), "passwd".into()],
            timeout_seconds: 30,
        }
    }
}

/// Function definition within a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// The name of the function to be called
    pub name: String,

    /// A description of what the function does
    pub description: String,

    /// The parameters the function accepts, described as a JSON Schema object
    pub parameters: Value,
}

fn sanitize_tool_description(description: &str) -> String {
    let mut result = String::with_capacity(description.len());
    let mut first = true;
    for line in description.lines() {
        if !first {
            result.push('\n');
        }
        result.push_str(line.trim_end());
        first = false;
    }
    result.trim().to_owned()
}

impl ToolDefinition {
    /// Create a new tool definition with function type
    pub fn function(name: String, description: String, parameters: Value) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "function".to_owned(),
            function: Some(FunctionDefinition {
                name,
                description: sanitized_description,
                parameters,
            }),
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Set whether the tool should be considered strict (Anthropic structured tool use)
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    /// Set whether the tool should be deferred (Anthropic tool search)
    pub fn with_defer_loading(mut self, defer: bool) -> Self {
        self.defer_loading = Some(defer);
        self
    }

    /// Create a tool search tool definition for Anthropic's advanced-tool-use beta
    /// Supports regex and bm25 search algorithms
    pub fn tool_search(algorithm: ToolSearchAlgorithm) -> Self {
        let (tool_type, name) = match algorithm {
            ToolSearchAlgorithm::Regex => (
                "tool_search_tool_regex_20251119",
                "tool_search_tool_regex",
            ),
            ToolSearchAlgorithm::Bm25 => ("tool_search_tool_bm25_20251119", "tool_search_tool_bm25"),
        };

        Self {
            tool_type: tool_type.to_owned(),
            function: Some(FunctionDefinition {
                name: name.to_owned(),
                description: "Search for tools by name, description, or parameters".to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (regex pattern for regex variant, natural language for bm25)"
                        }
                    },
                    "required": ["query"]
                }),
            }),
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new apply_patch tool definition (GPT-5.1 specific)
    /// The apply_patch tool lets models create, update, and delete files using structured diffs
    pub fn apply_patch(description: String) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "apply_patch".to_owned(),
            function: Some(FunctionDefinition {
                name: "apply_patch".to_owned(),
                description: sanitized_description,
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The absolute path to the file to modify"
                        },
                        "patch": {
                            "type": "string",
                            "description": "Unified diff format patch to apply"
                        }
                    },
                    "required": ["file_path", "patch"]
                }),
            }),
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new custom tool definition for freeform function calling (GPT-5 specific)
    /// Allows raw text payloads without JSON wrapping
    pub fn custom(name: String, description: String) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "custom".to_owned(),
            function: Some(FunctionDefinition {
                name,
                description: sanitized_description,
                parameters: json!({}), // Custom tools may not need parameters
            }),
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new grammar tool definition for context-free grammar constraints (GPT-5 specific)
    /// Ensures model output matches predefined syntax
    pub fn grammar(syntax: String, definition: String) -> Self {
        Self {
            tool_type: "grammar".to_owned(),
            function: None,
            shell: None,
            grammar: Some(GrammarDefinition { syntax, definition }),
            strict: None,
            defer_loading: None,
        }
    }

    /// Get the function name for easy access
    pub fn function_name(&self) -> &str {
        if let Some(func) = &self.function {
            &func.name
        } else {
            &self.tool_type
        }
    }

    /// Get the description for easy access
    pub fn description(&self) -> &str {
        if let Some(func) = &self.function {
            &func.description
        } else if let Some(shell) = &self.shell {
            &shell.description
        } else {
            ""
        }
    }

    /// Validate that this tool definition is properly formed
    pub fn validate(&self) -> Result<(), String> {
        match self.tool_type.as_str() {
            "function" => self.validate_function(),
            "apply_patch" => self.validate_apply_patch(),
            "shell" => self.validate_shell(),
            "custom" => self.validate_custom(),
            "grammar" => self.validate_grammar(),
            "tool_search_tool_regex_20251119" | "tool_search_tool_bm25_20251119" => {
                self.validate_function()
            }
            other => Err(format!(
                "Unsupported tool type: {}. Supported types: function, apply_patch, shell, custom, grammar, tool_search_tool_*",
                other
            )),
        }
    }

    /// Returns true if this is a tool search tool type
    pub fn is_tool_search(&self) -> bool {
        matches!(
            self.tool_type.as_str(),
            "tool_search_tool_regex_20251119" | "tool_search_tool_bm25_20251119"
        )
    }

    fn validate_function(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name.is_empty() {
                return Err("Function name cannot be empty".to_owned());
            }
            if func.description.is_empty() {
                return Err("Function description cannot be empty".to_owned());
            }
            if !func.parameters.is_object() {
                return Err("Function parameters must be a JSON object".to_owned());
            }
            Ok(())
        } else {
            Err("Function tool missing function definition".to_owned())
        }
    }

    fn validate_apply_patch(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name != "apply_patch" {
                return Err(format!(
                    "apply_patch tool must have name 'apply_patch', got: {}",
                    func.name
                ));
            }
            if func.description.is_empty() {
                return Err("apply_patch description cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("apply_patch tool missing function definition".to_owned())
        }
    }

    fn validate_shell(&self) -> Result<(), String> {
        if let Some(shell) = &self.shell {
            if shell.description.is_empty() {
                return Err("Shell tool description cannot be empty".to_owned());
            }
            if shell.timeout_seconds == 0 {
                return Err("Shell tool timeout must be greater than 0".to_owned());
            }
            Ok(())
        } else {
            Err("Shell tool missing shell definition".to_owned())
        }
    }

    fn validate_custom(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name.is_empty() {
                return Err("Custom tool name cannot be empty".to_owned());
            }
            if func.description.is_empty() {
                return Err("Custom tool description cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("Custom tool missing function definition".to_owned())
        }
    }

    fn validate_grammar(&self) -> Result<(), String> {
        if let Some(grammar) = &self.grammar {
            if !["lark", "regex"].contains(&grammar.syntax.as_str()) {
                return Err("Grammar syntax must be 'lark' or 'regex'".to_owned());
            }
            if grammar.definition.is_empty() {
                return Err("Grammar definition cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("Grammar tool missing grammar definition".to_owned())
        }
    }
}

/// Universal tool call that matches OpenAI/Anthropic/Gemini specifications
/// Based on official API documentation from Context7
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call (e.g., "call_123")
    pub id: String,

    /// The type of tool call: "function", "custom" (GPT-5 freeform), or other
    #[serde(rename = "type")]
    pub call_type: String,

    /// Function call details (for function-type tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCall>,

    /// Raw text payload (for custom freeform tools in GPT-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Gemini-specific thought signature for maintaining reasoning context
    /// This encrypted string represents the model's internal reasoning state
    /// and must be preserved and sent back exactly as received for proper context continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// Function call within a tool call
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    /// The name of the function to call
    pub name: String,

    /// The arguments to pass to the function, as a JSON string
    pub arguments: String,
}

impl ToolCall {
    /// Create a new function tool call
    pub fn function(id: String, name: String, arguments: String) -> Self {
        Self {
            id,
            call_type: "function".to_owned(),
            function: Some(FunctionCall { name, arguments }),
            text: None,
            thought_signature: None,
        }
    }

    /// Create a new custom tool call with raw text payload (GPT-5 freeform)
    pub fn custom(id: String, name: String, text: String) -> Self {
        Self {
            id,
            call_type: "custom".to_owned(),
            function: Some(FunctionCall {
                name,
                arguments: text.clone(), // For custom tools, we treat the text as arguments
            }),
            text: Some(text),
            thought_signature: None,
        }
    }

    /// Parse the arguments as JSON Value (for function-type tools)
    pub fn parsed_arguments(&self) -> Result<Value, serde_json::Error> {
        if let Some(ref func) = self.function {
            serde_json::from_str(&func.arguments)
        } else {
            // Return an error by trying to parse invalid JSON
            serde_json::from_str("")
        }
    }

    /// Validate that this tool call is properly formed
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Tool call ID cannot be empty".to_owned());
        }

        match self.call_type.as_str() {
            "function" => {
                if let Some(func) = &self.function {
                    if func.name.is_empty() {
                        return Err("Function name cannot be empty".to_owned());
                    }
                    // Validate that arguments is valid JSON for function tools
                    if let Err(e) = self.parsed_arguments() {
                        return Err(format!("Invalid JSON in function arguments: {}", e));
                    }
                } else {
                    return Err("Function tool call missing function details".to_owned());
                }
            }
            "custom" => {
                // For custom tools, we allow raw text payload without JSON validation
                if let Some(func) = &self.function {
                    if func.name.is_empty() {
                        return Err("Custom tool name cannot be empty".to_owned());
                    }
                } else {
                    return Err("Custom tool call missing function details".to_owned());
                }
            }
            _ => return Err(format!("Unsupported tool call type: {}", self.call_type)),
        }

        Ok(())
    }
}

/// Universal LLM response
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReason,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    /// Tool references discovered via Anthropic's tool search feature
    /// These tool names should be expanded (defer_loading=false) in the next request
    pub tool_references: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum LLMStreamEvent {
    Token { delta: String },
    Reasoning { delta: String },
    Completed { response: LLMResponse },
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
    pub retry_after: Option<String>,
    pub message: Option<String>,
}

impl LLMErrorMetadata {
    pub fn new(
        provider: &'static str,
        status: Option<u16>,
        code: Option<String>,
        request_id: Option<String>,
        retry_after: Option<String>,
        message: Option<String>,
    ) -> Self {
        Self {
            provider,
            status,
            code,
            request_id,
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

// Implement conversion from provider::LLMError to llm::types::LLMError
impl From<LLMError> for crate::llm::types::LLMError {
    fn from(err: LLMError) -> crate::llm::types::LLMError {
        let convert = |meta: Option<LLMErrorMetadata>| {
            meta.map(|m| crate::llm::types::LLMErrorMetadata {
                provider: Some(m.provider.to_string()),
                status: m.status,
                code: m.code,
                request_id: m.request_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitize_tool_description_trims_padding() {
        let input = "\n\nLine 1\nLine 2 \n";
        assert_eq!(sanitize_tool_description(input), "Line 1\nLine 2");
    }

    #[test]
    fn sanitize_tool_description_preserves_internal_blank_lines() {
        let input = "Line 1\n\nLine 3";
        assert_eq!(sanitize_tool_description(input), input);
    }

    #[test]
    fn tool_definition_function_uses_sanitized_description() {
        let tool = ToolDefinition::function(
            "demo".to_owned(),
            "  Line 1  \n".to_owned(),
            json!({"type": "object", "properties": {}}),
        );
        assert_eq!(tool.function.as_ref().unwrap().description, "Line 1");
    }
}
