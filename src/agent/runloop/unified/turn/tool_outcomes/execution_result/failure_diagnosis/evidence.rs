//! Sanitized, bounded evidence for failure diagnosis.

use serde_json::{Map, Value};
use vtcode_core::tools::registry::ToolExecutionError;

use super::{DIAGNOSIS_MAX_EVIDENCE_BYTES, DIAGNOSIS_MAX_FIELD_BYTES, DIAGNOSIS_TRUNCATION_MARKER};

const DIAGNOSTIC_VALUE_MAX_DEPTH: usize = 8;
const DIAGNOSTIC_VALUE_MAX_ITEMS: usize = 128;
const DIAGNOSTIC_VALUE_MAX_NODES: usize = 512;
const DIAGNOSTIC_VALUE_TRUNCATION_MARKER: &str = "[diagnostic value truncated]";

pub(super) fn build_output_evidence(tool_name: &str, args: &Value, output: &Value) -> String {
    // Do not call `maybe_inline_spooled_with_preview` here. That model-facing
    // helper intentionally clones and serializes the shaped payload, which is
    // acceptable for the normal response path but needlessly creates a large
    // intermediate when an untrusted tool field is huge. This traversal bounds
    // each value before serialization and retains only the existing in-memory
    // spool preview; it never reopens a spool file.
    let mut bounded_output = sanitize_diagnostic_value(output);
    if vtcode_core::tools::tool_intent::should_use_spool_reference_only(Some(tool_name), output) {
        apply_spool_reference_only(&mut bounded_output, output);
    }
    build_value_evidence(tool_name, args, &bounded_output)
}

pub(super) fn build_error_evidence(
    tool_name: &str,
    args: &Value,
    error: &ToolExecutionError,
    failure_kind: &str,
) -> String {
    let mut error_payload = Map::with_capacity(8);
    error_payload.insert("failure_kind".to_owned(), Value::String(bounded_field(failure_kind)));
    error_payload.insert("category".to_owned(), Value::String(error.category.user_label().to_owned()));
    error_payload.insert("message".to_owned(), Value::String(bounded_field(&error.message)));
    error_payload.insert("retryable".to_owned(), Value::Bool(error.retryable));
    error_payload.insert("is_recoverable".to_owned(), Value::Bool(error.is_recoverable));
    error_payload.insert(
        "recovery_suggestions".to_owned(),
        Value::Array(
            error
                .recovery_suggestions
                .iter()
                .take(DIAGNOSTIC_VALUE_MAX_ITEMS)
                .map(|suggestion| Value::String(bounded_field(suggestion.as_ref())))
                .collect(),
        ),
    );
    error_payload.insert("partial_state_possible".to_owned(), Value::Bool(error.partial_state_possible));
    error_payload.insert("rollback_performed".to_owned(), Value::Bool(error.rollback_performed));
    build_value_evidence(tool_name, args, &Value::Object(error_payload))
}

#[cfg(test)]
pub(super) fn build_evidence(tool_name: &str, args: &Value, result: &str) -> String {
    let args = serde_json::to_string(&sanitize_diagnostic_value(args)).unwrap_or_else(|_| "{}".to_string());
    let result = sanitize_diagnostic_text(result);
    build_evidence_text(tool_name, &args, &result)
}

fn build_value_evidence(tool_name: &str, args: &Value, result: &Value) -> String {
    let args = serde_json::to_string(&sanitize_diagnostic_value(args)).unwrap_or_else(|_| "{}".to_string());
    let result = serde_json::to_string(&sanitize_diagnostic_value(result)).unwrap_or_else(|_| "{}".to_string());
    build_evidence_text(tool_name, &args, &result)
}

fn build_evidence_text(tool_name: &str, args: &str, result: &str) -> String {
    let tool_name = bounded_field(tool_name);
    let raw = format!("tool={tool_name}\narguments={args}\nresult={result}");
    let ansi_free = vtcode_commons::ansi::strip_ansi(&raw);
    let sanitized = vtcode_commons::sanitizer::sanitize_provider_diagnostic(ansi_free.as_bytes());
    bound_text(&sanitized, DIAGNOSIS_MAX_EVIDENCE_BYTES)
}

fn apply_spool_reference_only(bounded_output: &mut Value, original: &Value) {
    let Some(object) = bounded_output.as_object_mut() else {
        return;
    };

    for key in [
        "output",
        "content",
        "stdout",
        "stderr",
        "matches",
        "results",
        "files",
        "entries",
        "spooled_bytes",
    ] {
        object.remove(key);
    }

    if let Some(stderr) = original
        .get("stderr")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        object.insert("stderr_preview".to_owned(), Value::String(bounded_field(stderr)));
    }
    if let Some(preview) = original
        .get("preview")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
    {
        object.insert("preview".to_owned(), Value::String(bounded_field(preview)));
    }
    if let Some(spool_path) = original
        .get("spool_path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
    {
        object.insert("spool_path".to_owned(), Value::String(bounded_field(spool_path)));
    }
    if let Some(spooled_bytes) = original.get("spooled_bytes").and_then(Value::as_u64) {
        object.insert("spooled_bytes".to_owned(), Value::from(spooled_bytes));
    }
    object.insert("result_ref_only".to_owned(), Value::Bool(true));
}

