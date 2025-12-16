#![allow(clippy::too_many_arguments)]
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Error, anyhow};
use serde_json::Value;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::progress::ProgressReporter;
use vtcode_core::exec::cancellation;
use vtcode_core::tools::registry::ToolErrorType;
use vtcode_core::tools::registry::{
    ToolExecutionError, ToolRegistry, ToolTimeoutCategory, classify_error,
};

use super::run_loop_context::RunLoopContext;
use super::state::CtrlCState;
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::unified::tool_routing::ensure_tool_permission;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
use crate::hooks::lifecycle::LifecycleHookEngine;
use vtcode_core::config::loader::VTCodeConfig;

// No direct use of ApprovalRecorder or DecisionOutcome in this module; these are referenced via `RunLoopContext`.

/// Default timeout for tool execution if no policy is configured
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(180);
/// Minimum buffer before cancelling a tool once a warning fires
const MIN_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(5);
const MAX_TOOL_RETRIES: usize = 2;
const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(200);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(3);

/// Guard that ensures timeout warning tasks are cancelled when the tool attempt ends early
struct TimeoutWarningGuard {
    cancel_token: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl TimeoutWarningGuard {
    fn new(
        tool_name: &str,
        start_time: Instant,
        tool_timeout: Duration,
        warning_fraction: f32,
    ) -> Self {
        let cancel_token = CancellationToken::new();
        let handle = spawn_timeout_warning_task(
            tool_name.to_string(),
            start_time,
            cancel_token.clone(),
            tool_timeout,
            warning_fraction,
        );
        Self {
            cancel_token,
            handle,
        }
    }

    async fn cancel(&mut self) {
        self.cancel_token.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

/// Status of a tool execution with progress information
#[derive(Debug)]
#[allow(dead_code)]
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
    /// Tool completed
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

/// Outcome produced by a tool pipeline run - returns a success/failure wrapper along with stdout and modified files
#[allow(dead_code)]
pub(crate) struct ToolPipelineOutcome {
    pub status: ToolExecutionStatus,
    pub stdout: Option<String>,
    pub modified_files: Vec<String>,
    pub command_success: bool,
    pub has_more: bool,
}

impl ToolPipelineOutcome {
    pub(crate) fn from_status(status: ToolExecutionStatus) -> Self {
        match status {
            ToolExecutionStatus::Success {
                output,
                stdout,
                modified_files,
                command_success,
                has_more,
            } => ToolPipelineOutcome {
                status: ToolExecutionStatus::Success {
                    output: output.clone(),
                    stdout: stdout.clone(),
                    modified_files: modified_files.clone(),
                    command_success,
                    has_more,
                },
                stdout,
                modified_files,
                command_success,
                has_more,
            },
            other => ToolPipelineOutcome {
                status: other,
                stdout: None,
                modified_files: vec![],
                command_success: false,
                has_more: false,
            },
        }
    }
}

/// Execute tool call and handle permission, caching and common rendering.
#[allow(dead_code)]
pub(crate) async fn run_tool_call(
    ctx: &mut RunLoopContext<'_>,
    call: &vtcode_core::llm::provider::ToolCall,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    _token_budget: &Arc<vtcode_core::core::token_budget::TokenBudgetManager>,
    _vt_cfg: Option<&VTCodeConfig>,
    turn_index: usize,
) -> Result<ToolPipelineOutcome, anyhow::Error> {
    let function = match call.function.as_ref() {
        Some(func) => func,
        None => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!("Tool call missing function"),
                },
            ));
        }
    };

    let name = function.name.as_str().to_string();
    let args_val = match call.parsed_arguments() {
        Ok(args) => args,
        Err(err) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!(err),
                },
            ));
        }
    };

    // Pre-flight permission check
    match ensure_tool_permission(
        ctx.tool_registry,
        &name,
        Some(&args_val),
        ctx.renderer,
        ctx.handle,
        ctx.session,
        default_placeholder.clone(),
        ctrl_c_state,
        ctrl_c_notify,
        lifecycle_hooks,
        None, // justification
        Some(ctx.approval_recorder),
        Some(ctx.decision_ledger),
        Some(ctx.tool_permission_cache),
    )
    .await
    {
        Ok(super::tool_routing::ToolPermissionFlow::Approved) => {}
        Ok(super::tool_routing::ToolPermissionFlow::Denied) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow::anyhow!("Tool permission denied"),
                },
            ));
        }
        Ok(super::tool_routing::ToolPermissionFlow::Interrupted) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Cancelled,
            ));
        }
        Ok(super::tool_routing::ToolPermissionFlow::Exit) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Cancelled,
            ));
        }
        Err(e) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure { error: e },
            ));
        }
    }

    // Determine read-only tools for caching
    let is_read_only_tool = matches!(
        name.as_str(),
        "read_file" | "list_files" | "grep_search" | "find_files" | "tree_sitter_analyze"
    );
    let cache_target = cache_target_path(&name, &args_val);

    // Attempt cache retrieval for read-only tools
    if is_read_only_tool {
        let mut cache = ctx.tool_result_cache.write().await;
        let cache_key = vtcode_core::tools::result_cache::ToolCacheKey::from_json(
            &name,
            &args_val,
            &cache_target,
        );
        if let Some(cached_output) = cache.get(&cache_key) {
            let cached_json: serde_json::Value =
                serde_json::from_str(&cached_output).unwrap_or(serde_json::json!({}));
            let status = ToolExecutionStatus::Success {
                output: cached_json,
                stdout: None,
                modified_files: vec![],
                command_success: true,
                has_more: false,
            };
            return Ok(ToolPipelineOutcome::from_status(status));
        }
    }

    // Force TUI redraw to ensure stable UI without added delay
    ctx.handle.force_redraw();

    // Execute with progress reporter
    let progress_reporter = ProgressReporter::new();
    progress_reporter.set_total(100).await;
    progress_reporter.set_progress(0).await;
    progress_reporter
        .set_message(format!("Starting {}...", name))
        .await;

    let tool_spinner = PlaceholderSpinner::with_progress(
        ctx.handle,
        Some("".to_string()),
        Some("".to_string()),
        format!("Running tool: {}", name),
        Some(&progress_reporter),
    );

    let outcome = execute_tool_with_timeout_ref(
        ctx.tool_registry,
        &name,
        &args_val,
        ctrl_c_state,
        ctrl_c_notify,
        Some(&progress_reporter),
    )
    .await;

    // Handle loop detection for read-only tools: if blocked, try to return cached result
    let outcome = if is_read_only_tool {
        if let ToolExecutionStatus::Success { output, .. } = &outcome {
            // Check if this is actually a loop detection error wrapped as success
            if let Some(loop_detected) = output.get("loop_detected").and_then(|v| v.as_bool()) {
                if loop_detected {
                    // Tool was blocked due to loop detection - try to get cached result
                    let mut cache = ctx.tool_result_cache.write().await;
                    let cache_key = vtcode_core::tools::result_cache::ToolCacheKey::from_json(
                        &name,
                        &args_val,
                        &cache_target,
                    );
                    if let Some(cached_output) = cache.get(&cache_key) {
                        // We have a cached result from a previous successful call - return it
                        let cached_json: serde_json::Value =
                            serde_json::from_str(&cached_output).unwrap_or(serde_json::json!({}));
                        drop(cache);
                        tool_spinner.finish();
                        return Ok(ToolPipelineOutcome::from_status(
                            ToolExecutionStatus::Success {
                                output: cached_json,
                                stdout: None,
                                modified_files: vec![],
                                command_success: true,
                                has_more: false,
                            },
                        ));
                    }
                }
            }
        }
        outcome
    } else {
        outcome
    };

    if let ToolExecutionStatus::Success {
        output,
        stdout: _stdout,
        modified_files: _modified_files,
        command_success,
        has_more: _has_more,
    } = &outcome
    {
        tool_spinner.finish();
        // Cache successful read-only results
        if is_read_only_tool && *command_success {
            let mut cache = ctx.tool_result_cache.write().await;
            let output_json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
            let cache_key = vtcode_core::tools::result_cache::ToolCacheKey::from_json(
                &name,
                &args_val,
                &cache_target,
            );
            cache.insert_arc(cache_key, Arc::new(output_json));
        }
    }

    let mut pipeline_outcome = ToolPipelineOutcome::from_status(outcome);

    // If tool made file modifications, optionally confirm with git diff and either keep or revert
    if !pipeline_outcome.modified_files.is_empty() {
        let modified_files = pipeline_outcome.modified_files.clone();
        if confirm_changes_with_git_diff(&modified_files, skip_confirmations).await? {
            // record confirmed changes in trajectory inside ctx.traj
            ctx.traj.log_tool_call(
                turn_index,
                &name,
                &args_val,
                pipeline_outcome.command_success,
            );
            if pipeline_outcome.command_success {
                let mut cache = ctx.tool_result_cache.write().await;
                for path in &pipeline_outcome.modified_files {
                    cache.invalidate_for_path(path);
                }
            }
            // modified_files are kept as-is
        } else {
            // Reverted by confirm function; clear modified files
            pipeline_outcome.modified_files.clear();
            pipeline_outcome.command_success = false;
        }
    } else {
        // Log that the tool was invoked but made no file modifications
        ctx.traj.log_tool_call(
            turn_index,
            &name,
            &args_val,
            pipeline_outcome.command_success,
        );
    }

    // Ledger recording is left to the run loop where a decision id is available. Return the pipeline outcome only.

    Ok(pipeline_outcome)
}

