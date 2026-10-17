//! Bounded preview metadata for tool results (per-result limits, never aggregate).

use super::*;

pub(super) fn preview_spool_path(object: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<String> {
    object
        .and_then(|value| value.get("spool_path"))
        .and_then(serde_json::Value::as_str)
        .map(|path| bounded_diagnosis_preview(path, TOOL_PREVIEW_METADATA_STRING_LIMIT))
}

pub(super) fn preview_byte_count(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    fallback_len: usize,
) -> u64 {
    object
        .and_then(|value| {
            [
                "original_bytes",
                "spooled_bytes",
                "total_output_bytes",
                "total_bytes",
                "output_bytes",
                "byte_count",
                "bytes",
            ]
            .into_iter()
            .find_map(|key| value.get(key).and_then(serde_json::Value::as_u64))
        })
        .unwrap_or_else(|| u64::try_from(fallback_len).unwrap_or(u64::MAX))
}

pub(super) fn preview_completion_state(object: Option<&serde_json::Map<String, serde_json::Value>>) -> &'static str {
    let Some(value) = object else {
        return "unknown";
    };
    if value.get("spool_pending").and_then(serde_json::Value::as_bool) == Some(true) {
        return "pending";
    }
    let exited = value.get("spool_complete").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("is_exited").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("exit_code").and_then(serde_json::Value::as_i64).is_some()
        || value.get("exit_code").and_then(serde_json::Value::as_u64).is_some()
        || value.get("success").and_then(serde_json::Value::as_bool) == Some(true)
        || value.get("command_success").and_then(serde_json::Value::as_bool) == Some(true)
        || matches!(value.get("status").and_then(serde_json::Value::as_str), Some("completed" | "success"))
        || matches!(value.get("outcome").and_then(serde_json::Value::as_str), Some("completed" | "success"));
    if exited { "complete" } else { "unknown" }
}

/// Prefer one substantive body for a per-result head/tail excerpt. Outcome
/// metadata is preserved separately; identical producer aliases are not copied
/// into multiple preview fields.
const TOOL_PREVIEW_BODY_FIELDS: [&str; 5] = ["output", "preview", "content", "stdout", "stderr"];

pub(super) fn bounded_tool_preview_metadata(tool_name: Option<&str>, content: &str) -> String {
    let parsed = (content.len() <= TOOL_PREVIEW_METADATA_PARSE_LIMIT_BYTES)
        .then(|| serde_json::from_str::<serde_json::Value>(content).ok())
        .flatten();
    let object = parsed.as_ref().and_then(serde_json::Value::as_object);

    let spool_path = preview_spool_path(object);
    let byte_count = preview_byte_count(object, content.len());
    let completion_state = preview_completion_state(object);

    let diagnosis = object
        .and_then(|value| value.get("diagnosis"))
        .and_then(serde_json::Value::as_object)
        .map(|diagnosis| {
            let mut bounded = serde_json::Map::new();
            for key in ["observed", "likely_cause", "next_action"] {
                if let Some(value) = diagnosis.get(key).and_then(serde_json::Value::as_str) {
                    bounded.insert(
                        key.to_string(),
                        serde_json::Value::String(bounded_diagnosis_preview(value, TOOL_PREVIEW_METADATA_STRING_LIMIT)),
                    );
                }
            }
            serde_json::Value::Object(bounded)
        });

    let note = if spool_path.is_some() {
        "This result has a bounded preview; complete output remains in the spool and current-session tool-output viewer."
    } else {
        "This result has a bounded preview; use targeted extraction for additional evidence."
    };
    let mut metadata = serde_json::json!({
        "tool": tool_name.map(|name| bounded_preview_string(name, TOOL_PREVIEW_METADATA_STRING_LIMIT)),
        "spool_path": spool_path,
        "byte_count": byte_count,
        "completion_state": completion_state,
        "preview_truncated": true,
        "note": note,
    });
    if let Some(diagnosis) = diagnosis {
        metadata["diagnosis"] = diagnosis;
    }
    if let Some(object) = object {
        if let Some(error) = object.get("error")
            && let Some(bounded) = bounded_tool_failure_metadata(error)
        {
            metadata["error"] = bounded;
        }
        for key in [
            "error_summary",
            "original_error",
            "message",
            "stderr",
            "stderr_preview",
            "critical_note",
            "error_class",
            "category",
            "retry_summary",
            "recovery_suggestions",
        ] {
            let Some(value) = object.get(key) else {
                continue;
            };
            if let Some(bounded) = bounded_tool_failure_metadata(value) {
                metadata[key] = bounded;
            }
        }
        for key in [
            "success",
            "exit_code",
            "command_success",
            "blocked",
            "verification_required",
            "failure_kind",
            "status",
            "outcome",
            "output_truncated",
            "has_more",
            "next_action",
            "retryable",
            "is_exited",
            "spool_complete",
            "spool_pending",
            // Exec continuity + verifier metadata: session-vtcode-20260913T074747Z
            // stubs dropped `session_id`/`command`/`backend`, so the model could
            // neither continue pipe sessions via `write_stdin` nor tell which
            // verifier produced the stub. These scalars are bounded (strings
            // capped at 512 chars) and never carry payload bodies.
            "command",
            "session_id",
            "backend",
            "working_directory",
            "process_id",
            "wall_time",
            "waited_seconds",
            "total_output_bytes",
            "spooled_bytes",
            "spool_line_count",
            "matched_count",
            "truncated",
            "content_type",
        ] {
            let Some(value) = object.get(key) else {
                continue;
            };
            let bounded = match value {
                serde_json::Value::String(value) => {
                    serde_json::Value::String(bounded_diagnosis_preview(value, TOOL_PREVIEW_METADATA_STRING_LIMIT))
                }
                serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Null => value.clone(),
                _ => continue,
            };
            metadata[key] = bounded;
        }
    }
    let body = object
        .and_then(|value| {
            TOOL_PREVIEW_BODY_FIELDS
                .iter()
                .find_map(|field| value.get(*field).and_then(serde_json::Value::as_str))
        })
        .unwrap_or(content);
    metadata["preview"] =
        serde_json::Value::String(vtcode_commons::ansi::strip_ansi(&vtcode_commons::sanitizer::redact_secrets(
            vtcode_commons::preview::condense_text_bytes(body, 2 * 1024, 2 * 1024),
        )));
    metadata.to_string()
}

