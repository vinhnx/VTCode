use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use vtcode_core::tools::registry::ToolTimeoutCategory;
use vtcode_core::tools::registry::{ToolErrorType, ToolRegistry, classify_error};

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use vtcode_core::exec::cancellation;

use super::execution_helpers::process_llm_tool_output;
use super::status::ToolExecutionStatus;
use super::timeout::{TimeoutWarningGuard, create_timeout_error};
use super::{DEFAULT_TOOL_TIMEOUT, MAX_RETRY_BACKOFF, RETRY_BACKOFF_BASE};

#[allow(dead_code)]
pub(crate) async fn execute_tool_with_timeout(
    registry: &ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    execute_tool_with_timeout_ref(
        registry,
        name,
        &args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        max_tool_retries,
    )
    .await
}

pub(crate) async fn execute_tool_with_timeout_ref(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    execute_tool_with_timeout_ref_mode(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        max_tool_retries,
        false,
    )
    .await
}

pub(crate) async fn execute_tool_with_timeout_ref_prevalidated(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    execute_tool_with_timeout_ref_mode(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        max_tool_retries,
        true,
    )
    .await
}

async fn execute_tool_with_timeout_ref_mode(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
    prevalidated: bool,
) -> ToolExecutionStatus {
    let mut local_progress_reporter = None;
    let progress_reporter = if let Some(reporter) = progress_reporter {
        reporter
    } else {
        local_progress_reporter = Some(ProgressReporter::new());
        local_progress_reporter
            .as_ref()
            .expect("local reporter exists")
    };

    let timeout_category = registry.timeout_category_for(name).await;
    let timeout_ceiling = registry
        .timeout_policy()
        .ceiling_for(timeout_category)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT);
    let retry_allowed = is_retry_safe_tool(registry, name, args);

    if !retry_allowed && max_tool_retries > 0 {
        debug!(
            target: "vtcode.tool.exec",
            tool = name,
            "tool classified as non-idempotent; retries disabled"
        );
    }

    let result = execute_tool_with_progress(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        timeout_ceiling,
        timeout_category,
        retry_allowed,
        max_tool_retries,
        prevalidated,
    )
    .await;

    if let Some(ref local_reporter) = local_progress_reporter {
        local_reporter.complete().await;
    }
    result
}

async fn execute_tool_with_progress(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
    timeout_category: ToolTimeoutCategory,
    retry_allowed: bool,
    max_tool_retries: usize,
    prevalidated: bool,
) -> ToolExecutionStatus {
    let deadline = Instant::now() + tool_timeout;
    let mut attempt = 0usize;
    let mut status = run_attempt_with_logging(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        deadline.saturating_duration_since(Instant::now()),
        prevalidated,
        attempt,
        None,
    )
    .await;

    while let Some(delay) =
        retry_delay_for_status(&status, attempt, max_tool_retries, retry_allowed)
    {
        attempt += 1;
        progress_reporter
            .set_message(format!(
                "Retrying {} (attempt {}/{}) after {}ms...",
                name,
                attempt + 1,
                max_tool_retries + 1,
                delay.as_millis()
            ))
            .await;

        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = ctrl_c_notify.notified() => return ToolExecutionStatus::Cancelled,
        }

        let remaining_timeout = deadline.saturating_duration_since(Instant::now());
        if remaining_timeout < Duration::from_secs(1) {
            return create_timeout_error(name, timeout_category, Some(tool_timeout));
        }

        status = run_attempt_with_logging(
            registry,
            name,
            args,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
            remaining_timeout,
            prevalidated,
            attempt,
            Some(delay),
        )
        .await;
    }

    emit_tool_retry_outcome_metric(name, &status, attempt, max_tool_retries, retry_allowed);
    status
}

async fn run_attempt_with_logging(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    timeout: Duration,
    prevalidated: bool,
    attempt: usize,
    retry_delay: Option<Duration>,
) -> ToolExecutionStatus {
    let attempt_start = Instant::now();
    let status = run_single_tool_attempt(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        timeout,
        prevalidated,
    )
    .await;

    debug!(
        target: "vtcode.tool.exec",
        tool = name,
        attempt = attempt + 1,
        status = status_label(&status),
        elapsed_ms = attempt_start.elapsed().as_millis(),
        retry_delay_ms = retry_delay.map(|d| d.as_millis()),
        "tool attempt finished"
    );
    status
}

