//! Tool schema compaction utilities for session tool catalog.

use serde_json::{Value, json};
use vtcode_config::ToolDocumentationMode;

/// Default max description length for MCP tool descriptions in Full mode.
/// MCP tools from external servers can have arbitrarily long descriptions;
/// capping them prevents token inflation.
pub const MCP_TOOL_DESCRIPTION_MAX_LEN: usize = 512;

pub fn compact_tool_description(original: &str, mode: ToolDocumentationMode, per_tool_max: Option<usize>) -> String {
    let mode_max = match mode {
        ToolDocumentationMode::Minimal => 64,
        ToolDocumentationMode::Progressive => 120,
        ToolDocumentationMode::Full => usize::MAX,
    };
    let max_len = per_tool_max.unwrap_or(mode_max);

    let sentence = original
        .split('.')
        .next()
        .unwrap_or(original)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if sentence.len() <= max_len {
        sentence
    } else {
        let target = max_len.saturating_sub(1);
        let end = sentence
            .char_indices()
            .map(|(index, _)| index)
            .rfind(|&index| index <= target)
            .unwrap_or(0);
        format!("{}…", &sentence[..end])
    }
}

pub fn compact_parameters(parameters: Value, mode: ToolDocumentationMode) -> Value {
    if matches!(mode, ToolDocumentationMode::Full) {
        return parameters;
    }

    let mut compacted = parameters;
    remove_schema_descriptions(&mut compacted);
    compacted
}

pub fn remove_schema_descriptions(value: &mut Value) {
    remove_schema_descriptions_impl(value, false);
}

fn remove_schema_descriptions_impl(value: &mut Value, inside_properties_map: bool) {
    match value {
        Value::Object(map) => {
            if !inside_properties_map {
                map.remove("description");
            }
            for (key, nested) in map.iter_mut() {
                remove_schema_descriptions_impl(nested, key == "properties");
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_schema_descriptions_impl(item, false);
            }
        }
        _ => {}
    }
}

pub fn default_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}
