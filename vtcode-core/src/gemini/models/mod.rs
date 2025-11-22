pub mod request;
pub mod response;

pub use request::GenerateContentRequest;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    Text {
        text: String,
        /// Gemini 3 Pro thought signature for maintaining reasoning context
        /// Must be preserved and sent back exactly as received
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FunctionCall {
        function_call: crate::gemini::function_calling::FunctionCall,
        /// Gemini 3 Pro thought signature for maintaining reasoning context
        /// Required for sequential function calling, optional for parallel calls (only first has it)
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FunctionResponse {
        function_response: crate::gemini::function_calling::FunctionResponse,
        /// Gemini 3 Pro thought signature for maintaining reasoning context
        /// Preserved when echoing function responses back to the model
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    #[serde(rename = "functionCallingConfig")]
    pub function_calling_config: crate::gemini::function_calling::FunctionCallingConfig,
}

impl ToolConfig {
    pub fn auto() -> Self {
        Self {
            function_calling_config: crate::gemini::function_calling::FunctionCallingConfig::auto(),
        }
    }
}