pub(super) fn bounded_tool_failure_metadata(value: &serde_json::Value) -> Option<serde_json::Value> {
    bounded_tool_failure_metadata_at_depth(value, TOOL_PREVIEW_METADATA_MAX_DEPTH)
}

pub(super) fn bounded_tool_failure_metadata_at_depth(
    value: &serde_json::Value,
    depth: usize,
) -> Option<serde_json::Value> {
    if depth == 0 {
        return None;
    }

    match value {
        serde_json::Value::String(value) => {
            Some(serde_json::Value::String(bounded_diagnosis_preview(value, TOOL_PREVIEW_METADATA_STRING_LIMIT)))
        }
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Null => Some(value.clone()),
        serde_json::Value::Array(values) => {
            let bounded = values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .take(4)
                .map(|value| {
                    serde_json::Value::String(bounded_diagnosis_preview(value, TOOL_PREVIEW_METADATA_STRING_LIMIT))
                })
                .collect::<Vec<_>>();
            (!bounded.is_empty()).then_some(serde_json::Value::Array(bounded))
        }
        serde_json::Value::Object(object) => {
            let mut bounded = serde_json::Map::new();
            for key in [
                "tool_name",
                "error_type",
                "category",
                "message",
                "original_error",
                "retryable",
                "is_recoverable",
                "partial_state_possible",
                "rollback_performed",
                "circuit_breaker_impact",
                "retry_delay_ms",
                "retry_after_ms",
            ] {
                let Some(value) = object.get(key) else {
                    continue;
                };
                if let Some(value) = bounded_tool_failure_metadata_at_depth(value, depth - 1) {
                    bounded.insert(key.to_string(), value);
                }
            }
            if let Some(value) = object.get("recovery_suggestions")
                && let Some(value) = bounded_tool_failure_metadata_at_depth(value, depth - 1)
            {
                bounded.insert("recovery_suggestions".to_string(), value);
            }
            (!bounded.is_empty()).then_some(serde_json::Value::Object(bounded))
        }
    }
}

pub(super) fn bounded_preview_string(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

pub(super) fn bounded_diagnosis_preview(value: &str, limit: usize) -> String {
    let ansi_free = vtcode_commons::ansi::strip_ansi(value);
    let sanitized = vtcode_commons::sanitizer::sanitize_provider_diagnostic(ansi_free.as_bytes());
    bounded_preview_string(sanitized.trim(), limit)
}
