use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::GenerationConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionRequest {
    pub(crate) model: String,
    pub(crate) input: InteractionInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<InteractionTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system_instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) generation_config: Option<InteractionGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<InteractionToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) previous_interaction_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InteractionInput {
    Text(String),
    Content(Vec<InteractionContent>),
    Turns(Vec<InteractionTurn>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionTurn {
    pub(crate) role: String,
    pub(crate) content: InteractionTurnContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InteractionTurnContent {
    Text(String),
    Content(Vec<InteractionContent>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    FunctionCall {
        id: String,
        name: String,
        arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    FunctionResult {
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        result: InteractionResult,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Catch-all for unknown content types added by the Gemini API.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InteractionResult {
    String(String),
    Json(Value),
    Content(Vec<InteractionContent>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionTool {
    #[serde(rename = "type")]
    pub(crate) tool_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
    #[serde(flatten, default)]
    pub(crate) extra: Map<String, Value>,
}

impl InteractionTool {
    pub(crate) fn built_in(tool_type: &str, config: Option<&Value>) -> Self {
        let extra = config.and_then(Value::as_object).cloned().unwrap_or_default();
        Self {
            tool_type: tool_type.to_string(),
            name: None,
            description: None,
            parameters: None,
            extra,
        }
    }

    pub(crate) fn function(name: String, description: String, parameters: Value) -> Self {
        Self {
            tool_type: "function".to_string(),
            name: Some(name),
            description: Some(description),
            parameters: Some(parameters),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionToolChoice {
    pub(crate) mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<String>>,
}

impl InteractionToolChoice {
    pub(crate) fn new(mode: impl Into<String>) -> Self {
        Self { mode: mode.into(), tools: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_level: Option<String>,
}

impl From<GenerationConfig> for InteractionGenerationConfig {
    fn from(value: GenerationConfig) -> Self {
        Self {
            temperature: value.temperature,
            top_p: value.top_p,
            max_output_tokens: value.max_output_tokens,
            stop_sequences: value.stop_sequences,
            thinking_level: value.thinking_config.and_then(|cfg| cfg.thinking_level),
        }
    }
}

impl From<InteractionGenerationConfig> for GenerationConfig {
    fn from(value: InteractionGenerationConfig) -> Self {
        GenerationConfig {
            temperature: value.temperature,
            top_p: value.top_p,
            max_output_tokens: value.max_output_tokens,
            stop_sequences: value.stop_sequences,
            thinking_config: value
                .thinking_level
                .map(|thinking_level| super::ThinkingConfig { thinking_level: Some(thinking_level) }),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub(crate) id: String,
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) outputs: Vec<InteractionOutput>,
    #[serde(default)]
    pub(crate) usage: Option<InteractionUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOutput {
    #[serde(rename = "type")]
    pub(crate) output_type: String,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) arguments: Option<Value>,
    #[serde(default)]
    pub(crate) signature: Option<String>,
    #[serde(default)]
    pub(crate) function_call: Option<InteractionFunctionCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionFunctionCall {
    pub(crate) name: String,
    pub(crate) arguments: Value,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionUsage {
    #[serde(default)]
    pub(crate) total_input_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) total_output_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) total_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) total_cached_tokens: Option<u32>,
}
