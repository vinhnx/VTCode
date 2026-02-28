//! Core LLM types shared across the project

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Gemini,
    OpenAI,
    Anthropic,
    DeepSeek,
    OpenRouter,
    Ollama,
    ZAI,
    Moonshot,
    HuggingFace,
    Minimax,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
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
    pub fn total_cache_tokens(&self) -> u32 {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Pause,
    Refusal,
    Error(String),
}

/// Universal tool call that matches OpenAI/Anthropic/Gemini specifications
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
                arguments: text.clone(),
            }),
            text: Some(text),
            thought_signature: None,
        }
    }

    /// Parse the arguments as JSON Value (for function-type tools)
    pub fn parsed_arguments(&self) -> Result<serde_json::Value, serde_json::Error> {
        if let Some(ref func) = self.function {
            parse_tool_arguments(&func.arguments)
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

fn parse_tool_arguments(raw_arguments: &str) -> Result<serde_json::Value, serde_json::Error> {
    let trimmed = raw_arguments.trim();
    match serde_json::from_str(trimmed) {
        Ok(parsed) => Ok(parsed),
        Err(primary_error) => {
            if let Some(candidate) = extract_balanced_json(trimmed)
                && let Ok(parsed) = serde_json::from_str(candidate)
            {
                return Ok(parsed);
            }
            Err(primary_error)
        }
    }
}

fn extract_balanced_json(input: &str) -> Option<&str> {
    let start = input.find(['{', '['])?;
    let opening = input.as_bytes().get(start).copied()?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in input[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            _ if ch as u32 == opening as u32 => depth += 1,
            _ if ch as u32 == closing as u32 => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    return input.get(start..end);
                }
            }
            _ => {}
        }
    }

    None
}

/// Universal LLM response structure
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LLMResponse {
    /// The response content text
    pub content: Option<String>,

    /// Tool calls made by the model
    pub tool_calls: Option<Vec<ToolCall>>,

    /// The model that generated this response
    pub model: String,

    /// Token usage statistics
    pub usage: Option<Usage>,

    /// Why the response finished
    pub finish_reason: FinishReason,

    /// Reasoning content (for models that support it)
    pub reasoning: Option<String>,

    /// Detailed reasoning traces (for models that support it)
    pub reasoning_details: Option<Vec<String>>,

    /// Tool references for context
    pub tool_references: Vec<String>,

    /// Request ID from the provider
    pub request_id: Option<String>,

    /// Organization ID from the provider
    pub organization_id: Option<String>,
}

impl LLMResponse {
    /// Create a new LLM response with mandatory fields
    pub fn new(model: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            tool_calls: None,
            model: model.into(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        }
    }

    /// Get content or empty string
    pub fn content_text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }

    /// Get content as String (clone)
    pub fn content_string(&self) -> String {
        self.content.clone().unwrap_or_default()
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

impl LLMErrorMetadata {
    pub fn new(
        provider: impl Into<String>,
        status: Option<u16>,
        code: Option<String>,
        request_id: Option<String>,
        organization_id: Option<String>,
        retry_after: Option<String>,
        message: Option<String>,
    ) -> Box<Self> {
        Box::new(Self {
            provider: Some(provider.into()),
            status,
            code,
            request_id,
            organization_id,
            retry_after,
            message,
        })
    }
}

/// LLM error types with optional provider metadata
#[derive(Debug, thiserror::Error, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMError {
    #[error("Authentication failed: {message}")]
    Authentication {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    #[error("Rate limit exceeded")]
    RateLimit {
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    #[error("Invalid request: {message}")]
    InvalidRequest {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    #[error("Network error: {message}")]
    Network {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
    #[error("Provider error: {message}")]
    Provider {
        message: String,
        metadata: Option<Box<LLMErrorMetadata>>,
    },
}

#[cfg(test)]
mod tests {
    use super::ToolCall;
    use serde_json::json;

    #[test]
    fn parsed_arguments_accepts_trailing_characters() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "read_file".to_string(),
            r#"{"path":"src/main.rs"} trailing text"#.to_string(),
        );

        let parsed = call
            .parsed_arguments()
            .expect("arguments with trailing text should recover");
        assert_eq!(parsed, json!({"path":"src/main.rs"}));
    }

    #[test]
    fn parsed_arguments_accepts_code_fenced_json() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "read_file".to_string(),
            "```json\n{\"path\":\"src/lib.rs\",\"limit\":25}\n```".to_string(),
        );

        let parsed = call
            .parsed_arguments()
            .expect("code-fenced arguments should recover");
        assert_eq!(parsed, json!({"path":"src/lib.rs","limit":25}));
    }

    #[test]
    fn parsed_arguments_rejects_incomplete_json() {
        let call = ToolCall::function(
            "call_read".to_string(),
            "read_file".to_string(),
            r#"{"path":"src/main.rs""#.to_string(),
        );

        assert!(call.parsed_arguments().is_err());
    }
}
