use anyhow::Error;
use serde_json::{Value, json};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::request_user_input::normalize_request_user_input_fallback_args;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::compact_model_tool_payload;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::PreparedToolCall;

pub(super) fn recovery_fallback_for_tool(tool_name: &str, args: &Value) -> Option<(String, Value)> {
    match tool_name {
        tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_unified_search_args(args);
            let action = tool_intent::unified_search_action(&normalized).unwrap_or("grep");
            if action.eq_ignore_ascii_case("list") {
                Some((
                    tool_names::UNIFIED_SEARCH.to_string(),
                    json!({
                        "action": "list",
                        "path": normalized.get("path").and_then(|v| v.as_str()).unwrap_or("."),
                        "mode": normalized.get("mode").and_then(|v| v.as_str()).unwrap_or("list")
                    }),
                ))
            } else {
                None
            }
        }
        "list files" => Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "list",
                "path": args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
            }),
        )),
        "search text" => None,
        tool_names::READ_FILE | "read file" | "repo_browser.read_file" => {
            let parent_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .and_then(|path| std::path::Path::new(path).parent())
                .and_then(|path| path.to_str())
                .unwrap_or(".");
            Some((
                tool_names::UNIFIED_SEARCH.to_string(),
                json!({
                    "action": "list",
                    "path": parent_path
                }),
            ))
        }
        _ => Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "list",
                "path": "."
            }),
        )),
    }
}

pub(super) fn build_validation_error_content_with_fallback(
    error: String,
    validation_stage: &'static str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> String {
    let is_recoverable = fallback_tool.is_some();
    let loop_detected = validation_stage == "loop_detection";
    let next_action = if is_recoverable {
        "Retry with fallback_tool_args."
    } else {
        "Fix tool arguments to match the schema."
    };
    let mut payload = serde_json::json!({
        "error": error,
        "failure_kind": "validation",
        "error_class": "invalid_arguments",
        "validation_stage": validation_stage,
        "retryable": false,
        "is_recoverable": is_recoverable,
        "loop_detected": loop_detected,
        "next_action": next_action,
    });
    if let Some(obj) = payload.as_object_mut() {
        if let Some(tool) = fallback_tool {
            obj.insert("fallback_tool".to_string(), Value::String(tool));
        }
        if let Some(args) = fallback_tool_args {
            obj.insert("fallback_tool_args".to_string(), args);
        }
    }
    compact_model_tool_payload(payload).to_string()
}

pub(super) fn preflight_validation_fallback(
    tool_name: &str,
    args_val: &Value,
    error: &Error,
) -> Option<(String, Value)> {
    let error_text = error.to_string();
    let is_request_user_input = tool_name == tool_names::REQUEST_USER_INPUT
        || error_text.contains("tool 'request_user_input'")
        || error_text.contains("for 'request_user_input'");
    if is_request_user_input {
        let normalized = normalize_request_user_input_fallback_args(args_val)?;
        if normalized == *args_val {
            return None;
        }
        return Some((tool_names::REQUEST_USER_INPUT.to_string(), normalized));
    }

    let is_unified_search = tool_name == tool_names::UNIFIED_SEARCH
        || error_text.contains("tool 'unified_search'")
        || error_text.contains("for 'unified_search'");
    if is_unified_search {
        let mut normalized = tool_intent::normalize_unified_search_args(args_val);
        let inferred_pattern = normalized
            .get("pattern")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                normalized
                    .get("keyword")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                normalized
                    .get("query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });

        if let Some(obj) = normalized.as_object_mut() {
            let action = obj
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if action.eq_ignore_ascii_case("read") {
                if inferred_pattern.is_some() {
                    obj.insert("action".to_string(), Value::String("grep".to_string()));
                } else {
                    obj.insert("action".to_string(), Value::String("list".to_string()));
                }
            }

            if obj.get("action").and_then(Value::as_str) == Some("grep")
                && obj.get("pattern").is_none()
                && let Some(pattern) = inferred_pattern
            {
                obj.insert("pattern".to_string(), Value::String(pattern));
            }
        }

        if normalized != *args_val && normalized.get("action").is_some() {
            return Some((tool_names::UNIFIED_SEARCH.to_string(), normalized));
        }
    }

    let is_unified_file = tool_name == tool_names::UNIFIED_FILE
        || error_text.contains("tool 'unified_file'")
        || error_text.contains("for 'unified_file'");
    if !is_unified_file {
        return None;
    }

    if tool_intent::unified_file_action_is(args_val, "list") {
        let path = args_val
            .get("path")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(".");
        return Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            json!({
                "action": "list",
                "path": path,
            }),
        ));
    }

    tool_intent::remap_unified_file_command_args_to_unified_exec(args_val)
        .map(|args| (tool_names::UNIFIED_EXEC.to_string(), args))
}

pub(super) fn try_recover_preflight_with_fallback(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &Value,
    error: &Error,
) -> Option<PreparedToolCall> {
    let (recovered_tool_name, recovered_args) =
        preflight_validation_fallback(tool_name, args_val, error)?;
    match ctx
        .tool_registry
        .admit_public_tool_call(&recovered_tool_name, &recovered_args)
    {
        Ok(prepared) => Some(prepared),
        Err(recovery_err) => {
            tracing::debug!(
                tool = tool_name,
                original_error = %error,
                recovered_tool = %recovered_tool_name,
                recovery_error = %recovery_err,
                "Preflight recovery fallback failed"
            );
            None
        }
    }
}
