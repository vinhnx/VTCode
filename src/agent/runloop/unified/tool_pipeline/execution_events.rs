use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use serde_json::Value;
use vtcode_core::core::agent::events::{
    error_item_completed_event, tool_invocation_completed_event, tool_output_completed_event,
};
use vtcode_core::exec::events::ToolCallStatus;

use super::status::ToolExecutionStatus;

struct ToolOutputEventPayload {
    aggregated_output: String,
    spool_path: Option<String>,
}

fn aggregated_output_from_value(output: &Value) -> ToolOutputEventPayload {
    if let Some(spool_path) = output.get("spool_path").and_then(Value::as_str) {
        return ToolOutputEventPayload {
            aggregated_output: String::new(),
            spool_path: Some(spool_path.to_string()),
        };
    }

    let mut parts = Vec::new();

    for key in ["output", "stdout", "stderr", "content"] {
        if let Some(text) = output.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() && !parts.iter().any(|part| part == trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }

    ToolOutputEventPayload {
        aggregated_output: parts.join("\n"),
        spool_path: None,
    }
}

pub(super) fn emit_tool_completion_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_execution_started: bool,
    tool_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    aggregated_output: impl Into<String>,
) {
    if !tool_started_emitted {
        return;
    }

    if let Some(emitter) = harness_emitter {
        let aggregated_output = aggregated_output.into();
        let _ = emitter.emit(tool_invocation_completed_event(
            tool_item_id.to_string(),
            tool_name,
            Some(args),
            Some(tool_call_id),
            status.clone(),
        ));
        if tool_execution_started {
            let _ = emitter.emit(tool_output_completed_event(
                tool_item_id.to_string(),
                Some(tool_call_id),
                status,
                exit_code,
                spool_path,
                aggregated_output,
            ));
        } else if !aggregated_output.is_empty() {
            let _ = emitter.emit(error_item_completed_event(
                format!("{tool_item_id}:error"),
                aggregated_output,
            ));
        }
    }
}

pub(super) fn emit_tool_completion_for_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_execution_started: bool,
    tool_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
    tool_status: &ToolExecutionStatus,
) {
    let (status, exit_code, output_payload) = match tool_status {
        ToolExecutionStatus::Success {
            output,
            command_success,
            ..
        } => (
            if *command_success {
                ToolCallStatus::Completed
            } else {
                ToolCallStatus::Failed
            },
            output
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok()),
            aggregated_output_from_value(output),
        ),
        ToolExecutionStatus::Failure { error } => (
            ToolCallStatus::Failed,
            None,
            ToolOutputEventPayload {
                aggregated_output: error.to_string(),
                spool_path: None,
            },
        ),
        ToolExecutionStatus::Timeout { error } => (
            ToolCallStatus::Failed,
            None,
            ToolOutputEventPayload {
                aggregated_output: error.message.clone(),
                spool_path: None,
            },
        ),
        ToolExecutionStatus::Cancelled => (
            ToolCallStatus::Failed,
            None,
            ToolOutputEventPayload {
                aggregated_output: "Tool execution cancelled".to_string(),
                spool_path: None,
            },
        ),
    };
    emit_tool_completion_status(
        harness_emitter,
        tool_started_emitted,
        tool_execution_started,
        tool_item_id,
        tool_call_id,
        tool_name,
        args,
        status,
        exit_code,
        output_payload.spool_path.as_deref(),
        output_payload.aggregated_output,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn aggregates_command_output_without_duplicates() {
        let output = json!({
            "output": "same",
            "stdout": "same",
            "stderr": "warn"
        });

        let payload = aggregated_output_from_value(&output);
        assert_eq!(payload.aggregated_output, "same\nwarn");
        assert_eq!(payload.spool_path, None);
    }

    #[test]
    fn includes_content_when_command_stream_fields_absent() {
        let output = json!({
            "content": "file body",
            "path": "README.md"
        });

        let payload = aggregated_output_from_value(&output);
        assert_eq!(payload.aggregated_output, "file body");
        assert_eq!(payload.spool_path, None);
    }

    #[test]
    fn prefers_spool_reference_over_inline_output() {
        let output = json!({
            "output": "preview",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt"
        });

        let payload = aggregated_output_from_value(&output);
        assert_eq!(payload.aggregated_output, "");
        assert_eq!(
            payload.spool_path.as_deref(),
            Some(".vtcode/context/tool_outputs/run-1.txt")
        );
    }

    #[test]
    fn non_zero_command_marks_failed_completion() {
        let status = ToolExecutionStatus::Success {
            output: json!({
                "stdout": "boom",
                "exit_code": 1
            }),
            stdout: Some("boom".to_string()),
            modified_files: vec![],
            command_success: false,
        };

        let (event_status, exit_code, output_payload) = match &status {
            ToolExecutionStatus::Success {
                output,
                command_success,
                ..
            } => (
                if *command_success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                },
                output
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok()),
                aggregated_output_from_value(output),
            ),
            _ => unreachable!("success status expected"),
        };

        assert_eq!(event_status, ToolCallStatus::Failed);
        assert_eq!(exit_code, Some(1));
        assert_eq!(output_payload.aggregated_output, "boom");
        assert_eq!(output_payload.spool_path, None);
    }
}
