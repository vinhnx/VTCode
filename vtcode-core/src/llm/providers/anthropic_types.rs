use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Value>, // Can be string or array of blocks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>, // Deprecated in favor of thinking, but kept for backward compat or direct effort passing
    pub stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ThinkingConfig {
    Enabled { budget_tokens: u32 },
    Disabled,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        citations: Option<Vec<TextCitation>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "image")]
    Image {
        source: ImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Value, // string or array of blocks
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        signature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking {
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Server-side tool use (e.g., tool search execution) - advanced-tool-use beta
    #[serde(rename = "server_tool_use")]
    ServerToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool search result containing discovered tool references - advanced-tool-use beta
    #[serde(rename = "tool_search_tool_result")]
    ToolSearchToolResult {
        tool_use_id: String,
        content: ToolSearchResultContent,
    },
}

/// Content of a tool search result
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ToolSearchResultContent {
    #[serde(rename = "tool_search_tool_search_result")]
    SearchResult {
        tool_references: Vec<ToolReference>,
    },
    #[serde(rename = "tool_search_tool_result_error")]
    Error {
        error_code: String,
    },
}

/// A reference to a discovered tool from tool search
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolReference {
    #[serde(rename = "type")]
    pub ref_type: Option<String>, // "tool_reference"
    pub tool_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum TextCitation {
    #[serde(rename = "char_location")]
    CharLocation {
        cited_text: String,
        document_index: usize,
        document_title: Option<String>,
        start_char_index: usize,
        end_char_index: usize,
    },
    #[serde(rename = "page_location")]
    PageLocation {
        cited_text: String,
        document_index: usize,
        document_title: Option<String>,
        start_page_number: usize,
        end_page_number: usize,
    },
    #[serde(rename = "content_block_location")]
    ContentBlockLocation {
        cited_text: String,
        document_index: usize,
        document_title: Option<String>,
        start_block_index: usize,
        end_block_index: usize,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String, // "base64"
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub control_type: String, // "ephemeral"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>, // "5m" or "1h"
}

/// Anthropic tool definition
/// Supports both regular function tools and tool search tools (advanced-tool-use beta)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AnthropicTool {
    /// Tool search tool (regex or bm25)
    ToolSearch(AnthropicToolSearchTool),
    /// Regular function tool
    Function(AnthropicFunctionTool),
}

/// Regular function tool definition for Anthropic API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicFunctionTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    /// When true, the tool is deferred and only loaded when discovered via tool search
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
}

/// Tool search tool definition for Anthropic's advanced-tool-use beta
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicToolSearchTool {
    /// The type of tool search: "tool_search_tool_regex_20251119" or "tool_search_tool_bm25_20251119"
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Tool name (e.g., "tool_search_tool_regex" or "tool_search_tool_bm25")
    pub name: String,
}

// Kept for backward compatibility or internal usage if needed, but AnthropicContentBlock::Thinking is preferred
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicThinkingBlock {
    #[serde(rename = "type")]
    pub block_type: String, // "thinking"
    pub thinking: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}