async fn run_single_tool_attempt(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
    prevalidated: bool,
) -> ToolExecutionStatus {
    let start_time = Instant::now();
    let warning_fraction = registry.timeout_policy().warning_fraction();
    let mut warning_guard =
        TimeoutWarningGuard::new(name, start_time, tool_timeout, warning_fraction);

    progress_reporter
        .set_message(format!("Preparing {}...", name))
        .await;
    progress_reporter.set_progress(5).await;

    if let Err(_e) = ctrl_c_state.check_cancellation() {
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

    let _progress_update_guard = {
        use crate::agent::runloop::unified::progress::{
            ProgressUpdateGuard, spawn_elapsed_time_updater,
        };
        let handle = spawn_elapsed_time_updater(progress_reporter.clone(), name.to_string(), 500);
        ProgressUpdateGuard::new(handle)
    };

    let status = loop {
        if let Err(_e) = ctrl_c_state.check_cancellation() {
            progress_reporter
                .set_message(format!("{} cancelled", name))
                .await;
            progress_reporter.set_progress(100).await;
            warning_guard.cancel().await;
            break ToolExecutionStatus::Cancelled;
        }

        progress_reporter
            .set_message(format!("Executing {}...", name))
            .await;

        let token = CancellationToken::new();
        let exec_future = cancellation::with_tool_cancellation(token.clone(), async {
            progress_reporter.set_progress(40).await;

            let result = if prevalidated {
                registry.execute_tool_ref_prevalidated(name, args).await
            } else {
                registry.execute_tool_ref(name, args).await
            };

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
            Completed(Result<Value, Error>),
            TimedOut,
            Cancelled,
        }

        let control = tokio::select! {
            biased;
            _ = ctrl_c_notify.notified() => {
                if let Err(_e) = ctrl_c_state.check_cancellation() {
                    token.cancel();
                    ExecutionControl::Cancelled
                } else {
                    token.cancel();
                    ExecutionControl::Continue
                }
            }
            result = tokio::time::timeout(tool_timeout, exec_future) => {
                match result {
                    Ok(val) => ExecutionControl::Completed(val),
                    Err(_) => ExecutionControl::TimedOut,
                }
            },
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
                    Ok(output) => {
                        progress_reporter
                            .set_message(format!("{} completed", name))
                            .await;
                        progress_reporter.set_progress(100).await;
                        process_llm_tool_output(output)
                    }
                    Err(error) => {
                        progress_reporter
                            .set_message(format!("{} failed", name))
                            .await;
                        ToolExecutionStatus::Failure { error }
                    }
                };
            }
            ExecutionControl::TimedOut => {
                token.cancel();
                progress_reporter
                    .set_message(format!("{} timed out", name))
                    .await;
                let timeout_category = registry.timeout_category_for(name).await;
                break create_timeout_error(name, timeout_category, Some(tool_timeout));
            }
        }
    };

    warning_guard.cancel().await;
    status
}

fn is_retry_safe_tool(registry: &ToolRegistry, name: &str, args: &Value) -> bool {
    registry.is_retry_safe_call(name, args)
}

fn retry_delay_for_status(
    status: &ToolExecutionStatus,
    attempt: usize,
    max_tool_retries: usize,
    retry_allowed: bool,
) -> Option<Duration> {
    if !retry_allowed || attempt >= max_tool_retries {
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
            if should_retry_error_type(error_type) {
                Some(backoff_for_attempt(attempt))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn should_retry_error_type(error_type: ToolErrorType) -> bool {
    matches!(
        error_type,
        ToolErrorType::Timeout | ToolErrorType::NetworkError
    )
}

fn backoff_for_attempt(attempt: usize) -> Duration {
    let retry_number = attempt.saturating_add(1);
    let exp = 2_u64.saturating_pow(retry_number.saturating_sub(1).min(4) as u32);
    let jitter = Duration::from_millis(((retry_number as u64 * 53) % 150) + 75);
    let backoff = RETRY_BACKOFF_BASE
        .saturating_mul(exp as u32)
        .saturating_add(jitter);
    std::cmp::max(backoff, Duration::from_millis(350)).min(MAX_RETRY_BACKOFF)
}

fn status_label(status: &ToolExecutionStatus) -> &'static str {
    match status {
        ToolExecutionStatus::Success { .. } => "success",
        ToolExecutionStatus::Failure { .. } => "failure",
        ToolExecutionStatus::Timeout { .. } => "timeout",
        ToolExecutionStatus::Cancelled => "cancelled",
    }
}

fn emit_tool_retry_outcome_metric(
    tool_name: &str,
    status: &ToolExecutionStatus,
    retries_used: usize,
    max_tool_retries: usize,
    retry_allowed: bool,
) {
    let success = matches!(status, ToolExecutionStatus::Success { .. });
    if retries_used == 0 && success {
        return;
    }

    let attempts_made = retries_used.saturating_add(1);
    let exhausted_retry_budget =
        !success && retry_allowed && retries_used >= max_tool_retries && max_tool_retries > 0;
    tracing::info!(
        target: "vtcode.tool.metrics",
        metric = "tool_retry_outcome",
        tool = tool_name,
        attempts_made,
        retries_used,
        max_tool_retries,
        retry_allowed,
        success,
        exhausted_retry_budget,
        final_status = status_label(status),
        "tool metric"
    );
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn first_retry_backoff_is_non_zero_and_meaningful() {
        let delay = backoff_for_attempt(0);
        assert!(delay >= Duration::from_millis(350));
        assert!(delay <= MAX_RETRY_BACKOFF);
    }

    #[test]
    fn retry_delay_honors_retry_safety_gate() {
        let timeout_status = create_timeout_error(
            "read_file",
            ToolTimeoutCategory::Default,
            Some(Duration::from_secs(1)),
        );

        assert!(retry_delay_for_status(&timeout_status, 0, 2, true).is_some());
        assert!(retry_delay_for_status(&timeout_status, 0, 2, false).is_none());
    }

    #[test]
    fn retry_delay_skips_policy_and_validation_failures() {
        let denied = ToolExecutionStatus::Failure {
            error: anyhow!("tool denied by policy"),
        };
        let invalid = ToolExecutionStatus::Failure {
            error: anyhow!("invalid arguments: missing field"),
        };

        assert!(retry_delay_for_status(&denied, 0, 2, true).is_none());
        assert!(retry_delay_for_status(&invalid, 0, 2, true).is_none());
    }

    #[test]
    fn retry_delay_allows_network_failures() {
        let network = ToolExecutionStatus::Failure {
            error: anyhow!("network connection reset"),
        };

        assert!(retry_delay_for_status(&network, 0, 2, true).is_some());
    }

    #[tokio::test]
    async fn retry_safety_allows_read_only_and_blocks_mutating() {
        let temp_dir = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(is_retry_safe_tool(
            &registry,
            vtcode_core::config::constants::tools::READ_FILE,
            &json!({"path": "Cargo.toml"})
        ));
        assert!(!is_retry_safe_tool(
            &registry,
            vtcode_core::config::constants::tools::WRITE_FILE,
            &json!({"path": "scratch.txt", "content": "x"})
        ));
    }
}
