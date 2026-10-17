//! Open Responses specification compliance configuration.

use serde::{Deserialize, Serialize};

/// Open Responses specification compliance configuration
///
/// Enables vendor-neutral LLM API format per the Open Responses specification
/// (<https://www.openresponses.org/>). When enabled, VT Code emits semantic
/// streaming events and uses standardized response/item structures.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenResponsesConfig {
    /// Enable Open Responses specification compliance layer
    /// When true, VT Code emits semantic streaming events alongside internal events
    /// Default: false (opt-in feature)
    #[serde(default)]
    pub enabled: bool,

    /// Emit Open Responses events to the event sink
    /// When true, streaming events follow Open Responses format
    /// (response.created, response.output_item.added, response.output_text.delta, etc.)
    #[serde(default = "default_open_responses_emit_events")]
    pub emit_events: bool,

    /// Include VT Code extension items (vtcode:file_change, vtcode:web_search, etc.)
    /// When false, extension items are omitted from the Open Responses output
    #[serde(default = "default_open_responses_include_extensions")]
    pub include_extensions: bool,

    /// Map internal tool calls to Open Responses function_call items
    /// When true, command executions and MCP tool calls are represented as function_call items
    #[serde(default = "default_open_responses_map_tool_calls")]
    pub map_tool_calls: bool,

    /// Include reasoning items in Open Responses output
    /// When true, model reasoning/thinking is exposed as reasoning items
    #[serde(default = "default_open_responses_include_reasoning")]
    pub include_reasoning: bool,
}

impl Default for OpenResponsesConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in by default
            emit_events: default_open_responses_emit_events(),
            include_extensions: default_open_responses_include_extensions(),
            map_tool_calls: default_open_responses_map_tool_calls(),
            include_reasoning: default_open_responses_include_reasoning(),
        }
    }
}

#[inline]
const fn default_open_responses_emit_events() -> bool {
    true // When enabled, emit events by default
}

#[inline]
const fn default_open_responses_include_extensions() -> bool {
    true // Include VT Code-specific extensions by default
}

#[inline]
const fn default_open_responses_map_tool_calls() -> bool {
    true // Map tool calls to function_call items by default
}

#[inline]
const fn default_open_responses_include_reasoning() -> bool {
    true // Include reasoning items by default
}
