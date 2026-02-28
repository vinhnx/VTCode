use super::ToolCall;
use crate::llm::providers::clean_reasoning_text;
use serde::{Deserialize, Serialize};

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
    File {
        #[serde(rename = "type")]
        content_type: String, // "file" or "input_file"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_data: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
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

    pub fn file_from_id(file_id: String) -> Self {
        ContentPart::File {
            content_type: "file".to_owned(),
            filename: None,
            file_id: Some(file_id),
            file_data: None,
            file_url: None,
        }
    }

    pub fn file_from_url(file_url: String) -> Self {
        ContentPart::File {
            content_type: "input_file".to_owned(),
            filename: None,
            file_id: None,
            file_data: None,
            file_url: Some(file_url),
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

    pub fn is_file(&self) -> bool {
        matches!(self, ContentPart::File { .. })
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
    /// For Parts variant, concatenates text parts in order without adding spacing.
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
                let total_len = text_parts.iter().map(|s| s.len()).sum::<usize>();
                let mut result = String::with_capacity(total_len);
                for part in text_parts {
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
                        ContentPart::Image { .. } | ContentPart::File { .. } => false,
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
                        ContentPart::Image { .. } | ContentPart::File { .. } => count += 1000, // Rough estimate for images/files
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
        if self.role == MessageRole::Assistant
            && let Some(reasoning_text) = reasoning.as_ref()
        {
            let cleaned_reasoning = clean_reasoning_text(reasoning_text);
            if !cleaned_reasoning.is_empty() {
                let cleaned_content = clean_reasoning_text(self.content.as_text().as_ref());
                if !cleaned_content.is_empty() && cleaned_reasoning == cleaned_content {
                    self.reasoning = None;
                    return self;
                }
            }
        }
        self.reasoning = reasoning;
        self
    }

    /// Attach tool calls to this message.
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = Some(tool_calls);
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
                if matches!(provider, "openai" | "openrouter" | "deepseek" | "zai")
                    && !has_tool_call_id =>
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

#[cfg(test)]
mod tests {
    use super::{ContentPart, MessageContent};

    #[test]
    fn message_content_parts_concatenate_without_extra_spaces() {
        let parts = vec![
            ContentPart::text("Andre".to_string()),
            ContentPart::text("j".to_string()),
            ContentPart::text(" Kar".to_string()),
            ContentPart::text("pathy".to_string()),
            ContentPart::text("'s".to_string()),
        ];
        let content = MessageContent::Parts(parts);

        assert_eq!(content.as_text().as_ref() as &str, "Andrej Karpathy's");
    }
}
