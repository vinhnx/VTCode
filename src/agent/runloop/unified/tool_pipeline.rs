use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time;
use tokio_util::sync::CancellationToken;

use super::progress::ProgressReporter;
use vtcode_core::exec::cancellation;
use vtcode_core::tools::registry::ToolErrorType;
use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry};

use super::state::CtrlCState;

const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

/// Status of a tool execution with progress information
#[derive(Debug)]
pub(crate) struct ToolProgress {
    /// Current progress value (0-100)
    pub progress: u8,
    /// Status message
    pub message: String,
}

/// Result of a tool execution
#[derive(Debug)]
pub(crate) enum ToolExecutionStatus {
    /// Tool completed successfully
    Success {
        /// Tool output
        output: Value,
        /// Standard output if available
        stdout: Option<String>,
        /// List of modified files
        modified_files: Vec<String>,
        /// Whether the command was successful
        command_success: bool,
        /// Whether there are more results available
        has_more: bool,
    },
    /// Tool execution failed
    Failure {
        /// Error that occurred
        error: Error,
    },
    /// Tool execution timed out
    Timeout {
        /// Timeout error
        error: ToolExecutionError,
    },
    /// Tool execution was cancelled
    Cancelled,
    /// Tool execution progress update
    Progress(ToolProgress),
}

/// Execute a tool with a timeout and progress reporting
pub(crate) async fn execute_tool_with_timeout(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> ToolExecutionStatus {
    // Create a progress reporter for this tool execution
    let progress_reporter = ProgressReporter::new();

    // Execute with progress tracking
    let result = execute_tool_with_progress(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter.clone(),
    ).await;

    // Ensure progress is marked as complete
    progress_reporter.complete().await;
    result
}

/// Execute a tool with progress reporting
async fn execute_tool_with_progress(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: ProgressReporter,
) -> ToolExecutionStatus {
    // Set initial progress
    progress_reporter.set_message(format!("Initializing {}...", name)).await;
    progress_reporter.set_progress(5).await;

    loop {
        // Create a fresh clone of registry and args for each iteration
        let mut registry_clone = registry.clone();
        let args_clone = args.clone();

        // Create a new progress reporter for this iteration
        let progress_reporter_clone = progress_reporter.clone();
        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            return ToolExecutionStatus::Cancelled;
        }

        // Update progress periodically if the tool is still running
        progress_reporter.set_message(format!("Running {}...", name)).await;
        progress_reporter.increment(1).await;

        // Don't exceed 90% to leave room for final processing
        if progress_reporter.current_progress() >= 90 {
            progress_reporter.set_progress(90).await;
        }

        let token = CancellationToken::new();
        let exec_future = {
            let name = name.to_string();

            cancellation::with_tool_cancellation(token.clone(), async move {
                // Update progress when starting execution
                progress_reporter_clone.set_message(format!("Executing {}...", name)).await;
                progress_reporter_clone.set_progress(10).await;

                // Execute the tool with the cloned registry and args
                let result = registry_clone.execute_tool(&name, args_clone).await;

                // Update progress before returning
                progress_reporter_clone.set_message(format!("Finishing {}...", name)).await;
                progress_reporter_clone.set_progress(95).await;

                result
            })
        };

        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            token.cancel();
            return ToolExecutionStatus::Cancelled;
        }

        let result = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => {
                if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
                    token.cancel();
                    return ToolExecutionStatus::Cancelled;
                }
                token.cancel();
                continue;
            }

            result = time::timeout(TOOL_TIMEOUT, exec_future) => {
                result
            }
        };

        return match result {
            Ok(Ok(output)) => {
                // Mark as complete and process output
                progress_reporter.set_progress(100).await;
                progress_reporter.set_message(format!("{} completed", name)).await;
                process_tool_output(output)
            },
            Ok(Err(error)) => {
                // Update with error status
                progress_reporter.set_message(format!("{} failed", name)).await;
                ToolExecutionStatus::Failure { error }
            },
            Err(_) => {
                token.cancel();
                progress_reporter.set_message(format!("{} timed out", name)).await;
                create_timeout_error(name)
            }
        };
    }
}

/// Process the output from a tool execution and convert it to a ToolExecutionStatus
fn process_tool_output(output: Value) -> ToolExecutionStatus {
    let exit_code = output
        .get("exit_code")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let command_success = exit_code == 0;

    // Extract stdout if available
    let stdout = output
        .get("stdout")
        .and_then(|value| value.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Extract modified files if available
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

    // Check if there are more results
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

/// Create a timeout error for a tool execution
fn create_timeout_error(name: &str) -> ToolExecutionStatus {
    ToolExecutionStatus::Timeout {
        error: ToolExecutionError::new(
            name.to_string(),
            ToolErrorType::Timeout,
            format!("Operation '{}' timed out after {} seconds", name, TOOL_TIMEOUT.as_secs()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_execute_tool_with_timeout() {
        // Setup test dependencies
        let mut registry = ToolRegistry::new(std::env::current_dir().unwrap()).await;
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        // Test a simple tool execution with unknown tool
        let result = execute_tool_with_timeout(
            &mut registry,
            "test_tool",
            json!({}),
            &ctrl_c_state,
            &ctrl_c_notify,
        ).await;

        // Verify the result - unknown tool should return error or failure
        match result {
            ToolExecutionStatus::Failure { .. } => {
                // Expected for unknown tool
            }
            ToolExecutionStatus::Success { ref output, .. } => {
                // Tool returns success with error in output for unknown tools
                if output.get("error").is_some() {
                    // This is acceptable - tool returned an error object
                } else {
                    panic!("Expected tool to return error object for unknown tool");
                }
            }
            other => panic!("Unexpected result type: {:?}", other),
        }
    }

    #[test]
    fn test_process_tool_output() {
        // Test successful output
        let output = json!({
            "exit_code": 0,
            "stdout": "test output",
            "modified_files": ["file1.txt", "file2.txt"],
            "has_more": false
        });

        let status = process_tool_output(output);
        if let ToolExecutionStatus::Success {
            output: _,
            stdout,
            modified_files,
            command_success,
            has_more,
        } = status {
            assert_eq!(stdout, Some("test output".to_string()));
            assert_eq!(modified_files, vec!["file1.txt", "file2.txt"]);
            assert!(command_success);
            assert!(!has_more);
        } else {
            panic!("Expected Success variant");
        }
    }

    #[test]
    fn test_create_timeout_error() {
        let status = create_timeout_error("test_tool");
        if let ToolExecutionStatus::Timeout { error } = status {
            assert!(error.message.contains("test_tool"));
            assert!(error.message.contains("timed out"));
        } else {
            panic!("Expected Timeout variant");
        }
    }
}
