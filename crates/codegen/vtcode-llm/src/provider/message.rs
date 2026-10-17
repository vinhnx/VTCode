use super::ToolCall;
use crate::providers::clean_reasoning_text;
use serde::{Deserialize, Serialize};
use vtcode_commons::message_metadata::MessageMetadata;

/// Phase metadata for assistant messages in multi-step Responses-style workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Commentary,
    FinalAnswer,
}

/// Controls when an Anthropic mid-conversation system message is cleared.
///
/// This is intentionally a typed message property instead of a provider-wide
/// request flag so the message remains in persisted history and can be replayed
/// verbatim on the next request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageClearAt {
    NextUserMessage,
}

impl AssistantPhase {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Commentary => "commentary",
            Self::FinalAnswer => "final_answer",
        }
    }

    #[must_use]
    pub(crate) fn from_wire_str(value: &str) -> Option<Self> {
        match value {
            "commentary" => Some(Self::Commentary),
            "final_answer" => Some(Self::FinalAnswer),
            _ => None,
        }
    }
}

/// Detail level for image processing (DeepSeek/OpenAI `detail` field).
///
/// `Original` is retained for Gemini compatibility but not used for DeepSeek/OpenAI chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "auto")]
    Auto,
}

impl ImageDetail {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
            Self::Original => "original",
            Self::Auto => "auto",
        }
    }

    #[allow(
        clippy::should_implement_trait,
        reason = "preserve the public compatibility parser API"
    )]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            "original" => Some(Self::Original),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Strictly typed image source — replaces bare `data`/`mime_type`/`image_url` triple.
///
/// This makes the mutual exclusivity explicit (shape-suffix naming) and provides
/// a single dispatch point for serialization. The underlying `ContentPart::Image`
/// fields are kept for serde backward compat, but new code should use this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource<'a> {
    Base64 { data: &'a str, mime_type: &'a str },
    Url { url: &'a str },
}

