//! Helper utilities for tool outcome processing.

use vtcode_core::config::constants::defaults;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};

/// Checks if reasoning content is a duplicate of the main content.
/// Used to avoid showing redundant reasoning when it matches the response.
pub(super) fn reasoning_duplicates_content(reasoning: &str, content: &str) -> bool {
    let cleaned_reasoning = vtcode_core::llm::providers::clean_reasoning_text(reasoning);
    let cleaned_content = vtcode_core::llm::providers::clean_reasoning_text(content);
    !cleaned_reasoning.is_empty()
        && !cleaned_content.is_empty()
        && cleaned_reasoning == cleaned_content
}

pub(super) fn normalize_signature_args(
    tool_name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    let mut normalized = match args.as_object() {
        Some(obj) => serde_json::Value::Object(obj.clone()),
        None => return args.clone(),
    };

    let remove_paging_keys = |value: &mut serde_json::Value| {
        if let Some(map) = value.as_object_mut() {
            for key in [
                "offset_lines",
                "limit",
                "offset",
                "page_size_lines",
                "start_line",
                "end_line",
                "num_lines",
                "offset_bytes",
                "page_size_bytes",
                "max_bytes",
                "max_tokens",
                "max_lines",
                "chunk_lines",
            ] {
                map.remove(key);
            }
        }
    };

    if tool_name == tool_names::READ_FILE {
        remove_paging_keys(&mut normalized);
        return normalized;
    }

    if tool_name == tool_names::UNIFIED_FILE
        && let Some(action) = normalized.get("action").and_then(|value| value.as_str())
        && action == "read"
    {
        remove_paging_keys(&mut normalized);
    }

    normalized
}

pub(super) fn push_tool_response(
    history: &mut Vec<uni::Message>,
    tool_call_id: String,
    content: String,
    tool_name: &str,
) {
    let limited = truncate_message_content(&content);
    history.push(uni::Message::tool_response_with_origin(
        tool_call_id,
        limited,
        tool_name.to_string(),
    ));
    enforce_history_limits(history);
}

pub(super) fn push_assistant_message(history: &mut Vec<uni::Message>, mut message: uni::Message) {
    if message.role == uni::MessageRole::Assistant {
        if let Some(reasoning) = message.reasoning.as_ref()
            && let Some(content) = message.content.as_text_borrowed()
        {
            if reasoning_duplicates_content(reasoning, content) {
                message.reasoning = None;
            }
        }
    }
    if let Some(text) = message.content.as_text_borrowed() {
        let limited = truncate_message_content(text);
        message.content = uni::MessageContent::text(limited);
    }
    if let Some(reasoning) = message.reasoning.as_ref() {
        message.reasoning = Some(truncate_message_content(reasoning));
    }
    history.push(message);
    enforce_history_limits(history);
}

pub(super) fn resolve_max_tool_retries(vt_cfg: Option<&VTCodeConfig>) -> usize {
    vt_cfg
        .map(|cfg| cfg.agent.harness.max_tool_retries as usize)
        .unwrap_or(defaults::DEFAULT_MAX_TOOL_RETRIES as usize)
}

pub(super) fn signature_key_for(tool_name: &str, args: &serde_json::Value) -> String {
    let normalized = normalize_signature_args(tool_name, args);
    let args_str = serde_json::to_string(&normalized).unwrap_or_else(|_| args.to_string());
    format!("{}:{}", tool_name, args_str)
}

