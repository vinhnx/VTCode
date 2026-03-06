use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_invocation_completed_event, tool_output_completed_event,
};
use serde_json::Value;
use vtcode_core::exec::events::ToolCallStatus;

use super::status::ToolExecutionStatus;

fn aggregated_output_from_value(output: &Value) -> String {
    let mut parts = Vec::new();

    for key in ["output", "stdout", "stderr", "content"] {
        if let Some(text) = output.get(key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() && !parts.iter().any(|part| part == trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }

    parts.join("\n")
}

pub(super) fn emit_tool_completion_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    args: &Value,
    status: ToolCallStatus,
    exit_code: Option<i32>,
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
            args,
            status.clone(),
        ));
        let _ = emitter.emit(tool_output_completed_event(
            tool_item_id.to_string(),
            status,
            exit_code,
            aggregated_output,
        ));
    }
}

pub(super) fn emit_tool_completion_for_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    args: &Value,
    tool_status: &ToolExecutionStatus,
) {
    let (status, exit_code, aggregated_output) = match tool_status {
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
        ToolExecutionStatus::Failure { error } => (ToolCallStatus::Failed, None, error.to_string()),
        ToolExecutionStatus::Timeout { error } => {
            (ToolCallStatus::Failed, None, error.message.clone())
        }
        ToolExecutionStatus::Cancelled => (
            ToolCallStatus::Failed,
            None,
            "Tool execution cancelled".to_string(),
        ),
    };
    emit_tool_completion_status(
        harness_emitter,
        tool_started_emitted,
        tool_item_id,
        tool_name,
        args,
        status,
        exit_code,
        aggregated_output,
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

        assert_eq!(aggregated_output_from_value(&output), "same\nwarn");
    }

    #[test]
    fn includes_content_when_command_stream_fields_absent() {
        let output = json!({
            "content": "file body",
            "path": "README.md"
        });

        assert_eq!(aggregated_output_from_value(&output), "file body");
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
            has_more: false,
        };

        let (event_status, exit_code, aggregated_output) = match &status {
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
        assert_eq!(aggregated_output, "boom");
    }
}