/// Content type for messages that can include both text and images
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        data: String,      // Base64 encoded image data (empty when `image_url` is used)
        mime_type: String, // MIME type (e.g., "image/png")
        #[serde(rename = "type")]
        content_type: String, // "image"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>, // DeepSeek/OpenAI detail: low|high|original|auto
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image_url: Option<String>, // External https URL alternative to base64 data
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
            detail: None,
            image_url: None,
        }
    }

    pub fn image_with_detail(data: String, mime_type: String, detail: ImageDetail) -> Self {
        ContentPart::Image {
            data,
            mime_type,
            content_type: "image".to_owned(),
            detail: Some(detail),
            image_url: None,
        }
    }

    /// Create an image part from an external URL.
    ///
    /// DeepSeek external URLs must be `https://`, ≤8192 chars, and ≤32MiB file.
    /// Returns `Err` for malformed URLs (fail-closed) so callers must handle it.
    pub fn image_from_url(url: String, detail: Option<ImageDetail>) -> Result<Self, String> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err("image URL must not be empty".to_owned());
        }
        if trimmed.len() > 8192 {
            return Err(format!("image URL exceeds 8192 char limit (len={})", trimmed.len()));
        }
        if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
            return Err(format!("image URL must be https://, got {trimmed:?}"));
        }
        Ok(ContentPart::Image {
            data: String::new(),
            mime_type: String::new(),
            content_type: "image".to_owned(),
            detail,
            image_url: Some(url),
        })
    }

    /// Strictly typed view of the image source (Base64 vs URL).
    ///
    /// This isolates the `data`/`mime_type`/`image_url` triple behind a single
    /// dispatch point (KISS/DRY guard rail for the next generation phase).
    pub fn image_source(&self) -> Option<ImageSource<'_>> {
        match self {
            ContentPart::Image { data, mime_type, image_url, .. } => {
                if let Some(url) = image_url {
                    Some(ImageSource::Url { url })
                } else if !data.is_empty() && !mime_type.is_empty() {
                    Some(ImageSource::Base64 { data, mime_type })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Validate image part invariants (MIME allowlist, size, URL limits).
    ///
    /// DeepSeek supports JPEG/PNG/GIF/WebP; other types are warned but not rejected
    /// to keep the interface open for future providers.
    pub fn validate_image(&self) -> Result<(), String> {
        match self {
            ContentPart::Image { data, mime_type, image_url, .. } => {
                if let Some(url) = image_url {
                    if url.len() > 8192 {
                        return Err(format!("image URL exceeds 8192 chars: {}", url.len()));
                    }
                    if !(url.starts_with("https://") || url.starts_with("http://")) {
                        return Err(format!("image URL must be https://: {url:?}"));
                    }
                } else {
                    if data.is_empty() || mime_type.is_empty() {
                        return Err("image data and mime_type must be non-empty for base64 images".to_owned());
                    }
                    const ALLOWED: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];
                    if !ALLOWED.contains(&mime_type.as_str()) {
                        tracing::warn!(mime_type = %mime_type, "image MIME type not in DeepSeek allowlist");
                    }
                    // Rough size check: base64 string length * 3/4 ≈ decoded bytes
                    let decoded_approx = data.len() * 3 / 4;
                    if decoded_approx > 32 * 1024 * 1024 {
                        return Err(format!("image exceeds 32MiB limit: ~{} bytes", decoded_approx));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Ergonomic helper for string-based detail (parses case-insensitively).
    /// Returns `None` and warns if `detail_str` is invalid.
    pub fn image_with_detail_str(data: String, mime_type: String, detail_str: &str) -> Option<Self> {
        match ImageDetail::from_str(detail_str) {
            Some(d) => Some(Self::image_with_detail(data, mime_type, d)),
            None => {
                tracing::warn!(detail = %detail_str, "invalid image detail, expected low|high|original|auto");
                None
            }
        }
    }

    pub(crate) fn file_from_id(file_id: String) -> Self {
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

    pub fn file_from_data(filename: String, file_data: String) -> Self {
        ContentPart::File {
            content_type: "input_file".to_owned(),
            filename: Some(filename),
            file_id: None,
            file_data: Some(file_data),
            file_url: None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentPart::Text { text } => Some(text),
            _ => None,
        }
    }

    pub(crate) fn is_image(&self) -> bool {
        matches!(self, ContentPart::Image { .. })
    }

    pub(crate) fn is_file(&self) -> bool {
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
    /// Optional assistant-only phase metadata used by OpenAI Responses workflows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<AssistantPhase>,
    /// Optional origin tool name for tracking which tool generated this message
    /// Used in tool-aware context retention to preserve results from recently-active tools
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_tool: Option<String>,
    /// Optional per-message metadata (timestamp, importance, compression status, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
    /// Optional provider-specific clear scope for a mid-conversation system message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_at: Option<MessageClearAt>,
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
                let mut first_text = None;
                let mut text_count = 0usize;
                let mut total_len = 0usize;

                for text in parts.iter().filter_map(ContentPart::as_text) {
                    if first_text.is_none() {
                        first_text = Some(text);
                    }
                    text_count += 1;
                    total_len += text.len();
                }

                if text_count == 0 {
                    return std::borrow::Cow::Borrowed("");
                }
                if text_count == 1 {
                    return std::borrow::Cow::Borrowed(first_text.unwrap_or(""));
                }

                let mut result = String::with_capacity(total_len);
                for text in parts.iter().filter_map(ContentPart::as_text) {
                    result.push_str(text);
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

    pub(crate) fn is_empty(&self) -> bool {
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

    pub(crate) fn has_images(&self) -> bool {
        match self {
            MessageContent::Text(_) => false,
            MessageContent::Parts(parts) => parts.iter().any(|part| part.is_image()),
        }
    }

    /// Returns content with images stripped, preserving text and file parts.
    /// Returns `None` if the content already contains no images.
    pub(crate) fn without_images(&self) -> Option<MessageContent> {
        match self {
            MessageContent::Text(_) => None,
            MessageContent::Parts(parts) => {
                let has_image = parts.iter().any(|part| part.is_image());
                if !has_image {
                    return None;
                }
                let text_parts: Vec<ContentPart> = parts.iter().filter(|part| !part.is_image()).cloned().collect();
                if text_parts.is_empty() {
                    Some(MessageContent::Text(String::new()))
                } else if text_parts.len() == 1 {
                    if let ContentPart::Text { text } = &text_parts[0] {
                        Some(MessageContent::Text(text.clone()))
                    } else {
                        Some(MessageContent::Parts(text_parts))
                    }
                } else {
                    Some(MessageContent::Parts(text_parts))
                }
            }
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
            MessageContent::Text(text) => count += crate::utils::estimate_token_count(text),
            MessageContent::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentPart::Text { text } => count += crate::utils::estimate_token_count(text),
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
                    count += crate::utils::estimate_token_count(&func.name);
                    count += crate::utils::estimate_token_count(&func.arguments);
                }
                if let Some(sig) = &call.thought_signature {
                    count += crate::utils::estimate_token_count(sig);
                }
            }
        }

        // Tool call ID (for responses)
        if let Some(id) = &self.tool_call_id {
            count += crate::utils::estimate_token_count(id);
        }

        if let Some(phase) = self.phase {
            count += crate::utils::estimate_token_count(phase.as_str());
        }

        count
    }

    /// Helper to create a base message with common defaults.
    /// Public for use in provider implementations.
    #[inline]
    pub(crate) const fn base(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            phase: None,
            origin_tool: None,
            metadata: None,
            clear_at: None,
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
    pub fn assistant_with_tools_and_parts(content_parts: Vec<ContentPart>, tool_calls: Vec<ToolCall>) -> Self {
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

    /// Create a system message whose lifecycle is scoped to the current turn.
    /// Provider/model routes that advertise native support let Anthropic clear
    /// it when the next user turn arrives. Other routes receive the same text
    /// as an ordinary system/history directive after the runtime removes the
    /// Anthropic-only lifecycle field; canonical history retains the typed
    /// marker for replay fidelity.
    #[inline]
    pub fn turn_scoped_system(content: String) -> Self {
        Self {
            clear_at: Some(MessageClearAt::NextUserMessage),
            ..Self::system(content)
        }
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
    pub fn tool_response_with_name(tool_call_id: String, _function_name: String, content: String) -> Self {
        // We can store the function name in the content metadata or handle it provider-specifically
        Self::tool_response(tool_call_id, content)
    }

    /// Create a tool response message with origin tool tracking
    /// The origin_tool field helps with tool-aware context retention
    #[inline]
    pub fn tool_response_with_origin(tool_call_id: String, content: String, origin_tool: String) -> Self {
        Self {
            tool_call_id: Some(tool_call_id),
            origin_tool: Some(origin_tool),
            ..Self::base(MessageRole::Tool, MessageContent::Text(content))
        }
    }

    /// Create a user message with image from a local file
    pub async fn user_with_local_image<P: AsRef<std::path::Path>>(file_path: P) -> Result<Self, anyhow::Error> {
        let image_data = vtcode_commons::image::read_image_file(file_path).await?;
        let image_part = ContentPart::image(image_data.base64_data, image_data.mime_type);
        Ok(Self::user_with_parts(vec![image_part]))
    }

    /// Create a user message with text and a local image
    pub async fn user_with_text_and_local_image<P: AsRef<std::path::Path>>(
        text: String,
        file_path: P,
    ) -> Result<Self, anyhow::Error> {
        let image_data = vtcode_commons::image::read_image_file(file_path).await?;
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
    pub fn with_reasoning_details(mut self, reasoning_details: Option<Vec<serde_json::Value>>) -> Self {
        self.reasoning_details = reasoning_details;
        self
    }

    /// Attach assistant phase metadata for providers that support it.
    #[must_use]
    pub fn with_phase(mut self, phase: Option<AssistantPhase>) -> Self {
        self.phase = if self.role == MessageRole::Assistant {
            phase
        } else {
            None
        };
        self
    }

    /// Attach per-message metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: MessageMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Validate this message for a specific provider
    /// Based on official API documentation constraints
    pub(crate) fn validate_for_provider(&self, provider: &str) -> Result<(), String> {
        if self.clear_at.is_some() {
            if self.role != MessageRole::System {
                return Err("clear_at is only valid on system messages".to_owned());
            }
            let text_only = match &self.content {
                MessageContent::Text(_) => true,
                MessageContent::Parts(parts) => parts.iter().all(|part| matches!(part, ContentPart::Text { .. })),
            };
            if !text_only {
                return Err("clear_at system messages must contain text-only content".to_owned());
            }
            if !provider.eq_ignore_ascii_case("anthropic") {
                return Err(format!("clear_at system messages are only supported by Anthropic, got {provider}"));
            }
        }

        // Check role-specific constraints
        self.role.validate_for_provider(provider, self.tool_call_id.is_some())?;

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
            "openai" | "openrouter" | "meta" | "zai" | "stepfun" | "evolink" | "deepseek" => {
                if self.role == MessageRole::Tool && self.tool_call_id.is_none() {
                    return Err(format!("{provider} requires tool_call_id for tool messages"));
                }
            }
            "gemini" => {
                if self.role == MessageRole::Tool && self.tool_call_id.is_none() {
                    return Err("Gemini tool responses need tool_call_id for function name mapping".to_owned());
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

        // DeepSeek vision guard rails: images only in user messages, MIME/size checks
        if provider == "deepseek" && self.has_images() {
            if self.role != MessageRole::User {
                return Err("DeepSeek vision images are only supported in user messages".to_owned());
            }
            for part in self.content.get_images() {
                if let Err(e) = part.validate_image() {
                    return Err(format!("DeepSeek image validation failed: {e}"));
                }
            }
        }

        Ok(())
    }

    /// Check if this message has tool calls
    pub(crate) fn has_tool_calls(&self) -> bool {
        self.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
    pub(crate) fn as_gemini_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system", // Handled as systemInstruction, not in contents
            MessageRole::User => "user",
            MessageRole::Assistant => "model", // Gemini uses "model" instead of "assistant"
            MessageRole::Tool => "user",       // Tool responses are sent as user messages with functionResponse
        }
    }

    /// Get the role string for OpenAI API
    /// OpenAI supports all standard role types including:
    /// - system, user, assistant, tool
    /// - function (legacy, now replaced by tool)
    pub(crate) fn as_openai_str(&self) -> &'static str {
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
    pub(crate) fn as_anthropic_str(&self) -> &'static str {
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
    fn can_make_tool_calls(&self) -> bool {
        matches!(self, MessageRole::Assistant)
    }

    /// Check if this role represents a tool response
    pub fn is_tool_response(&self) -> bool {
        matches!(self, MessageRole::Tool)
    }

    /// Validate message role constraints for a given provider
    /// Based on official API documentation requirements
    fn validate_for_provider(&self, provider: &str, has_tool_call_id: bool) -> Result<(), String> {
        match (self, provider) {
            (MessageRole::Tool, provider)
                if matches!(provider, "openai" | "openrouter" | "meta" | "deepseek" | "zai") && !has_tool_call_id =>
            {
                Err(format!("{provider} tool messages must have tool_call_id"))
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
    use super::{
        AssistantPhase, ContentPart, ImageDetail, Message, MessageClearAt, MessageContent, MessageRole, ToolCall,
    };

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

    #[test]
    fn message_content_parts_with_single_text_stays_borrowed() {
        let content = MessageContent::Parts(vec![ContentPart::text("borrowed".to_string())]);

        assert!(matches!(content.as_text(), std::borrow::Cow::Borrowed("borrowed")));
    }

    #[test]
    fn message_content_parts_without_text_stays_borrowed_empty() {
        let content = MessageContent::Parts(vec![ContentPart::image("encoded".to_string(), "image/png".to_string())]);

        assert!(matches!(content.as_text(), std::borrow::Cow::Borrowed("")));
    }

    #[test]
    fn assistant_phase_parses_wire_strings() {
        assert_eq!(AssistantPhase::from_wire_str("commentary"), Some(AssistantPhase::Commentary));
        assert_eq!(AssistantPhase::from_wire_str("final_answer"), Some(AssistantPhase::FinalAnswer));
        assert_eq!(AssistantPhase::from_wire_str("other"), None);
    }

    /// `AssistantPhase` exposes the same value through the derived serde wire
    /// form, `as_str()`, and `from_wire_str()`. Lock all three so a one-sided
    /// edit cannot desync the persisted phase from the wire parser.
    #[test]
    fn assistant_phase_string_paths_stay_in_lockstep() {
        for phase in [AssistantPhase::Commentary, AssistantPhase::FinalAnswer] {
            let expected = phase.as_str();
            assert_eq!(
                serde_json::to_value(phase).expect("serializes"),
                serde_json::json!(expected),
                "serde form drifted for {phase:?}"
            );
            assert_eq!(AssistantPhase::from_wire_str(expected), Some(phase), "from_wire_str drift");
        }
    }

    /// `ImageDetail` mirrors its `#[serde(rename)]` values in `as_str()` and
    /// `from_str()`; lock them together.
    #[test]
    fn image_detail_string_paths_stay_in_lockstep() {
        for detail in [
            ImageDetail::Low,
            ImageDetail::High,
            ImageDetail::Original,
            ImageDetail::Auto,
        ] {
            let expected = detail.as_str();
            assert_eq!(
                serde_json::to_value(&detail).expect("serializes"),
                serde_json::json!(expected),
                "serde form drifted for {detail:?}"
            );
            assert_eq!(ImageDetail::from_str(expected), Some(detail), "from_str drift");
        }
    }

    #[test]
    fn with_phase_ignores_non_assistant_roles() {
        let user = Message::user("hello".to_string()).with_phase(Some(AssistantPhase::Commentary));
        let tool = Message::tool_response("call_1".to_string(), "ok".to_string())
            .with_phase(Some(AssistantPhase::FinalAnswer));

        assert_eq!(user.role, MessageRole::User);
        assert!(user.phase.is_none());
        assert_eq!(tool.role, MessageRole::Tool);
        assert!(tool.phase.is_none());
    }

    #[test]
    fn turn_scoped_system_message_round_trips_clear_scope() {
        let message = Message::turn_scoped_system("Only you see the output".to_string());
        let encoded = serde_json::to_value(&message).expect("message serialization");

        assert_eq!(encoded["role"], "System");
        assert_eq!(encoded["clear_at"], "next_user_message");
        assert_eq!(message.clear_at, Some(MessageClearAt::NextUserMessage));
        assert_eq!(serde_json::from_value::<Message>(encoded).expect("message deserialization"), message);
    }

    #[test]
    fn turn_scoped_system_message_requires_non_anthropic_wire_translation() {
        let error = Message::turn_scoped_system("notice".to_string())
            .validate_for_provider("openai")
            .expect_err("raw Anthropic lifecycle fields are not valid on an OpenAI wire");
        assert!(error.contains("only supported by Anthropic"));
    }

    #[test]
    fn validate_for_provider_accepts_recovered_tool_arguments() {
        let message = Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "call_search".to_string(),
                "code_search".to_string(),
                "{\"query\": \"persistent_memory\", \"path\": \"crates/codegen/vtcode-core/src</parameter>\n<</invoke>\n</minimax:tool_call>".to_string(),
            )],
        );

        message.validate_for_provider("anthropic").unwrap();
    }

    #[test]
    fn validate_for_provider_requires_tool_call_id_for_meta() {
        let message = Message {
            role: MessageRole::Tool,
            content: MessageContent::text("result".to_owned()),
            ..Default::default()
        };

        let error = message
            .validate_for_provider("meta")
            .expect_err("Meta tool messages need an id");
        assert!(error.contains("tool_call_id"));
    }
}
