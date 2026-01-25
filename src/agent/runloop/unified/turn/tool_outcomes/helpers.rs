//! Helper utilities for tool outcome processing.

use vtcode_core::config::constants::defaults;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};

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

#[allow(dead_code)]
pub(super) fn classify_error_type(
    error_message: &str,
) -> vtcode_core::core::agent::error_recovery::ErrorType {
    let error_lower = error_message.to_lowercase();
    if error_lower.contains("timeout") || error_lower.contains("timed out") {
        vtcode_core::core::agent::error_recovery::ErrorType::Timeout
    } else if error_lower.contains("permission") || error_lower.contains("denied") {
        vtcode_core::core::agent::error_recovery::ErrorType::PermissionDenied
    } else if error_lower.contains("invalid")
        || error_lower.contains("argument")
        || error_lower.contains("missing")
    {
        vtcode_core::core::agent::error_recovery::ErrorType::InvalidArguments
    } else if error_lower.contains("not found") || error_lower.contains("no such") {
        vtcode_core::core::agent::error_recovery::ErrorType::ResourceNotFound
    } else if error_lower.contains("circuit") || error_lower.contains("breaker") {
        vtcode_core::core::agent::error_recovery::ErrorType::CircuitBreaker
    } else {
        vtcode_core::core::agent::error_recovery::ErrorType::Other
    }
}
