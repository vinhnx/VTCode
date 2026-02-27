use anyhow::anyhow;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::error_messages::agent_execution;

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
        ToolExecutionStatus::Failure { error } => error.to_string().contains("LOOP DETECTION"),
        _ => false,
    }
}

pub(super) fn build_tool_status_message(tool_name: &str, args: &Value) -> String {
    if is_command_tool(tool_name) {
        let command = args
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or(tool_name);
        format!("Running command: {}", command)
    } else {
        format!("Running tool: {}", tool_name)
    }
}

fn is_command_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        tools::RUN_PTY_CMD
            | tools::SHELL
            | tools::UNIFIED_EXEC
            | tools::EXECUTE_CODE
            | tools::EXEC_PTY_CMD
            | tools::EXEC
    )
}

pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    if let Some(loop_detected) = output.get("loop_detected").and_then(|v| v.as_bool())
        && loop_detected
    {
        let tool_name = output
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("tool");
        let repeat_count = output
            .get("repeat_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let base_error_msg = output
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Tool blocked due to repeated identical invocations");

        let clear_error_msg = agent_execution::loop_detection_block_message(
            tool_name,
            repeat_count,
            Some(base_error_msg),
        );
        return ToolExecutionStatus::Failure {
            error: anyhow!(clear_error_msg),
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
            error: anyhow!(error_msg),
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
    let has_more = output
        .get("has_more")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    ToolExecutionStatus::Success {
        output,
        stdout,
        modified_files,
        command_success,
        has_more,
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
        assert!(parse_cached_output("{not-valid-json").is_err());
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
