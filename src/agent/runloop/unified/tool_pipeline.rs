use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::progress::ProgressReporter;
use tracing::warn;
use vtcode_core::exec::cancellation;
use vtcode_core::tools::registry::ToolErrorType;
use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry, ToolTimeoutCategory};

use super::state::CtrlCState;

const TOOL_TIMEOUT: Duration = Duration::from_secs(300);
const TOOL_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(5);

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

    let warning_cancel_token = CancellationToken::new();
    let warning_task = spawn_timeout_warning_task(
        name.to_string(),
        start_time,
        warning_cancel_token.clone(),
    );

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

    let status = loop {
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

        enum ExecutionControl {
            Continue,
            Cancelled,
            Completed(Result<Result<Value, Error>, time::error::Elapsed>),
        }

        let control = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => {
                if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
                    token.cancel();
                    ExecutionControl::Cancelled
                } else {
                    token.cancel();
                    ExecutionControl::Continue
                }
            }
            _ => None,
        };
        let mut warning_emitted = false;

            result = time::timeout(TOOL_TIMEOUT, exec_future) => ExecutionControl::Completed(result),
        };

        match control {
            ExecutionControl::Continue => continue,
            ExecutionControl::Cancelled => break ToolExecutionStatus::Cancelled,
            ExecutionControl::Completed(result) => {
                break match result {
                    Ok(Ok(output)) => {
                        progress_reporter
                            .set_message(format!("Finalizing {}...", name))
                            .await;
                        progress_reporter.set_progress(95).await;

                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                        progress_reporter.set_progress(100).await;
                        progress_reporter
                            .set_message(format!("{} completed successfully", name))
                            .await;
                        process_tool_output(output)
                    }
                    Ok(Err(error)) => {
                        progress_reporter
                            .set_message(format!("{} failed", name))
                            .await;
                        ToolExecutionStatus::Failure { error }
                    }
                    Err(_) => {
                        token.cancel();
                        progress_reporter
                            .set_message(format!("{} timed out", name))
                            .await;
                        create_timeout_error(name)
                    }
                };
            }
        }
    };

    warning_cancel_token.cancel();
    if let Some(handle) = warning_task {
        let _ = handle.await;
    }

    status
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

fn spawn_timeout_warning_task(
    tool_name: String,
    start_time: std::time::Instant,
    cancel_token: CancellationToken,
) -> Option<JoinHandle<()>> {
    let warning_delay = TOOL_TIMEOUT
        .checked_sub(TOOL_TIMEOUT_WARNING_HEADROOM)
        .filter(|delay| !delay.is_zero())?;

    Some(tokio::spawn(async move {
        tokio::select! {
            _ = cancel_token.cancelled() => {}
            _ = tokio::time::sleep(warning_delay) => {
                let elapsed = start_time.elapsed().as_secs();
                let timeout_secs = TOOL_TIMEOUT.as_secs();
                let remaining_secs = TOOL_TIMEOUT_WARNING_HEADROOM.as_secs();
                warn!(
                    "Tool '{}' has run for {} seconds and is approaching the {} second time limit ({} seconds remaining). It will be cancelled soon unless it completes.",
                    tool_name,
                    elapsed,
                    timeout_secs,
                    remaining_secs
                );
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::BoxFuture;
    use serde_json::json;
    use std::io;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::Notify;
    use tokio::task::yield_now;
    use tokio::time::advance;
    use tracing::Level;
    use assert_fs::TempDir;
    use futures::future::BoxFuture;
    use tracing_subscriber::fmt;
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

    #[derive(Clone)]
    struct CaptureWriter {
        buffer: Arc<Mutex<Vec<String>>>,
    }

    impl CaptureWriter {
        fn new(buffer: Arc<Mutex<Vec<String>>>) -> Self {
            Self { buffer }
        }
    }

    impl io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let text = String::from_utf8_lossy(buf).to_string();
            self.buffer.lock().unwrap().push(text);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn slow_tool_executor<'a>(
        _registry: &'a mut ToolRegistry,
        _args: Value,
    ) -> BoxFuture<'a, anyhow::Result<Value>> {
        Box::pin(async move {
            tokio::time::sleep(TOOL_TIMEOUT + Duration::from_secs(1)).await;
            Ok(json!({
                "exit_code": 0
            }))
        })
    }

    #[tokio::test(start_paused = true)]
    async fn emits_warning_before_timeout_ceiling() {
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let writer_buffer = warnings.clone();

        let subscriber = fmt()
            .with_writer(move || CaptureWriter::new(writer_buffer.clone()))
            .with_max_level(Level::WARN)
            .without_time()
            .finish();

        let _guard = tracing::subscriber::set_default(subscriber);

        let temp_dir = TempDir::new().expect("create temp dir");
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry
            .register_tool(ToolRegistration::new(
                "__test_slow_tool__",
                CapabilityLevel::Basic,
                false,
                slow_tool_executor,
            ))
            .expect("register slow tool");

        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let mut registry_task = registry;
        let ctrl_c_state_clone = ctrl_c_state.clone();
        let ctrl_c_notify_clone = ctrl_c_notify.clone();

        let execution = tokio::spawn(async move {
            execute_tool_with_timeout(
                &mut registry_task,
                "__test_slow_tool__",
                Value::Null,
                &ctrl_c_state_clone,
                &ctrl_c_notify_clone,
                None,
            )
            .await
        });

        let warning_delay = TOOL_TIMEOUT
            .checked_sub(TOOL_TIMEOUT_WARNING_HEADROOM)
            .expect("warning delay");
        advance(warning_delay).await;
        yield_now().await;

        let captured = warnings.lock().unwrap();
        let combined = captured.join("");
        assert!(
            combined.contains("has run"),
            "expected warning log to include 'has run', captured logs: {}",
            combined
        );
        drop(captured);

        advance(TOOL_TIMEOUT_WARNING_HEADROOM + Duration::from_secs(1)).await;
        let status = execution.await.expect("join execution");
        assert!(matches!(status, ToolExecutionStatus::Timeout { .. }));
    }
}
