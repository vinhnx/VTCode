use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::CacheControl;

/// Extracted struct for `AnthropicContentBlock::ToolUse` (boxed to reduce enum size).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicToolUseBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) toolset_name: Option<String>,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cache_control: Option<CacheControl>,
}

/// Extracted struct for `AnthropicContentBlock::ToolResult` (boxed to reduce enum size).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicToolResultBlock {
    pub(crate) tool_use_id: String,
    pub(crate) content: Value, // string or array of blocks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cache_control: Option<CacheControl>,
}

impl AnthropicToolUseBlock {
    pub(crate) fn function(id: String, name: String, input: Value) -> Self {
        Self {
            toolset_name: None,
            id,
            name,
            input,
            cache_control: None,
        }
    }
}
