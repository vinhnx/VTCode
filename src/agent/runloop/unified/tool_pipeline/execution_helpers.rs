use anyhow::anyhow;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::tools::tool_intent;

use super::status::ToolExecutionStatus;

pub(super) fn parse_cached_output(cached_output: &str) -> serde_json::Result<Value> {
    serde_json::from_str(cached_output)
}

pub(super) fn is_loop_detection_status(status: &ToolExecutionStatus) -> bool {
    match status {
        ToolExecutionStatus::Success { output, .. } => output
            .get("loop_detected")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        ToolExecutionStatus::Failure { error } => error.message.contains("LOOP DETECTION"),
        _ => false,
    }
}

pub(super) fn build_tool_status_message(tool_name: &str, args: &Value) -> String {
    if is_command_tool(tool_name, args) {
        let command = args
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or(tool_name);
        format!("Running command: {command}")
    } else {
        format!("Running tool: {tool_name}")
    }
}

fn is_command_tool(tool_name: &str, args: &Value) -> bool {
    tool_name == tools::EXECUTE_CODE || tool_intent::is_command_run_tool_call(tool_name, args)
}

pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    // Treat loop_detected as Success (not Failure) so it does not increment the
    // consecutive_blocked_tool_calls fuse.  The model receives the loop metadata
    // (loop_detected, reused_recent_result, etc.) and can adjust its approach,
    // but the blocked-streak counter is not polluted by a synthetic PolicyViolation.
    //
    // Prior to this fix, loop_detected was wrapped in a PolicyViolation failure
    // which caused the consecutive blocked fuse to kill the entire turn after
    // DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN (8) iterations —
    // blocking even unrelated tool calls (e.g. unified_exec) that happened to
    // follow the loop-detected ones.
    if output
        .get("loop_detected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return ToolExecutionStatus::Success {
            output,
            stdout: None,
            modified_files: vec![],
            command_success: true,
        };
    }

    if let Some(error) = ToolExecutionError::from_tool_output(&output) {
        return if matches!(error.error_type, ToolErrorType::Timeout) {
            ToolExecutionStatus::Timeout { error }
        } else {
            ToolExecutionStatus::Failure { error }
        };
    }

    if let Some(error_value) = output.get("error") {
        let error_msg = if let Some(message) = error_value.get("message").and_then(|m| m.as_str()) {
            message.to_string()
        } else if let Some(error_str) = error_value.as_str() {
            error_str.to_string()
        } else {
            "Unknown tool execution error".to_string()
        };
        return ToolExecutionStatus::Failure {
            error: ToolExecutionError::from_anyhow(
                "tool",
                &anyhow!(error_msg),
                0,
                false,
                false,
                Some("unified_runloop"),
            ),
        };
    }

    let exit_code = output
        .get("exit_code")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let command_success = exit_code == 0;
    let stdout = output
        .get("stdout")
        .or_else(|| {
            if is_command_like_output(&output) {
                output.get("output")
            } else {
                None
            }
        })
        .and_then(|value| value.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let modified_files = output
        .get("modified_files")
        .and_then(|value| value.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    ToolExecutionStatus::Success {
        output,
        stdout,
        modified_files,
        command_success,
    }
}

fn is_command_like_output(output: &Value) -> bool {
    output.get("command").is_some()
        || output.get("working_directory").is_some()
        || output.get("session_id").is_some()
        || output.get("process_id").is_some()
        || output.get("spool_path").is_some()
        || output.get("is_exited").is_some()
        || output.get("exit_code").is_some()
        || output
            .get("content_type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "exec_inspect")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn loop_detection_status_detects_failure_marker() {
        let status = process_llm_tool_output(json!({
            "error": {
                "message": "Tool blocked after repeated invocations"
            },
            "loop_detected": true,
            "repeat_count": 3,
            "tool": "read_file"
        }));

        assert!(is_loop_detection_status(&status));
    }

    #[test]
    fn malformed_cached_json_is_rejected() {
        parse_cached_output("{not-valid-json").unwrap_err();
    }

    #[test]
    fn falls_back_to_output_for_command_like_stdout() {
        let status = process_llm_tool_output(json!({
            "command": "ls -la",
            "output": "file-a\nfile-b\n",
            "exit_code": 0,
            "is_exited": true
        }));

        match status {
            ToolExecutionStatus::Success { stdout, .. } => {
                assert_eq!(stdout.as_deref(), Some("file-a\nfile-b"));
            }
            _ => panic!("expected success status"),
        }
    }

    #[test]
    fn falls_back_to_output_for_inspect_payload() {
        let status = process_llm_tool_output(json!({
            "output": "1: src/main.rs",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "content_type": "exec_inspect"
        }));

        match status {
            ToolExecutionStatus::Success { stdout, .. } => {
                assert_eq!(stdout.as_deref(), Some("1: src/main.rs"));
            }
            _ => panic!("expected success status"),
        }
    }
}