/// Execute a tool with a timeout and progress reporting
///
/// This is a convenience wrapper around `execute_tool_with_timeout_ref` that takes
/// ownership of the args Value. Primarily used in tests and legacy code.
/// Production code should prefer `execute_tool_with_timeout_ref` to avoid cloning.
#[allow(dead_code)]
pub(crate) async fn execute_tool_with_timeout(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
) -> ToolExecutionStatus {
    execute_tool_with_timeout_ref(
        registry,
        name,
        &args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
    )
    .await
}

/// Execute a tool with a timeout and progress reporting (reference-based to avoid cloning args)
pub(crate) async fn execute_tool_with_timeout_ref(
    registry: &mut ToolRegistry,
    name: &str,
    args: &Value,
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

    // Determine the timeout category for this tool
    let timeout_category = registry.timeout_category_for(name).await;
    let timeout_ceiling = registry
        .timeout_policy()
        .ceiling_for(timeout_category)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT);

    // Execute with progress tracking
    let result = execute_tool_with_progress(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        timeout_ceiling,
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
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
) -> ToolExecutionStatus {
    // Execute first attempt
    let mut attempt = 0usize;
    let mut status = run_single_tool_attempt(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        tool_timeout,
    )
    .await;

    // Retry on recoverable errors with bounded backoff
    while let Some(delay) = retry_delay_for_status(&status, attempt) {
        attempt += 1;
        progress_reporter
            .set_message(format!(
                "Retrying {} (attempt {}/{}) after {}ms...",
                name,
                attempt + 1,
                MAX_TOOL_RETRIES + 1,
                delay.as_millis()
            ))
            .await;
        tokio::time::sleep(delay).await;

        status = run_single_tool_attempt(
            registry,
            name,
            args,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
            tool_timeout,
        )
        .await;
    }

    status
}

