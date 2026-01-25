use serde::{Deserialize, Serialize};
use serde_json::Value;

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
