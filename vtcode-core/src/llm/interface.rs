use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::types::ReasoningEffortLevel;

/// Provider-agnostic request payload shared across adapters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub parallel_tool_config: Option<ParallelToolConfig>,
    pub reasoning_effort: Option<ReasoningEffortLevel>,
}

/// Provider-neutral tool selection semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Auto,
    None,
    Any,
    Specific(SpecificToolChoice),
}

/// Specific tool invocation targeting a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: SpecificFunctionChoice,
}

/// Descriptor for the function to invoke when forcing a tool choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificFunctionChoice {
    pub name: String,
}

impl ToolChoice {
    pub fn auto() -> Self {
        Self::Auto
    }

    pub fn none() -> Self {
        Self::None
    }

    pub fn any() -> Self {
        Self::Any
    }

    pub fn function(name: String) -> Self {
        Self::Specific(SpecificToolChoice {
            tool_type: "function".to_string(),
            function: SpecificFunctionChoice { name },
        })
    }

    pub fn allows_parallel_tools(&self) -> bool {
        matches!(self, Self::Auto | Self::Any)
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Auto => "Model decides when to use tools (allows parallel)",
            Self::None => "No tools will be used",
            Self::Any => "At least one tool must be used (allows parallel)",
            Self::Specific(_) => "Specific tool must be used (no parallel)",
        }
    }

    pub fn to_provider_format(&self, provider: &str) -> Value {
        match (self, provider) {
            (Self::Auto, "openai") | (Self::Auto, "deepseek") => json!("auto"),
            (Self::None, "openai") | (Self::None, "deepseek") => json!("none"),
            (Self::Any, "openai") | (Self::Any, "deepseek") => json!("required"),
            (Self::Specific(choice), "openai") | (Self::Specific(choice), "deepseek") => {
                json!(choice)
            }

            (Self::Auto, "anthropic") => json!({"type": "auto"}),
            (Self::None, "anthropic") => json!({"type": "none"}),
            (Self::Any, "anthropic") => json!({"type": "any"}),
            (Self::Specific(choice), "anthropic") => {
                json!({"type": "tool", "name": choice.function.name})
            }

            (Self::Auto, "gemini") => json!({"mode": "auto"}),
            (Self::None, "gemini") => json!({"mode": "none"}),
            (Self::Any, "gemini") => json!({"mode": "any"}),
            (Self::Specific(choice), "gemini") => {
                json!({"mode": "any", "allowed_function_names": [choice.function.name]})
            }

            _ => match self {
                Self::Auto => json!("auto"),
                Self::None => json!("none"),
                Self::Any => json!("required"),
                Self::Specific(choice) => json!(choice),
            },
        }
    }
}

/// Configuration describing whether providers may execute tools concurrently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelToolConfig {
    pub disable_parallel_tool_use: bool,
    pub max_parallel_tools: Option<usize>,
    pub encourage_parallel: bool,
}

impl Default for ParallelToolConfig {
    fn default() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: Some(5),
            encourage_parallel: true,
        }
    }
}

impl ParallelToolConfig {
    pub fn anthropic_optimized() -> Self {
        Self {
            disable_parallel_tool_use: false,
            max_parallel_tools: None,
            encourage_parallel: true,
        }
    }

    pub fn sequential_only() -> Self {
        Self {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(1),
            encourage_parallel: false,
        }
    }
}

/// Provider-neutral representation of a chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: MessageRole::User,
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: MessageRole::Assistant,
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tools(content: String, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            role: MessageRole::System,
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool_response(tool_call_id: String, content: String) -> Self {
        Self {
            role: MessageRole::Tool,
            content,
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }

    pub fn tool_response_with_name(
        tool_call_id: String,
        _function_name: String,
        content: String,
    ) -> Self {
        Self::tool_response(tool_call_id, content)
    }

    pub fn validate_for_provider(&self, provider: &str) -> Result<(), String> {
        self.role
            .validate_for_provider(provider, self.tool_call_id.is_some())?;

        if let Some(tool_calls) = &self.tool_calls {
            if !self.role.can_make_tool_calls() {
                return Err(format!("Role {:?} cannot make tool calls", self.role));
            }

            if tool_calls.is_empty() {
                return Err("Tool calls array should not be empty".to_string());
            }

            for tool_call in tool_calls {
                tool_call.validate()?;
            }
        }

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
                            .to_string(),
                    );
                }
            }
            "anthropic" => {}
            _ => {}
        }

        Ok(())
    }

    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .map_or(false, |calls| !calls.is_empty())
    }

    pub fn get_tool_calls(&self) -> Option<&[ToolCall]> {
        self.tool_calls.as_deref()
    }

    pub fn is_tool_response(&self) -> bool {
        self.role == MessageRole::Tool
    }
}

/// Supported conversation roles across providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    pub fn as_gemini_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "model",
            MessageRole::Tool => "user",
        }
    }

    pub fn as_openai_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        }
    }

    pub fn as_anthropic_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "user",
        }
    }

    pub fn as_generic_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        }
    }

    pub fn can_make_tool_calls(&self) -> bool {
        matches!(self, MessageRole::Assistant)
    }

    pub fn is_tool_response(&self) -> bool {
        matches!(self, MessageRole::Tool)
    }

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
                Err("Gemini tool messages need tool_call_id for function mapping".to_string())
            }
            _ => Ok(()),
        }
    }
}

/// Structured tool definition shared across providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function schema exposed to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDefinition {
    pub fn function(name: String, description: String, parameters: Value) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name,
                description,
                parameters,
            },
        }
    }

    pub fn function_name(&self) -> &str {
        &self.function.name
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.tool_type != "function" {
            return Err(format!(
                "Only 'function' type is supported, got: {}",
                self.tool_type
            ));
        }

        if self.function.name.is_empty() {
            return Err("Function name cannot be empty".to_string());
        }

        if self.function.description.is_empty() {
            return Err("Function description cannot be empty".to_string());
        }

        if !self.function.parameters.is_object() {
            return Err("Function parameters must be a JSON object".to_string());
        }

        Ok(())
    }
}

/// Model-issued function call details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Arguments associated with a tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn function(id: String, name: String, arguments: String) -> Self {
        Self {
            id,
            call_type: "function".to_string(),
            function: FunctionCall { name, arguments },
        }
    }

    pub fn parsed_arguments(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(&self.function.arguments)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.call_type != "function" {
            return Err(format!(
                "Only 'function' type is supported, got: {}",
                self.call_type
            ));
        }

        if self.id.is_empty() {
            return Err("Tool call ID cannot be empty".to_string());
        }

        if self.function.name.is_empty() {
            return Err("Function name cannot be empty".to_string());
        }

        if let Err(e) = self.parsed_arguments() {
            return Err(format!("Invalid JSON in function arguments: {}", e));
        }

        Ok(())
    }
}

/// Canonical response returned by adapters.
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReason,
    pub reasoning: Option<String>,
}

/// Token accounting associated with a response.
#[derive(Debug, Clone)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
}

/// Completion termination reasons recognized across providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error(String),
}

impl fmt::Display for FinishReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FinishReason::Stop => write!(f, "stop"),
            FinishReason::Length => write!(f, "length"),
            FinishReason::ToolCalls => write!(f, "tool_calls"),
            FinishReason::ContentFilter => write!(f, "content_filter"),
            FinishReason::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}