async fn run_single_tool_attempt(
    registry: &mut ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
) -> ToolExecutionStatus {
    let start_time = Instant::now();
    let warning_fraction = registry.timeout_policy().warning_fraction();
    let mut warning_guard = TimeoutWarningGuard::new(name, start_time, tool_timeout, warning_fraction);

    progress_reporter
        .set_message(format!("Preparing {}...", name))
        .await;
    progress_reporter.set_progress(5).await;

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        progress_reporter
            .set_message(format!("{} cancelled", name))
            .await;
        progress_reporter.set_progress(100).await;
        warning_guard.cancel().await;
        return ToolExecutionStatus::Cancelled;
    }

    progress_reporter
        .set_message(format!("Setting up {} execution...", name))
        .await;
    progress_reporter.set_progress(20).await;

    let status = loop {

        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            progress_reporter
                .set_message(format!("{} cancelled", name))
                .await;
            progress_reporter.set_progress(100).await;
            warning_guard.cancel().await;
            return ToolExecutionStatus::Cancelled;
        }

        progress_reporter
            .set_message(format!("Executing {}...", name))
            .await;

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
        let exec_future = cancellation::with_tool_cancellation(token.clone(), async {
            progress_reporter.set_progress(40).await;

            let result = registry.execute_tool_ref(name, args).await;

            progress_reporter
                .set_message(format!("Processing {} results...", name))
                .await;
            progress_reporter.set_progress(90).await;

            result
        });

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

            result = time::timeout(tool_timeout, exec_future) => ExecutionControl::Completed(result),
        };

        match control {
            ExecutionControl::Continue => continue,
            ExecutionControl::Cancelled => {
                progress_reporter
                    .set_message(format!("{} cancelled", name))
                    .await;
                progress_reporter.set_progress(100).await;
                break ToolExecutionStatus::Cancelled;
            }
            ExecutionControl::Completed(result) => {
                break match result {
                    Ok(Ok(output)) => {
                        progress_reporter
                            .set_message(format!("Finalizing {}...", name))
                            .await;
                        progress_reporter.set_progress(95).await;

                        progress_reporter.set_progress(100).await;
                        progress_reporter
                            .set_message(format!("{} completed", name))
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
                        let timeout_category = registry.timeout_category_for(name).await;
                        create_timeout_error(name, timeout_category, Some(tool_timeout))
                    }
                };
            }
        }
    };

    warning_guard.cancel().await;

    status
}

