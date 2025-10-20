use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time;

use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry};

use super::state::CtrlCState;

const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) enum ToolExecutionStatus {
    Success {
        output: Value,
        stdout: Option<String>,
        modified_files: Vec<String>,
        command_success: bool,
        has_more: bool,
    },
    Failure {
        error: Error,
    },
    Timeout {
        error: ToolExecutionError,
    },
    Cancelled,
}

pub(crate) async fn execute_tool_with_timeout(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> ToolExecutionStatus {
    loop {
        let result = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => {
                if ctrl_c_state.is_cancel_requested() {
                    return ToolExecutionStatus::Cancelled;
                }
                continue;
            }

            result = time::timeout(TOOL_TIMEOUT, registry.execute_tool(name, args.clone())) => {
                result
            }
        };

        return match result {
            Ok(Ok(output)) => process_tool_output(output),
            Ok(Err(error)) => ToolExecutionStatus::Failure { error },
            Err(_) => create_timeout_error(name),
        };
    }
}

fn process_tool_output(output: Value) -> ToolExecutionStatus {
    let exit_code = output
        .get("exit_code")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let command_success = exit_code == 0;
    let stdout = output
        .get("stdout")
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

fn create_timeout_error(name: &str) -> ToolExecutionStatus {
    let timeout_error = ToolExecutionError::new(
        name.to_string(),
        vtcode_core::tools::registry::ToolErrorType::ExecutionError,
        "Tool execution timed out after 5 minutes".to_string(),
    );
    ToolExecutionStatus::Timeout {
        error: timeout_error,
    }
}
