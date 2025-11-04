use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time;
use tokio_util::sync::CancellationToken;

use super::progress::ProgressReporter;
use tracing::warn;
use vtcode_core::exec::cancellation;
use vtcode_core::tools::registry::ToolErrorType;
use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry, ToolTimeoutCategory};

use super::state::CtrlCState;

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
#[allow(dead_code)]
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
    progress_reporter: Option<&ProgressReporter>,
) -> ToolExecutionStatus {
    // Use provided progress reporter or create a new one
    let mut local_progress_reporter = None;
    let progress_reporter = if let Some(reporter) = progress_reporter {
        reporter
    } else {
        local_progress_reporter = Some(ProgressReporter::new());
        local_progress_reporter.as_ref().unwrap()
    };

    // Execute with progress tracking
    let result = execute_tool_with_progress(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
    )
    .await;

    // Ensure progress is marked as complete only if we created the reporter
    if let Some(ref local_reporter) = local_progress_reporter {
        local_reporter.complete().await;
    }
    result
}

/// Execute a tool with progress reporting
async fn execute_tool_with_progress(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
) -> ToolExecutionStatus {
    let start_time = std::time::Instant::now();

    // Phase 1: Preparation (0-15%)
    progress_reporter
        .set_message(format!("Preparing {}...", name))
        .await;
    progress_reporter.set_progress(5).await;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        return ToolExecutionStatus::Cancelled;
    }

    // Phase 2: Setup (15-25%)
    progress_reporter
        .set_message(format!("Setting up {} execution...", name))
        .await;
    progress_reporter.set_progress(20).await;

    let category = registry.timeout_category_for(name).await;
    let timeout_policy = registry.timeout_policy().clone();
    let timeout_duration = timeout_policy.ceiling_for(category);
    let warning_fraction = timeout_policy.warning_fraction();

    'outer: loop {
        // Create a fresh clone of registry and args for each iteration
        let mut registry_clone = registry.clone();
        let args_clone = args.clone();

        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            return ToolExecutionStatus::Cancelled;
        }

        // Phase 3: Active execution (25-85%)
        progress_reporter
            .set_message(format!("Executing {}...", name))
            .await;

        // Use elapsed time to estimate progress during execution
        // Most tools complete within a few seconds, so we'll show progress based on time
        let elapsed = start_time.elapsed();
        let estimated_progress = if elapsed < std::time::Duration::from_millis(500) {
            30
        } else if elapsed < std::time::Duration::from_secs(2) {
            50
        } else if elapsed < std::time::Duration::from_secs(5) {
            70
        } else {
            85
        };
        progress_reporter.set_progress(estimated_progress).await;

        let token = CancellationToken::new();
        let exec_future = {
            let name = name.to_string();
            let progress_reporter = progress_reporter.clone();

            cancellation::with_tool_cancellation(token.clone(), async move {
                // Tool execution in progress (already set above)
                progress_reporter.set_progress(40).await;

                // Execute the tool with the cloned registry and args
                let result = registry_clone.execute_tool(&name, args_clone).await;

                // Phase 4: Processing results (85-95%)
                progress_reporter
                    .set_message(format!("Processing {} results...", name))
                    .await;
                progress_reporter.set_progress(90).await;

                result
            })
        };

        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            token.cancel();
            return ToolExecutionStatus::Cancelled;
        }

        let mut exec_future = Box::pin(exec_future);
        let mut timeout_timer = timeout_duration.map(|duration| Box::pin(time::sleep(duration)));
        let mut warning_timer: Option<Pin<Box<time::Sleep>>> = match timeout_duration {
            Some(duration) if warning_fraction > 0.0 && warning_fraction < 1.0 => {
                let threshold = duration.mul_f32(warning_fraction);
                if threshold.is_zero() {
                    None
                } else {
                    Some(Box::pin(time::sleep(threshold)))
                }
            }
            _ => None,
        };
        let mut warning_emitted = false;

        enum ExecutionOutcome {
            Completed(anyhow::Result<Value>),
            Timeout,
            Cancelled,
        }

        let outcome = loop {
            tokio::select! {
                _ = ctrl_c_notify.notified() => {
                    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
                        token.cancel();
                        break ExecutionOutcome::Cancelled;
                    }
                    token.cancel();
                    continue 'outer;
                }
                _ = async {
                    if let Some(timer) = warning_timer.as_mut() {
                        timer.as_mut().await;
                    }
                }, if warning_timer.is_some() && !warning_emitted => {
                    warning_emitted = true;
                    warning_timer = None;
                    if let Some(limit) = timeout_duration {
                        let elapsed = start_time.elapsed();
                        let message = format!(
                            "{} has run for {:.1}s ({} ceiling: {}s). Press Ctrl+C to cancel or adjust [timeouts] in vtcode.toml.",
                            name,
                            elapsed.as_secs_f32(),
                            category.label(),
                            limit.as_secs()
                        );
                        progress_reporter.set_message(message).await;
                        if progress_reporter.current_progress() < 90 {
                            progress_reporter.set_progress(90).await;
                        }
                        warn!(
                            tool = name,
                            category = category.label(),
                            elapsed_seconds = elapsed.as_secs_f32(),
                            ceiling_seconds = limit.as_secs(),
                            "tool execution nearing timeout ceiling"
                        );
                    }
                    continue;
                }
                _ = async {
                    if let Some(timer) = timeout_timer.as_mut() {
                        timer.as_mut().await;
                    }
                }, if timeout_timer.is_some() => {
                    token.cancel();
                    break ExecutionOutcome::Timeout;
                }
                result = &mut exec_future => {
                    break ExecutionOutcome::Completed(result);
                }
            }
        };

        match outcome {
            ExecutionOutcome::Cancelled => return ToolExecutionStatus::Cancelled,
            ExecutionOutcome::Timeout => {
                progress_reporter
                    .set_message(format!("{} timed out", name))
                    .await;
                return create_timeout_error(name, category, timeout_duration);
            }
            ExecutionOutcome::Completed(result) => {
                return match result {
                    Ok(output) => {
                        // Phase 5: Finalization (95-100%)
                        progress_reporter
                            .set_message(format!("Finalizing {}...", name))
                            .await;
                        progress_reporter.set_progress(95).await;

                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                        // Mark as complete
                        progress_reporter.set_progress(100).await;
                        progress_reporter
                            .set_message(format!("{} completed successfully", name))
                            .await;
                        process_tool_output(output)
                    }
                    Err(error) => {
                        // Update with error status
                        progress_reporter
                            .set_message(format!("{} failed", name))
                            .await;
                        ToolExecutionStatus::Failure { error }
                    }
                };
            }
        }
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
fn create_timeout_error(
    name: &str,
    category: ToolTimeoutCategory,
    timeout: Option<Duration>,
) -> ToolExecutionStatus {
    let message = match timeout {
        Some(limit) => format!(
            "Operation '{}' exceeded the {} timeout ceiling ({}s)",
            name,
            category.label(),
            limit.as_secs()
        ),
        None => format!(
            "Operation '{}' exceeded the {} timeout ceiling",
            name,
            category.label()
        ),
    };

    ToolExecutionStatus::Timeout {
        error: ToolExecutionError::new(name.to_string(), ToolErrorType::Timeout, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;
    use tokio::sync::Notify;
    use tokio::task::yield_now;
    use vtcode_core::config::TimeoutsConfig;
    use vtcode_core::config::types::CapabilityLevel;
    use vtcode_core::tools::registry::ToolRegistration;

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
            None,
        )
        .await;

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
        } = status
        {
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
        let status = create_timeout_error(
            "test_tool",
            ToolTimeoutCategory::Default,
            Some(Duration::from_secs(42)),
        );
        if let ToolExecutionStatus::Timeout { error } = status {
            assert!(error.message.contains("test_tool"));
            assert!(error.message.contains("timeout ceiling"));
            assert!(error.message.contains("42"));
        } else {
            panic!("Expected Timeout variant");
        }
    }

    const DELAYED_TOOL: &str = "delayed_tool";

    fn delayed_tool_executor(
        _registry: &mut ToolRegistry,
        _args: Value,
    ) -> BoxFuture<'_, anyhow::Result<Value>> {
        Box::pin(async move {
            if let Some(token) = cancellation::current_tool_cancellation() {
                let _ = token.cancelled().await;
            } else {
                tokio::time::sleep(StdDuration::from_secs(60)).await;
            }
            Ok(json!({ "status": "cancelled" }))
        })
    }

    #[tokio::test]
    async fn emits_warning_before_timeout_ceiling() {
        let mut registry = ToolRegistry::new(std::env::current_dir().unwrap()).await;
        registry
            .register_tool(
                ToolRegistration::new(
                    DELAYED_TOOL,
                    CapabilityLevel::Editing,
                    false,
                    delayed_tool_executor,
                )
                .with_llm_visibility(false),
            )
            .expect("tool registration should succeed");

        let mut timeouts = TimeoutsConfig::default();
        timeouts.default_ceiling_seconds = 1;
        timeouts.warning_threshold_percent = 10;
        registry.apply_timeout_policy(&timeouts);

        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let reporter = ProgressReporter::new();

        let ctrl_c_state_handle = ctrl_c_state.clone();
        let ctrl_c_notify_handle = ctrl_c_notify.clone();
        let reporter_handle = reporter.clone();

        let handle = tokio::spawn(async move {
            execute_tool_with_timeout(
                &mut registry,
                DELAYED_TOOL,
                json!({}),
                &ctrl_c_state_handle,
                &ctrl_c_notify_handle,
                Some(&reporter_handle),
            )
            .await
        });

        tokio::time::sleep(StdDuration::from_millis(150)).await;
        yield_now().await;

        let (_, _, warning_message, _, _, _) = reporter.get_state().get_progress().await;
        assert!(warning_message.contains("has run"));
        assert!(warning_message.contains("Press Ctrl+C"));

        tokio::time::sleep(StdDuration::from_millis(1200)).await;

        let status = handle.await.expect("task should complete");
        assert!(matches!(status, ToolExecutionStatus::Timeout { .. }));
    }
}