fn sanitize_diagnostic_value(value: &Value) -> Value {
    let mut remaining_nodes = DIAGNOSTIC_VALUE_MAX_NODES;
    sanitize_diagnostic_value_with_budget(value, 0, &mut remaining_nodes)
}

fn sanitize_diagnostic_value_with_budget(value: &Value, depth: usize, remaining_nodes: &mut usize) -> Value {
    if depth >= DIAGNOSTIC_VALUE_MAX_DEPTH || *remaining_nodes == 0 {
        return Value::String(DIAGNOSTIC_VALUE_TRUNCATION_MARKER.to_owned());
    }
    *remaining_nodes -= 1;

    match value {
        Value::Array(values) => {
            let mut sanitized = Vec::with_capacity(values.len().min(DIAGNOSTIC_VALUE_MAX_ITEMS));
            for value in values.iter().take(DIAGNOSTIC_VALUE_MAX_ITEMS) {
                sanitized.push(sanitize_diagnostic_value_with_budget(value, depth + 1, remaining_nodes));
            }
            if values.len() > DIAGNOSTIC_VALUE_MAX_ITEMS {
                sanitized.push(Value::String(DIAGNOSTIC_VALUE_TRUNCATION_MARKER.to_owned()));
            }
            Value::Array(sanitized)
        }
        Value::Object(object) => {
            let mut sanitized = Map::with_capacity(object.len().min(DIAGNOSTIC_VALUE_MAX_ITEMS));
            for (key, value) in object.iter().take(DIAGNOSTIC_VALUE_MAX_ITEMS) {
                let sensitive = is_sensitive_diagnostic_key(key);
                let key = bounded_field(key);
                let value = if sensitive {
                    Value::String("[REDACTED_SECRET]".to_owned())
                } else {
                    sanitize_diagnostic_value_with_budget(value, depth + 1, remaining_nodes)
                };
                sanitized.insert(key, value);
            }
            if object.len() > DIAGNOSTIC_VALUE_MAX_ITEMS {
                sanitized.insert(DIAGNOSTIC_VALUE_TRUNCATION_MARKER.to_owned(), Value::Bool(true));
            }
            Value::Object(sanitized)
        }
        Value::String(text) => Value::String(bounded_field(text)),
        _ => value.clone(),
    }
}

fn is_sensitive_diagnostic_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "apikey",
        "accesskey",
        "clientsecret",
        "credential",
        "privatekey",
        "token",
        "secret",
        "password",
        "auth",
        "authorization",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

pub(super) fn bounded_field(value: &str) -> String {
    let sanitized = sanitize_diagnostic_text(value);
    let mut single_line = String::with_capacity(sanitized.len());
    for character in sanitized.trim().chars() {
        match character {
            '\r' | '\n' | '\t' => single_line.push(' '),
            _ => single_line.push(character),
        }
    }
    bound_text(single_line.trim(), DIAGNOSIS_MAX_FIELD_BYTES)
}

/// Keep tool-provided text inside an evidence boundary even when it contains
/// markup that looks like the boundary itself.
pub(super) fn escape_untrusted_evidence(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub(super) fn safe_error_text(error: &anyhow::Error) -> String {
    // Preserve the bounded, sanitized cause chain (`{:#}` joins anyhow
    // contexts with ": ") so auth/param/transport failures stay
    // distinguishable. Raw bodies/credentials are still stripped by the
    // sanitizer and the result stays bounded to DIAGNOSIS_MAX_FIELD_BYTES.
    let sanitized = sanitize_diagnostic_text(&format!("{error:#}"));
    bound_text(sanitized.trim(), DIAGNOSIS_MAX_FIELD_BYTES)
}

fn sanitize_diagnostic_text(value: &str) -> String {
    // Sample before stripping ANSI so a hostile field cannot force a full-size
    // intermediate allocation. Run the sanitizer again after ANSI removal so
    // secrets split by escape sequences are still covered.
    let sampled = vtcode_commons::sanitizer::sanitize_provider_diagnostic(value.as_bytes());
    let ansi_free = vtcode_commons::ansi::strip_ansi(&sampled);
    vtcode_commons::sanitizer::sanitize_provider_diagnostic(ansi_free.as_bytes())
}

fn bound_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let suffix_len = DIAGNOSIS_TRUNCATION_MARKER.len();
    let content_limit = max_bytes.saturating_sub(suffix_len);
    let mut end = content_limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], DIAGNOSIS_TRUNCATION_MARKER)
}