fn retry_delay_for_status(status: &ToolExecutionStatus, attempt: usize) -> Option<Duration> {
    if attempt >= MAX_TOOL_RETRIES {
        return None;
    }

    match status {
        ToolExecutionStatus::Timeout { error } => {
            if error.is_recoverable {
                Some(backoff_for_attempt(attempt))
            } else {
                None
            }
        }
        ToolExecutionStatus::Failure { error } => {
            let error_type = classify_error(error);
            if matches!(
                error_type,
                ToolErrorType::Timeout | ToolErrorType::NetworkError
            ) {
                Some(backoff_for_attempt(attempt))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn backoff_for_attempt(attempt: usize) -> Duration {
    let exp = 2_u64.saturating_pow(attempt.min(4) as u32); // cap exponent growth
    let jitter = Duration::from_millis(((attempt as u64 * 37) % 120).min(120));
    let backoff = RETRY_BACKOFF_BASE
        .saturating_mul(exp as u32)
        .saturating_add(jitter);
    backoff.min(MAX_RETRY_BACKOFF)
}

/// Process the output from a tool execution and convert it to a ToolExecutionStatus
fn process_tool_output(output: Value) -> ToolExecutionStatus {
    // Check for loop detection first - this is a critical signal to stop retrying
    if let Some(loop_detected) = output.get("loop_detected").and_then(|v| v.as_bool()) {
        if loop_detected {
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

            // Create a structured, explicit error message that clearly instructs the LLM to stop
            // Format: Use clear directives and structured information for better LLM understanding
            let clear_error_msg = format!(
                "LOOP DETECTION: Tool '{}' has been called {} times with identical parameters and is now blocked.\n\n\
                ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.\n\n\
                If you need the result from this tool:\n\
                1. Check if you already have the result from a previous successful call in your conversation history\n\
                2. If not available, use a different approach or modify your request\n\n\
                Original error: {}",
                tool_name, repeat_count, base_error_msg
            );
            return ToolExecutionStatus::Failure {
                error: anyhow::anyhow!(clear_error_msg),
            };
        }
    }

    // Check if the output contains an error object
    if let Some(error_value) = output.get("error") {
        let error_msg = if let Some(message) = error_value.get("message").and_then(|m| m.as_str()) {
            // Error is an object with message field
            message.to_string()
        } else if let Some(error_str) = error_value.as_str() {
            // Error is a direct string
            error_str.to_string()
        } else {
            // Fallback for unknown error format
            "Unknown tool execution error".to_string()
        };
        return ToolExecutionStatus::Failure {
            error: anyhow::anyhow!(error_msg),
        };
    }

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
    tool_timeout: Duration,
    warning_fraction: f32,
) -> Option<JoinHandle<()>> {
    let fraction = warning_fraction.clamp(0.1, 0.95);
    let fraction_delay = tool_timeout.mul_f32(fraction);
    let headroom_delay = tool_timeout.saturating_sub(MIN_TIMEOUT_WARNING_HEADROOM);
    let warning_delay = fraction_delay.min(headroom_delay);

    if warning_delay.is_zero() {
        return None;
    }

    Some(tokio::spawn(async move {
        tokio::select! {
            _ = cancel_token.cancelled() => {}
            _ = tokio::time::sleep(warning_delay) => {
                let elapsed_secs = start_time.elapsed().as_secs();
                let timeout_secs = tool_timeout.as_secs();
                let remaining_secs = tool_timeout
                    .saturating_sub(Duration::from_secs(elapsed_secs))
                    .as_secs();
                warn!(
                    "Tool '{}' has run for {} seconds and is approaching the {} second time limit ({} seconds remaining). It will be cancelled soon unless it completes.",
                    tool_name,
                    elapsed_secs,
                    timeout_secs,
                    remaining_secs
                );
            }
        }
    }))
}

fn cache_target_path(tool_name: &str, args: &Value) -> String {
    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(root) = args.get("root").and_then(|v| v.as_str()) {
        return root.to_string();
    }
    if let Some(target) = args.get("target_path").and_then(|v| v.as_str()) {
        return target.to_string();
    }
    if let Some(dir) = args.get("dir").and_then(|v| v.as_str()) {
        return dir.to_string();
    }

    tool_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use vtcode_core::acp::PermissionGrant;
    use vtcode_core::acp::permission_cache::ToolPermissionCache;
    use vtcode_core::core::decision_tracker::DecisionTracker;
    use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
    use vtcode_core::core::trajectory::TrajectoryLogger;
    use vtcode_core::tools::ApprovalRecorder;
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_core::tools::result_cache::ToolResultCache;
    use vtcode_core::ui::theme;
    use vtcode_core::ui::tui::{spawn_session, theme_from_styles};
    use vtcode_core::utils::ansi::AnsiRenderer;

    /// Helper function to create test registry with common setup
    async fn create_test_registry(workspace: &std::path::Path) -> ToolRegistry {
        ToolRegistry::new(workspace.to_path_buf()).await
    }

    /// Helper function to create test renderer with default config
    fn create_test_renderer(
        handle: &vtcode_core::ui::tui::InlineHandle,
    ) -> vtcode_core::utils::ansi::AnsiRenderer {
        AnsiRenderer::with_inline_ui(handle.clone(), Default::default())
    }

    /// Helper function to create common test context components
    struct TestContext {
        registry: ToolRegistry,
        renderer: vtcode_core::utils::ansi::AnsiRenderer,
        session: vtcode_core::ui::tui::InlineSession,
        handle: vtcode_core::ui::tui::InlineHandle,
        approval_recorder: vtcode_core::tools::ApprovalRecorder,
        workspace: std::path::PathBuf,
    }

    impl TestContext {
        async fn new() -> Self {
            let tmp = tempfile::TempDir::new().unwrap();
            let workspace = tmp.path().to_path_buf();

            let registry = create_test_registry(&workspace).await;
            let active_styles = theme::active_styles();
            let theme_spec = theme_from_styles(&active_styles);
            let session = spawn_session(
                theme_spec,
                None,
                vtcode_core::config::types::UiSurfacePreference::default(),
                10,
                false,
                None,
            )
            .unwrap();
            let handle = session.clone_inline_handle();
            let renderer = create_test_renderer(&handle);
            let approval_recorder = vtcode_core::tools::ApprovalRecorder::new(workspace.clone());

            Self {
                registry,
                renderer,
                session,
                handle,
                approval_recorder,
                workspace,
            }
        }
    }

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
    fn test_process_tool_output_loop_detection() {
        // Test loop detection output - should return Failure with clear message
        let output = json!({
            "error": {
                "tool_name": "read_file",
                "error_type": "PolicyViolation",
                "message": "Tool 'read_file' blocked after 5 identical invocations in recent history (limit: 5)",
                "is_recoverable": false,
                "recovery_suggestions": [],
                "original_error": null
            },
            "loop_detected": true,
            "repeat_count": 5,
            "tool": "read_file"
        });

        let status = process_tool_output(output);
        if let ToolExecutionStatus::Failure { error } = status {
            let error_msg = error.to_string();
            assert!(error_msg.contains("LOOP DETECTION"));
            assert!(error_msg.contains("read_file"));
            assert!(error_msg.contains("5"));
            assert!(error_msg.contains("DO NOT retry"));
            assert!(error_msg.contains("ACTION REQUIRED"));
        } else {
            panic!(
                "Expected Failure variant for loop detection, got: {:?}",
                status
            );
        }
    }

    #[tokio::test]
    async fn test_run_tool_call_unknown_tool_failure() {
        let mut test_ctx = TestContext::new().await;
        let mut registry = test_ctx.registry;

        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
        {
            let mut cache = permission_cache_arc.write().await;
            cache.cache_grant("test_tool".to_string(), PermissionGrant::Permanent);
        }

        let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
        let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
        let pruning_ledger = Arc::new(tokio::sync::RwLock::new(PruningDecisionLedger::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = test_ctx.approval_recorder;
        let traj = TrajectoryLogger::new(&test_ctx.workspace);

        let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

        let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
            renderer: &mut test_ctx.renderer,
            handle: &test_ctx.handle,
            tool_registry: &mut registry,
            tools: &tools,
            tool_result_cache: &result_cache,
            tool_permission_cache: &permission_cache_arc,
            decision_ledger: &decision_ledger,
            pruning_ledger: &pruning_ledger,
            session_stats: &mut session_stats,
            mcp_panel_state: &mut mcp_panel,
            approval_recorder: &approval_recorder,
            session: &mut test_ctx.session,
            traj: &traj,
        };

        let call = vtcode_core::llm::provider::ToolCall::function(
            "call_1".to_string(),
            "test_tool".to_string(),
            "{}".to_string(),
        );
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let outcome = run_tool_call(
            &mut ctx,
            &call,
            &ctrl_c_state,
            &ctrl_c_notify,
            None,
            None,
            true,
            &Arc::new(vtcode_core::core::token_budget::TokenBudgetManager::new(
                vtcode_core::core::token_budget::TokenBudgetConfig::for_model("gpt-5", 4096),
            )),
            None,
            0,
        )
        .await
        .expect("run_tool_call must run");

        assert!(matches!(
            outcome.status,
            ToolExecutionStatus::Failure { .. }
        ));
    }

    #[tokio::test]
    async fn test_run_tool_call_read_file_success() {
        use std::io::Write;
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();
        let file_path = workspace.join("sample.txt");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "hello world").unwrap();

        let mut registry = ToolRegistry::new(workspace.clone()).await;

        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
        {
            let mut cache = permission_cache_arc.write().await;
            cache.cache_grant("read_file".to_string(), PermissionGrant::Permanent);
        }

        let mut session = spawn_session(
            theme_from_styles(&theme::active_styles()),
            None,
            vtcode_core::config::types::UiSurfacePreference::default(),
            10,
            false,
            None,
        )
        .unwrap();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
        let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
        let pruning_ledger = Arc::new(tokio::sync::RwLock::new(PruningDecisionLedger::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let traj = TrajectoryLogger::new(&workspace);

        let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

        let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
            renderer: &mut renderer,
            handle: &handle,
            tool_registry: &mut registry,
            tools: &tools,
            tool_result_cache: &result_cache,
            tool_permission_cache: &permission_cache_arc,
            decision_ledger: &decision_ledger,
            pruning_ledger: &pruning_ledger,
            session_stats: &mut session_stats,
            mcp_panel_state: &mut mcp_panel,
            approval_recorder: &approval_recorder,
            session: &mut session,
            traj: &traj,
        };

        let args = serde_json::json!({"path": file_path.to_string_lossy()});
        let call = vtcode_core::llm::provider::ToolCall::function(
            "call_2".to_string(),
            "read_file".to_string(),
            serde_json::to_string(&args).unwrap(),
        );

        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let outcome = run_tool_call(
            &mut ctx,
            &call,
            &ctrl_c_state,
            &ctrl_c_notify,
            None,
            None,
            true,
            &Arc::new(vtcode_core::core::token_budget::TokenBudgetManager::new(
                vtcode_core::core::token_budget::TokenBudgetConfig::for_model("gpt-5", 4096),
            )),
            None,
            1,
        )
        .await
        .expect("read_file run_tool_call should succeed");

        match outcome.status {
            ToolExecutionStatus::Success { output, .. } => {
                assert_eq!(output.get("success").and_then(|v| v.as_bool()), Some(true));
            }
            other => panic!("Expected success, got: {:?}", other),
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

    // Note: This test requires tokio's test-util feature (start_paused, advance)
    // which is not enabled in the standard build. The test is commented out
    // to avoid compilation errors. To run it, enable tokio/test-util in Cargo.toml.
    //
    // #[tokio::test(start_paused = true)]
    // async fn emits_warning_before_timeout_ceiling() {
    //     let warnings = Arc::new(Mutex::new(Vec::new()));
    //     let writer_buffer = warnings.clone();
    //
    //     let subscriber = fmt()
    //         .with_writer(move || CaptureWriter::new(writer_buffer.clone()))
    //         .with_max_level(Level::WARN)
    //         .without_time()
    //         .finish();
    //
    //     let _guard = tracing::subscriber::set_default(subscriber);
    //
    //     let temp_dir = TempDir::new().expect("create temp dir");
    //     let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    //     registry
    //         .register_tool(ToolRegistration::new(
    //             "__test_slow_tool__",
    //             CapabilityLevel::Basic,
    //             false,
    //             slow_tool_executor,
    //         ))
    //         .expect("register slow tool");
    //
    //     let ctrl_c_state = Arc::new(CtrlCState::new());
    //     let ctrl_c_notify = Arc::new(Notify::new());
    //
    //     let mut registry_task = registry;
    //     let ctrl_c_state_clone = ctrl_c_state.clone();
    //     let ctrl_c_notify_clone = ctrl_c_notify.clone();
    //
    //     let execution = tokio::spawn(async move {
    //         execute_tool_with_timeout(
    //             &mut registry_task,
    //             "__test_slow_tool__",
    //             Value::Null,
    //             &ctrl_c_state_clone,
    //             &ctrl_c_notify_clone,
    //             None,
    //         )
    //         .await
    //     });
    //
    //     let default_timeout = Duration::from_secs(300);
    //     let warning_delay = default_timeout
    //         .checked_sub(TOOL_TIMEOUT_WARNING_HEADROOM)
    //         .expect("warning delay");
    //     advance(warning_delay).await;
    //     yield_now().await;
    //
    //     let captured = warnings.lock().unwrap();
    //     let combined = captured.join("");
    //     assert!(
    //         combined.contains("has run"),
    //         "expected warning log to include 'has run', captured logs: {}",
    //         combined
    //     );
    //     drop(captured);
    //
    //     advance(TOOL_TIMEOUT_WARNING_HEADROOM + Duration::from_secs(1)).await;
    //     let status = execution.await.expect("join execution");
    //     assert!(matches!(status, ToolExecutionStatus::Timeout { .. }));
    // }
}
