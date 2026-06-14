pub mod request;
pub mod response;

pub use request::{GenerateContentRequest, GenerationConfig, ThinkingConfig};
pub use response::{Candidate, GenerateContentResponse};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstruction {
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolCall {
    #[serde(rename = "toolType", alias = "tool_type")]
    pub tool_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolResponse {
    #[serde(rename = "toolType", alias = "tool_type")]
    pub tool_type: String,
    pub response: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Content {
    pub fn user_text(text: impl Into<String>) -> Self {
        Content {
            role: "user".into(),
            parts: vec![Part::Text {
                text: text.into(),
                thought_signature: None,
            }],
        }
    }

    pub fn system_text(text: impl Into<String>) -> Self {
        // This creates a Content for backwards compatibility
        // For systemInstruction field, use SystemInstruction::new() instead
        Content {
            role: "user".into(), // Convert system to user to avoid API error
            parts: vec![Part::Text {
                text: format!("System: {}", text.into()),
                thought_signature: None,
            }],
        }
    }

    pub fn user_parts(parts: Vec<Part>) -> Self {
        Content {
            role: "user".into(),
            parts,
        }
    }
}

impl SystemInstruction {
    pub fn new(text: impl Into<String>) -> Self {
        SystemInstruction {
            parts: vec![Part::Text {
                text: text.into(),
                thought_signature: None,
            }],
        }
    }

    pub fn with_ttl(text: impl Into<String>, ttl_seconds: u64) -> Self {
        SystemInstruction {
            parts: vec![
                Part::Text {
                    text: text.into(),
                    thought_signature: None,
                },
                Part::CacheControl {
                    ttl_seconds: Some(ttl_seconds),
                },
            ],
        }
    }
}

/// IMPORTANT: Variant ordering matters for `#[serde(untagged)]` deserialization.
/// Serde tries variants in declaration order and returns the first successful match.
/// Variants with required fields (FunctionCall, FunctionResponse, InlineData, Text)
/// must come BEFORE CacheControl, which has only optional fields and would otherwise
/// act as a catch-all, matching any JSON object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    #[serde(rename_all = "camelCase")]
    FunctionCall {
        function_call: super::function_calling::FunctionCall,
        /// Gemini 3 thought signature for maintaining reasoning context
        /// Required for sequential function calling, optional for parallel calls (only first has it)
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FunctionResponse {
        function_response: super::function_calling::FunctionResponse,
        /// Gemini 3 thought signature for maintaining reasoning context
        /// Preserved when echoing function responses back to the model
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        #[serde(rename = "toolCall")]
        tool_call: ServerToolCall,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ToolResponse {
        #[serde(rename = "toolResponse")]
        tool_response: ServerToolResponse,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ExecutableCode {
        #[serde(rename = "executableCode")]
        executable_code: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    CodeExecutionResult {
        #[serde(rename = "codeExecutionResult")]
        code_execution_result: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    InlineData {
        #[serde(rename = "inline_data")]
        inline_data: InlineData,
    },
    Text {
        text: String,
        /// Gemini 3 thought signature for maintaining reasoning context
        /// Must be preserved and sent back exactly as received
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature", alias = "thought_signature")]
        thought_signature: Option<String>,
    },
    /// CacheControl MUST be last: it has only optional fields and would match any JSON object
    #[serde(rename_all = "camelCase")]
    CacheControl {
        #[serde(rename = "ttlSeconds")]
        ttl_seconds: Option<u64>,
    },
}

impl Part {
    /// Get the text content if this is a Text part
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Part::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn thought_signature(&self) -> Option<&str> {
        match self {
            Part::FunctionCall {
                thought_signature, ..
            }
            | Part::FunctionResponse {
                thought_signature, ..
            }
            | Part::ToolCall {
                thought_signature, ..
            }
            | Part::ToolResponse {
                thought_signature, ..
            }
            | Part::ExecutableCode {
                thought_signature, ..
            }
            | Part::CodeExecutionResult {
                thought_signature, ..
            }
            | Part::Text {
                thought_signature, ..
            } => thought_signature.as_deref(),
            Part::InlineData { .. } | Part::CacheControl { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tool {
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "functionDeclarations"
    )]
    pub function_declarations: Option<Vec<FunctionDeclaration>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "googleSearch")]
    pub google_search: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "googleMaps")]
    pub google_maps: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "urlContext")]
    pub url_context: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "fileSearch")]
    pub file_search: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "codeExecution")]
    pub code_execution: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineData {
    #[serde(rename = "mime_type")]
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConfig {
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "functionCallingConfig"
    )]
    pub function_calling_config: Option<super::function_calling::FunctionCallingConfig>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "includeServerSideToolInvocations"
    )]
    pub include_server_side_tool_invocations: Option<bool>,
}

impl ToolConfig {
    pub fn auto() -> Self {
        Self {
            function_calling_config: Some(super::function_calling::FunctionCallingConfig::auto()),
            include_server_side_tool_invocations: None,
        }
    }

    pub fn validated() -> Self {
        Self {
            function_calling_config: Some(
                super::function_calling::FunctionCallingConfig::validated(),
            ),
            include_server_side_tool_invocations: None,
        }
    }
}
