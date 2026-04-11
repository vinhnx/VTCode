use vtcode_commons::ErrorCategory;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::ToolExecutionError;

use super::helpers::check_is_argument_error;
use super::response_content::compact_model_tool_payload;

const MAX_ERROR_MESSAGE_CHARS: usize = 420;
const MAX_FALLBACK_ARGS_PREVIEW_CHARS: usize = 140;
const MAX_FALLBACK_ARGS_INLINE_CHARS: usize = 240;

pub(super) fn is_blocked_or_denied_failure(error: &str) -> bool {
    if agent_execution::is_plan_mode_denial(error) {
        return true;
    }

    let lowered = error.to_ascii_lowercase();
    [
        "tool permission denied",
        "policy violation:",
        "safety validation failed",
        "tool argument validation failed",
        "not allowed in plan mode",
        "only available when plan mode is active",
        "compatibility alias",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

pub(super) fn truncate_text_for_model(value: &str, max_chars: usize) -> (String, bool) {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return (value.to_string(), false);
    }

    const MARKER: &str = " ... [truncated] ... ";
    let marker_chars = MARKER.chars().count();
    if max_chars <= marker_chars + 16 {
        let mut truncated = value.chars().take(max_chars).collect::<String>();
        truncated.push_str(" [truncated]");
        return (truncated, true);
    }

    let available = max_chars.saturating_sub(marker_chars);
    let head_chars = (available * 2) / 3;
    let tail_chars = available.saturating_sub(head_chars);
    let head = value.chars().take(head_chars).collect::<String>();
    let tail = value
        .chars()
        .skip(total_chars.saturating_sub(tail_chars))
        .collect::<String>();
    let mut truncated = String::with_capacity(max_chars + 20);
    truncated.push_str(&head);
    truncated.push_str(MARKER);
    truncated.push_str(&tail);
    (truncated, true)
}

fn compact_json_preview(serialized: &str, max_chars: usize) -> String {
    let (preview, _) = truncate_text_for_model(serialized, max_chars);
    preview
}

pub(super) fn serialize_json_for_model(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

fn should_inline_fallback_args(serialized_args: &str) -> bool {
    serialized_args.chars().count() <= MAX_FALLBACK_ARGS_INLINE_CHARS
}

fn push_fallback_args(
    payload: &mut serde_json::Value,
    args: serde_json::Value,
    args_preview: &str,
    inline_full_args: bool,
) {
    if let Some(obj) = payload.as_object_mut() {
        if inline_full_args {
            obj.insert("fallback_tool_args".to_string(), args);
        } else {
            obj.insert(
                "fallback_tool_args_preview".to_string(),
                serde_json::Value::String(args_preview.to_string()),
            );
            obj.insert(
                "fallback_tool_args_truncated".to_string(),
                serde_json::Value::Bool(true),
            );
        }
    }
}

fn push_error_truncation_flag(payload: &mut serde_json::Value, error_truncated: bool) {
    if !error_truncated {
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_truncated".to_string(), serde_json::Value::Bool(true));
    }
}

fn fallback_args_preview_and_inline(
    fallback_tool_args: &Option<serde_json::Value>,
) -> (Option<String>, bool) {
    let Some(args) = fallback_tool_args else {
        return (None, false);
    };
    let serialized = serialize_json_for_model(args);
    let preview = compact_json_preview(&serialized, MAX_FALLBACK_ARGS_PREVIEW_CHARS);
    (Some(preview), should_inline_fallback_args(&serialized))
}

fn failure_guidance(
    error_msg: &str,
    failure_kind: &'static str,
) -> (&'static str, bool, &'static str) {
    if failure_kind == "timeout" {
        return (
            "timeout",
            true,
            "Retry with smaller scope or higher timeout.",
        );
    }

    if check_is_argument_error(error_msg) {
        return (
            "invalid_arguments",
            true,
            "Fix tool arguments to match the schema.",
        );
    }

    if is_blocked_or_denied_failure(error_msg) {
        return (
            "policy_blocked",
            false,
            "Switch to an allowed tool or mode.",
        );
    }

    (
        "execution_failure",
        true,
        "Try an alternative tool or narrower scope.",
    )
}

fn structured_failure_guidance(
    error: &ToolExecutionError,
    failure_kind: &'static str,
) -> (&'static str, bool, &'static str) {
    if failure_kind == "timeout" || matches!(error.category, ErrorCategory::Timeout) {
        return (
            "timeout",
            true,
            "Retry with smaller scope or a higher timeout.",
        );
    }

    if matches!(error.category, ErrorCategory::InvalidParameters)
        || check_is_argument_error(&error.message)
    {
        return (
            "invalid_arguments",
            true,
            "Fix the tool arguments to match the schema.",
        );
    }

    if matches!(error.category, ErrorCategory::Authentication) {
        return (
            "authentication_failed",
            false,
            "Verify your credentials or choose a different provider.",
        );
    }

    if matches!(
        error.category,
        ErrorCategory::PermissionDenied
            | ErrorCategory::PolicyViolation
            | ErrorCategory::PlanModeViolation
    ) || is_blocked_or_denied_failure(&error.message)
    {
        return (
            "policy_blocked",
            false,
            "Switch to an allowed tool or mode.",
        );
    }

    if matches!(error.category, ErrorCategory::CircuitOpen) {
        return (
            "service_temporarily_unavailable",
            true,
            "Wait briefly, then retry or use a different tool.",
        );
    }

    (
        "execution_failure",
        error.is_recoverable,
        "Try an alternative tool or narrower scope.",
    )
}

pub(super) fn format_structured_tool_error_for_user(
    tool_name: &str,
    error: &ToolExecutionError,
) -> (String, Option<String>) {
    let sanitized = crate::agent::runloop::unified::turn::turn_helpers::sanitize_error_for_display(
        &error.message,
    );
    let mut primary = format!(
        "Tool '{}' failed ({}): {}",
        tool_name,
        error.category.user_label(),
        sanitized
    );

    if error.rollback_performed {
        primary.push_str(" Any partial changes were rolled back.");
    } else if error.partial_state_possible {
        primary.push_str(" Partial changes may still exist.");
    }

    if let Some(retry_summary) = error.retry_summary() {
        primary.push(' ');
        primary.push_str(&retry_summary);
    }

    let hint = if error.recovery_suggestions.is_empty() {
        None
    } else {
        Some(format!(
            "Hint: {}",
            error
                .recovery_suggestions
                .iter()
                .map(|suggestion| suggestion.as_ref())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    };

    (primary, hint)
}

pub(crate) fn build_error_content(
    error_msg: String,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<serde_json::Value>,
    failure_kind: &'static str,
) -> serde_json::Value {
    let (error_text, error_truncated) =
        truncate_text_for_model(&error_msg, MAX_ERROR_MESSAGE_CHARS);
    let (error_class, is_recoverable, next_action) = failure_guidance(&error_msg, failure_kind);
    let (args_preview, inline_full_args) = fallback_args_preview_and_inline(&fallback_tool_args);

    if let Some(tool) = fallback_tool {
        let mut payload = serde_json::json!({
            "error": error_text,
            "failure_kind": failure_kind,
            "error_class": error_class,
            "is_recoverable": is_recoverable,
            "next_action": next_action,
            "fallback_tool": tool,
        });
        push_error_truncation_flag(&mut payload, error_truncated);
        if let Some(args) = fallback_tool_args {
            push_fallback_args(
                &mut payload,
                args,
                args_preview.as_deref().unwrap_or("<invalid-json>"),
                inline_full_args,
            );
        }
        compact_model_tool_payload(payload)
    } else {
        let mut payload = serde_json::json!({
            "error": error_text,
            "failure_kind": failure_kind,
            "error_class": error_class,
            "is_recoverable": is_recoverable,
            "next_action": next_action,
        });
        push_error_truncation_flag(&mut payload, error_truncated);
        compact_model_tool_payload(payload)
    }
}

pub(super) fn build_structured_error_content(
    error: &ToolExecutionError,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<serde_json::Value>,
    failure_kind: &'static str,
) -> serde_json::Value {
    let (error_summary, error_truncated) =
        truncate_text_for_model(&error.user_message(), MAX_ERROR_MESSAGE_CHARS);
    let (error_class, is_recoverable, default_next_action) =
        structured_failure_guidance(error, failure_kind);
    let (args_preview, inline_full_args) = fallback_args_preview_and_inline(&fallback_tool_args);
    let retry_summary = error.retry_summary();
    let next_action = error
        .recovery_suggestions
        .first()
        .map(|suggestion| suggestion.as_ref())
        .unwrap_or(default_next_action);
    let mut payload = error.to_json_value();

    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "error_summary".to_string(),
            serde_json::Value::String(error_summary),
        );
        obj.insert(
            "failure_kind".to_string(),
            serde_json::Value::String(failure_kind.to_string()),
        );
        obj.insert(
            "error_class".to_string(),
            serde_json::Value::String(error_class.to_string()),
        );
        obj.insert("category".to_string(), serde_json::json!(error.category));
        obj.insert(
            "is_recoverable".to_string(),
            serde_json::Value::Bool(is_recoverable),
        );
        obj.insert(
            "retryable".to_string(),
            serde_json::Value::Bool(error.retryable),
        );
        obj.insert(
            "partial_state_possible".to_string(),
            serde_json::Value::Bool(error.partial_state_possible),
        );
        obj.insert(
            "rollback_performed".to_string(),
            serde_json::Value::Bool(error.rollback_performed),
        );
        obj.insert(
            "circuit_breaker_impact".to_string(),
            serde_json::Value::Bool(error.circuit_breaker_impact),
        );
        if is_recoverable {
            obj.insert(
                "next_action".to_string(),
                serde_json::Value::String(next_action.to_string()),
            );
        }
        if !error.recovery_suggestions.is_empty() {
            obj.insert(
                "recovery_suggestions".to_string(),
                serde_json::json!(
                    error
                        .recovery_suggestions
                        .iter()
                        .map(|suggestion| suggestion.as_ref())
                        .collect::<Vec<_>>()
                ),
            );
        }
        if let Some(summary) = retry_summary {
            obj.insert(
                "retry_summary".to_string(),
                serde_json::Value::String(summary),
            );
        }
        if let Some(retry_delay_ms) = error.retry_delay_ms {
            obj.insert(
                "retry_delay_ms".to_string(),
                serde_json::Value::Number(retry_delay_ms.into()),
            );
        }
        if let Some(retry_after_ms) = error.retry_after_ms {
            obj.insert(
                "retry_after_ms".to_string(),
                serde_json::Value::Number(retry_after_ms.into()),
            );
        }
        if let Some(debug_context) = &error.debug_context {
            obj.insert(
                "debug_context".to_string(),
                serde_json::to_value(debug_context).unwrap_or(serde_json::Value::Null),
            );
        }
    }

    if let Some(tool) = fallback_tool {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("fallback_tool".to_string(), serde_json::Value::String(tool));
        }
        if let Some(args) = fallback_tool_args {
            push_fallback_args(
                &mut payload,
                args,
                args_preview.as_deref().unwrap_or("<invalid-json>"),
                inline_full_args,
            );
        }
    }

    push_error_truncation_flag(&mut payload, error_truncated);
    compact_model_tool_payload(payload)
}

fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn extract_pty_session_id_from_error(error_msg: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_msg.find(marker)? + marker.len();
    let rest = &error_msg[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn extract_patch_target_path_from_error(error_msg: &str) -> Option<String> {
    let markers = [
        "failed to locate expected lines in ",
        "failed to locate expected text in ",
    ];
    for marker in markers {
        let Some(start) = error_msg.find(marker) else {
            continue;
        };
        let rest = &error_msg[start + marker.len()..];
        for quote in ['\'', '"'] {
            let quote_s = quote.to_string();
            let Some(stripped) = rest.strip_prefix(&quote_s) else {
                continue;
            };
            let Some(end_idx) = stripped.find(quote) else {
                continue;
            };
            let path = stripped[..end_idx].trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

pub(super) fn fallback_from_error(
    tool_name: &str,
    error_msg: &str,
) -> Option<(String, serde_json::Value)> {
    if tool_name == tool_names::UNIFIED_SEARCH
        && error_msg
            .to_ascii_lowercase()
            .contains("invalid action: read")
    {
        return Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::json!({
                "action": "list",
                "path": "."
            }),
        ));
    }

    if matches!(
        tool_name,
        tool_names::UNIFIED_FILE | tool_names::READ_FILE | "read file" | "repo_browser.read_file"
    ) && let Some(session_id) = extract_pty_session_id_from_error(error_msg)
    {
        return Some((
            tool_names::UNIFIED_EXEC.to_string(),
            serde_json::json!({
                "action": "poll",
                "session_id": session_id
            }),
        ));
    }

    if matches!(
        tool_name,
        tool_names::APPLY_PATCH | tool_names::UNIFIED_FILE | "apply patch"
    ) && let Some(path) = extract_patch_target_path_from_error(error_msg)
    {
        return Some((
            tool_names::READ_FILE.to_string(),
            serde_json::json!({
                "path": path,
                "offset": 1,
                "limit": 120
            }),
        ));
    }

    if matches!(
        tool_name,
        tool_names::TASK_TRACKER | tool_names::PLAN_TASK_TRACKER
    ) {
        let lower = error_msg.to_ascii_lowercase();
        if lower.contains("required for 'update'")
            || lower.contains("required for \"update\"")
            || lower.contains("invalid task_tracker arguments")
            || lower.contains("invalid plan_task_tracker arguments")
        {
            return Some((
                tool_name.to_string(),
                serde_json::json!({
                    "action": "list"
                }),
            ));
        }
    }

    None
}
