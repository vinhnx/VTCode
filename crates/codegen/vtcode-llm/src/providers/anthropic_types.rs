use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AnthropicFallback {
    #[serde(rename = "fallback")]
    Fallback {
        from: AnthropicFallbackModel,
        to: AnthropicFallbackModel,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicFallbackModel {
    model: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<AnthropicMessage>,
    pub(crate) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system: Option<Value>, // Can be string or array of blocks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning: Option<Value>, // Deprecated in favor of thinking, but kept for backward compat or direct effort passing
    #[serde(
        default,
        deserialize_with = "deserialize_boxed_output_config_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) output_config: Option<Box<AnthropicOutputConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) context_management: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallbacks: Option<Vec<AnthropicFallbackParam>>,
    /// Opaque credit token returned by a refused request's `stop_details.fallback_credit_token`.
    /// Echoed on the retry to avoid paying the prompt-cache cost twice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_credit_token: Option<String>,
    pub(crate) stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicFallbackParam {
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ThinkingConfig {
    Enabled {
        budget_tokens: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        display: Option<ThinkingDisplay>,
    },
    Adaptive {
        #[serde(skip_serializing_if = "Option::is_none")]
        display: Option<ThinkingDisplay>,
    },
    Disabled,
    /// Catch-all for unknown thinking config types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

/// Controls how thinking content is returned in API responses.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingDisplay {
    /// Thinking blocks contain summarized thinking text (default on Claude 4 models).
    Summarized,
    /// Thinking blocks are returned with an empty `thinking` field; the `signature`
    /// still carries encrypted full thinking for multi-turn continuity
    /// (default on Claude Opus 4.7).
    Omitted,
    /// Catch-all for unknown display modes added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub(crate) role: String,
    pub(crate) content: Vec<AnthropicContentBlock>,
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
    ToolUse(Box<AnthropicToolUseBlock>),
    #[serde(rename = "tool_result")]
    ToolResult(Box<AnthropicToolResultBlock>),
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking {
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    #[serde(rename = "compaction")]
    Compaction {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
    /// Server-side tool use (e.g., tool search execution) - advanced-tool-use beta
    #[serde(rename = "server_tool_use")]
    ServerToolUse { id: String, name: String, input: Value },
    /// Tool search result containing discovered tool references - advanced-tool-use beta
    #[serde(rename = "tool_search_tool_result")]
    ToolSearchToolResult {
        tool_use_id: String,
        content: ToolSearchResultContent,
    },
    /// Files API upload reference used by Anthropic code execution.
    #[serde(rename = "container_upload")]
    ContainerUpload { file_id: String },
    /// Generic code execution result block used in some Anthropic responses.
    #[serde(rename = "code_execution_tool_result")]
    CodeExecutionToolResult { tool_use_id: String, content: Value },
    /// Bash code execution result block returned by Anthropic code execution.
    #[serde(rename = "bash_code_execution_tool_result")]
    BashCodeExecutionToolResult { tool_use_id: String, content: Value },
    /// Text editor code execution result block returned by Anthropic code execution.
    #[serde(rename = "text_editor_code_execution_tool_result")]
    TextEditorCodeExecutionToolResult { tool_use_id: String, content: Value },
    /// Native web search result blocks returned by Anthropic web search tools.
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult { tool_use_id: String, content: Value },
    /// Advisor tool result block returned by the server-side advisor sub-inference.
    /// `content` is the verbatim result union (`advisor_result`,
    /// `advisor_redacted_result`, or `advisor_tool_result_error`).
    #[serde(rename = "advisor_tool_result")]
    AdvisorToolResult { tool_use_id: String, content: Value },
    /// Fallback content block marking model boundary in server-side fallback
    #[serde(rename = "fallback")]
    Fallback {
        from: AnthropicFallbackModel,
        to: AnthropicFallbackModel,
    },
    /// Catch-all for unknown content block types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

/// Extracted struct for `AnthropicContentBlock::ToolUse` (boxed to reduce enum size).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicToolUseBlock {
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

/// Content of a tool search result
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ToolSearchResultContent {
    #[serde(rename = "tool_search_tool_search_result")]
    SearchResult { tool_references: Vec<ToolReference> },
    #[serde(rename = "tool_search_tool_result_error")]
    Error { error_code: String },
    /// Catch-all for unknown tool search result types.
    #[serde(other)]
    Unknown,
}

/// A reference to a discovered tool from tool search
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolReference {
    #[serde(rename = "type")]
    ref_type: Option<String>, // "tool_reference"
    tool_name: String,
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
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation {
        url: Option<String>,
        title: Option<String>,
        encrypted_index: Option<String>,
        cited_text: Option<String>,
    },
    /// Catch-all for unknown citation types.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub(crate) source_type: String, // "base64"
    pub(crate) media_type: String,
    pub(crate) data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub(crate) control_type: String, // "ephemeral"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ttl: Option<String>, // "5m" or "1h"
}

/// Anthropic tool definition
/// Supports both regular function tools and tool search tools (advanced-tool-use beta)
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum AnthropicTool {
    /// Tool search tool (regex or bm25)
    ToolSearch(AnthropicToolSearchTool),
    /// Native Anthropic code execution tool revision
    CodeExecution(AnthropicCodeExecutionTool),
    /// Native Anthropic memory tool revision
    Memory(AnthropicMemoryTool),
    /// Native Anthropic web search tool revision
    WebSearch(AnthropicWebSearchTool),
    /// Anthropic server-side advisor tool (beta `advisor-tool-2026-03-01`).
    Advisor(AnthropicAdvisorTool),
    /// Regular function tool
    Function(AnthropicFunctionTool),
}

/// Regular function tool definition for Anthropic API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicFunctionTool {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) input_examples: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) strict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) allowed_callers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cache_control: Option<CacheControl>,
    /// When true, the tool is deferred and only loaded when discovered via tool search
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) defer_loading: Option<bool>,
}

/// Native code execution tool definition for Anthropic API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicCodeExecutionTool {
    /// Versioned code execution type (e.g. "code_execution_20250825")
    #[serde(rename = "type")]
    pub(crate) tool_type: String,
    /// Tool name (typically "code_execution")
    pub(crate) name: String,
}

/// Native memory tool definition for Anthropic API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicMemoryTool {
    /// Versioned memory type (e.g. "memory_20250818")
    #[serde(rename = "type")]
    pub(crate) tool_type: String,
    /// Tool name (typically "memory")
    pub(crate) name: String,
}

/// Tool search tool definition for Anthropic's advanced-tool-use beta
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicToolSearchTool {
    /// The type of tool search: "tool_search_tool_regex_20251119" or "tool_search_tool_bm25_20251119"
    #[serde(rename = "type")]
    pub(crate) tool_type: String,
    /// Tool name (e.g., "tool_search_tool_regex" or "tool_search_tool_bm25")
    pub(crate) name: String,
}

/// Native web search tool definition for Anthropic API (PTC-enabled search revisions)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicWebSearchTool {
    /// Versioned web search type (e.g., "web_search_20260209")
    #[serde(rename = "type")]
    pub(crate) tool_type: String,
    /// Tool name (typically "web_search")
    pub(crate) name: String,
    /// Optional Anthropic-native web search configuration.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub(crate) options: Map<String, Value>,
}

/// Anthropic server-side advisor tool definition (beta `advisor-tool-2026-03-01`).
///
/// The executor model consults the advisor model for strategic guidance
/// mid-generation. The tool takes no client-supplied input; the server builds
/// the advisor's view from the full transcript.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicAdvisorTool {
    /// Fixed advisor tool type.
    #[serde(rename = "type")]
    pub(crate) tool_type: String, // "advisor_20260301"
    /// Fixed advisor tool name.
    pub(crate) name: String, // "advisor"
    /// Advisor model id (e.g. "claude-opus-4-8").
    pub(crate) model: String,
    /// Maximum number of advisor invocations per request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_uses: Option<u32>,
    /// Caps the advisor's total output (thinking plus text) per call (min 1024).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    /// Enables prompt caching for the advisor's own transcript across calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) caching: Option<AnthropicAdvisorCaching>,
}

/// Prompt-caching configuration for the advisor tool.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicAdvisorCaching {
    /// Fixed cache type.
    #[serde(rename = "type")]
    pub(crate) cache_type: String, // "ephemeral"
    /// Cache lifetime ("5m" or "1h").
    pub(crate) ttl: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicMessageResponse },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: AnthropicContentBlock,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: AnthropicStreamDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: Option<AnthropicUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(rename = "error")]
    Error { error: AnthropicErrorBody },
    /// Catch-all for unknown streaming event types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
    #[serde(rename = "compaction_delta")]
    CompactionDelta { content: String },
    /// Catch-all for unknown delta types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessageDelta {
    pub(crate) stop_reason: Option<String>,
    stop_sequence: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AnthropicStopDetails {
    #[serde(rename = "type")]
    stop_details_type: Option<String>,
    category: Option<String>,
    explanation: Option<String>,
    /// Opaque token that represents fallback credit when retrying on another model.
    /// Present when the refusal qualifies for fallback credit.
    fallback_credit_token: Option<String>,
    /// Whether the retry can append an assistant message continuing the refused model's
    /// partial output (`true`) or must use the unchanged request body (`false`).
    fallback_has_prefill_claim: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessageResponse {
    id: String,
    role: String,
    content: Vec<AnthropicContentBlock>,
    model: String,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    stop_details: Option<AnthropicStopDetails>,
    pub(crate) usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicUsage {
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
    pub(crate) cache_creation_input_tokens: Option<u32>,
    pub(crate) cache_read_input_tokens: Option<u32>,
    /// Per-iteration token usage, populated when compaction triggers or server-side fallback runs.
    /// Each entry represents one sampling pass (compaction, message, or fallback_message).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iterations: Option<Vec<AnthropicUsageIteration>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicUsageIteration {
    Message {
        model: String,
        input_tokens: u32,
        output_tokens: u32,
        cache_creation_input_tokens: Option<u32>,
        cache_read_input_tokens: Option<u32>,
    },
    FallbackMessage {
        model: String,
        input_tokens: u32,
        output_tokens: u32,
        cache_creation_input_tokens: Option<u32>,
        cache_read_input_tokens: Option<u32>,
    },
    Compaction {
        input_tokens: u32,
        output_tokens: u32,
        cache_creation_input_tokens: Option<u32>,
        cache_read_input_tokens: Option<u32>,
    },
    /// Catch-all for unknown iteration types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: String,
    pub(crate) message: String,
}

/// Output configuration for Anthropic API (e.g., effort parameter)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicOutputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) task_budget: Option<AnthropicTaskBudget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) format: Option<AnthropicOutputFormat>,
}

impl AnthropicOutputConfig {
    fn is_empty(&self) -> bool {
        self.effort.is_none() && self.task_budget.is_none() && self.format.is_none()
    }

    fn into_boxed_if_non_empty(self) -> Option<Box<Self>> {
        (!self.is_empty()).then_some(Box::new(self))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicTaskBudget {
    #[serde(rename = "type")]
    pub(crate) budget_type: String,
    pub(crate) total: u32,
}

/// Native structured output format for Anthropic responses.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicOutputFormat {
    JsonSchema {
        schema: Value,
    },
    /// Catch-all for unknown output format types added by the Anthropic API.
    #[serde(other)]
    Unknown,
}

/// Request body for Anthropic's count_tokens endpoint
/// <https://docs.anthropic.com/en/api/messages-count-tokens>
#[derive(Debug, Serialize, Deserialize)]
pub struct CountTokensRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Value>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

fn deserialize_boxed_output_config_opt<'de, D>(deserializer: D) -> Result<Option<Box<AnthropicOutputConfig>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<AnthropicOutputConfig>::deserialize(deserializer)
        .map(|value| value.and_then(AnthropicOutputConfig::into_boxed_if_non_empty))
}

/// Response from Anthropic's count_tokens endpoint
#[derive(Debug, Deserialize)]
pub struct CountTokensResponse {
    input_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::{AnthropicContentBlock, AnthropicOutputConfig, AnthropicRequest, AnthropicStreamEvent, TextCitation};

    #[test]
    fn stream_content_block_start_thinking_allows_missing_signature() {
        let payload = r#"{
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "thinking",
                "thinking": "Drafting plan"
            }
        }"#;

        let event: AnthropicStreamEvent = serde_json::from_str(payload).expect("should deserialize thinking block");
        match event {
            AnthropicStreamEvent::ContentBlockStart {
                content_block: AnthropicContentBlock::Thinking { thinking, signature, .. },
                ..
            } => {
                assert_eq!(thinking, "Drafting plan");
                assert!(signature.is_none());
            }
            other => panic!("expected thinking content_block_start, got {other:?}"),
        }
    }

    #[test]
    fn stream_content_block_start_accepts_web_search_tool_result() {
        let payload = r#"{
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "web_search_tool_result",
                "tool_use_id": "srvtoolu_123",
                "content": [{
                    "type": "web_search_result",
                    "title": "Rust Releases",
                    "url": "https://blog.rust-lang.org"
                }]
            }
        }"#;

        let event: AnthropicStreamEvent =
            serde_json::from_str(payload).expect("should deserialize web search tool result");
        match event {
            AnthropicStreamEvent::ContentBlockStart {
                content_block: AnthropicContentBlock::WebSearchToolResult { tool_use_id, .. },
                ..
            } => assert_eq!(tool_use_id, "srvtoolu_123"),
            other => panic!("expected web_search_tool_result content_block_start, got {other:?}"),
        }
    }

    #[test]
    fn stream_content_block_start_accepts_bash_code_execution_result() {
        let payload = r#"{
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "bash_code_execution_tool_result",
                "tool_use_id": "srvtoolu_456",
                "content": {
                    "type": "bash_code_execution_result",
                    "stdout": "Python 3.11.12",
                    "stderr": "",
                    "return_code": 0
                }
            }
        }"#;

        let event: AnthropicStreamEvent =
            serde_json::from_str(payload).expect("should deserialize bash code execution result");
        match event {
            AnthropicStreamEvent::ContentBlockStart {
                content_block: AnthropicContentBlock::BashCodeExecutionToolResult { tool_use_id, .. },
                ..
            } => assert_eq!(tool_use_id, "srvtoolu_456"),
            other => panic!("expected bash_code_execution_tool_result content_block_start, got {other:?}"),
        }
    }

    #[test]
    fn stream_content_block_start_accepts_text_editor_code_execution_result() {
        let payload = r#"{
            "type": "content_block_start",
            "index": 2,
            "content_block": {
                "type": "text_editor_code_execution_tool_result",
                "tool_use_id": "srvtoolu_789",
                "content": {
                    "type": "text_editor_code_execution_result",
                    "is_file_update": false
                }
            }
        }"#;

        let event: AnthropicStreamEvent =
            serde_json::from_str(payload).expect("should deserialize text editor code execution result");
        match event {
            AnthropicStreamEvent::ContentBlockStart {
                content_block: AnthropicContentBlock::TextEditorCodeExecutionToolResult { tool_use_id, .. },
                ..
            } => assert_eq!(tool_use_id, "srvtoolu_789"),
            other => panic!("expected text_editor_code_execution_tool_result content_block_start, got {other:?}"),
        }
    }

    #[test]
    fn text_block_accepts_web_search_result_citation() {
        let payload = r#"{
            "type": "text",
            "text": "Rust 1.82 shipped",
            "citations": [{
                "type": "web_search_result_location",
                "url": "https://blog.rust-lang.org",
                "title": "Rust Blog",
                "encrypted_index": "enc_123",
                "cited_text": "Rust 1.82 shipped"
            }]
        }"#;

        let block: AnthropicContentBlock = serde_json::from_str(payload).expect("should deserialize cited text block");
        match block {
            AnthropicContentBlock::Text { citations: Some(citations), .. } => {
                assert!(matches!(
                    &citations[0],
                    TextCitation::WebSearchResultLocation {
                        url: Some(url),
                        title: Some(title),
                        encrypted_index: Some(index),
                        cited_text: Some(cited_text),
                    } if url == "https://blog.rust-lang.org"
                        && title == "Rust Blog"
                        && index == "enc_123"
                        && cited_text == "Rust 1.82 shipped"
                ));
            }
            other => panic!("expected text block with citations, got {other:?}"),
        }
    }

    #[test]
    fn empty_output_config_deserializes_to_none() {
        let request: AnthropicRequest = serde_json::from_str(
            r#"{
                "model": "claude-sonnet",
                "messages": [],
                "max_tokens": 128,
                "output_config": {},
                "stream": false
            }"#,
        )
        .expect("should deserialize request");

        assert!(request.output_config.is_none());
    }

    #[test]
    fn boxed_output_config_is_smaller_than_inline_option() {
        use std::mem::size_of;

        assert!(size_of::<Option<Box<AnthropicOutputConfig>>>() < size_of::<Option<AnthropicOutputConfig>>());
    }
}
