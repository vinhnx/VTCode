use serde_json::Value;

use crate::tools::output_limits::OUTPUT_PREVIEW_CHARS_PER_TOKEN;

fn has_text_output_field(value: &Value) -> bool {
    ["raw_output", "output", "stdout"]
        .into_iter()
        .any(|field| value.get(field).is_some_and(Value::is_string))
}

fn is_pty_output_response(tool_name: &str, value: &Value) -> bool {
    crate::tools::tool_intent::is_command_session_tool(tool_name) && has_text_output_field(value)
}

fn output_field_bytes(value: &Value) -> usize {
    ["raw_output", "output", "stdout"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn output_budget_bytes(max_output_tokens: usize) -> usize {
    max_output_tokens.saturating_mul(OUTPUT_PREVIEW_CHARS_PER_TOKEN)
}

pub(super) fn preview_budget_bytes(max_output_tokens: usize) -> usize {
    output_budget_bytes(max_output_tokens).max(1)
}

pub(super) fn should_force_spool(
    tool_name: &str,
    value: &Value,
    is_mcp: bool,
    spooling_enabled: bool,
    max_output_tokens: usize,
) -> bool {
    if !spooling_enabled {
        return false;
    }
    let is_pty_output = is_pty_output_response(tool_name, value);
    let no_spool = value.get("no_spool").and_then(Value::as_bool).unwrap_or(false);

    // PTY/session responses contain substantial session metadata. Decide
    // whether their output is capped from the output field itself rather than
    // forcing a spool because the surrounding JSON envelope exceeds a small
    // caller-selected token cap.
    if is_pty_output {
        if no_spool {
            return false;
        }
        if value.get("truncated").and_then(Value::as_bool).unwrap_or(false) {
            return true;
        }
        let output_bytes = output_field_bytes(value);
        return output_bytes > output_budget_bytes(max_output_tokens);
    }

    if value.to_string().len() > output_budget_bytes(max_output_tokens) {
        return true;
    }
    if no_spool {
        return false;
    }
    if is_mcp {
        return false;
    }
    false
}

pub(super) fn should_keep_inline_pty_output(tool_name: &str, value: &Value, max_output_tokens: usize) -> bool {
    is_pty_output_response(tool_name, value) && output_field_bytes(value) <= output_budget_bytes(max_output_tokens)
}

pub(super) fn limit_output_preview(mut value: Value, max_output_tokens: usize) -> Value {
    let max_preview_bytes = preview_budget_bytes(max_output_tokens);
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    let mut preview_truncated = false;
    for field in ["raw_output", "output", "stdout", "stderr", "content", "preview"] {
        let Some(text) = object.get(field).and_then(Value::as_str) else {
            continue;
        };
        if text.len() > max_preview_bytes {
            object.insert(
                field.to_string(),
                Value::String(vtcode_commons::formatting::truncate_byte_budget(text, max_preview_bytes, "")),
            );
            preview_truncated = true;
        }
    }
    if preview_truncated {
        object.insert("truncated".to_string(), Value::Bool(true));
        object.insert("preview_truncated".to_string(), Value::Bool(true));
    }
    value
}

pub(super) fn limit_spooled_preview(value: Value, max_output_tokens: usize) -> Value {
    if value.get("spool_path").and_then(Value::as_str).is_none() {
        return value;
    }
    limit_output_preview(value, max_output_tokens)
}
