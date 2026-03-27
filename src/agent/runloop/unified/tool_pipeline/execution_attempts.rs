use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Error;
use serde_json::Value;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use vtcode_core::retry::RetryPolicy;
use vtcode_core::tools::registry::ToolTimeoutCategory;
use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry};

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::wait_feedback::{
    WAIT_KEEPALIVE_INITIAL, WAIT_KEEPALIVE_INTERVAL, wait_keepalive_message,
};
use vtcode_core::exec::cancellation;

use super::execution_helpers::process_llm_tool_output;
use super::status::ToolExecutionStatus;
use super::timeout::{TimeoutWarningGuard, create_timeout_error};
use super::{DEFAULT_TOOL_TIMEOUT, MAX_RETRY_BACKOFF, RETRY_BACKOFF_BASE};

#[cfg(test)]
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

#[cfg(test)]
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
    settle_noninteractive_exec: bool,
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
        settle_noninteractive_exec,
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
    settle_noninteractive_exec: bool,
) -> ToolExecutionStatus {
    let created_local_reporter = progress_reporter.is_none();
    let fallback_progress_reporter = ProgressReporter::new();
    let progress_reporter = progress_reporter.unwrap_or(&fallback_progress_reporter);

    let timeout_category = registry.timeout_category_for(name).await;
    let timeout_ceiling = registry
        .timeout_policy()
        .ceiling_for(timeout_category)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT);
    let retry_allowed = is_retry_safe_tool(registry, name, args);
    let mut retry_policy = RetryPolicy::from_retries(
        max_tool_retries as u32,
        RETRY_BACKOFF_BASE,
        MAX_RETRY_BACKOFF,
        2.0,
    );
    retry_policy.jitter = 0.15;

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
        &retry_policy,
        prevalidated,
        settle_noninteractive_exec,
    )
    .await;

    if created_local_reporter {
        fallback_progress_reporter.complete().await;
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
    retry_policy: &RetryPolicy,
    prevalidated: bool,
    settle_noninteractive_exec: bool,
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
        settle_noninteractive_exec,
        attempt,
        None,
        retry_policy,
    )
    .await;

    while let Some(delay) = retry_delay_for_status(&status, attempt, retry_allowed, retry_policy) {
        attempt += 1;
        progress_reporter
            .set_message(format!(
                "Retrying {} (attempt {}/{}) after {}ms...",
                name,
                attempt + 1,
                retry_policy.max_attempts,
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
            settle_noninteractive_exec,
            attempt,
            Some(delay),
            retry_policy,
        )
        .await;
    }

    emit_tool_retry_outcome_metric(
        name,
        &status,
        attempt,
        retry_policy.max_attempts.saturating_sub(1) as usize,
        retry_allowed,
    );
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
    settle_noninteractive_exec: bool,
    attempt: usize,
    retry_delay: Option<Duration>,
    retry_policy: &RetryPolicy,
) -> ToolExecutionStatus {
    let attempt_start = Instant::now();
    let status = apply_retry_context(
        run_single_tool_attempt(
            registry,
            name,
            args,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
            timeout,
            prevalidated,
            settle_noninteractive_exec,
        )
        .await,
        name,
        args,
        attempt as u32,
        retry_policy,
    );

    debug!(
        target: "vtcode.tool.exec",
        tool = name,
        attempt = attempt + 1,
        status = status_label(&status),
        elapsed_ms = attempt_start.elapsed().as_millis(),
        retry_delay_ms = retry_delay.map(|d| d.as_millis()),
        category = status.error().map(|error| error.category.user_label()),
        retryable = status.error().map(|error| error.retryable),
        "tool attempt finished"
    );
    status
}

fn apply_retry_context(
    status: ToolExecutionStatus,
    name: &str,
    args: &Value,
    attempt_index: u32,
    retry_policy: &RetryPolicy,
) -> ToolExecutionStatus {
    match status {
        ToolExecutionStatus::Failure { error } => ToolExecutionStatus::Failure {
            error: retry_policy.apply_to_tool_execution_error(
                error
                    .with_tool_call_context(name, args)
                    .with_attempt(attempt_index + 1)
                    .with_surface("unified_runloop"),
                attempt_index,
                Some(name),
            ),
        },
        ToolExecutionStatus::Timeout { error } => ToolExecutionStatus::Timeout {
            error: retry_policy.apply_to_tool_execution_error(
                error
                    .with_tool_call_context(name, args)
                    .with_attempt(attempt_index + 1)
                    .with_surface("unified_runloop"),
                attempt_index,
                Some(name),
            ),
        },
        ToolExecutionStatus::Success { .. } | ToolExecutionStatus::Cancelled => status,
    }
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
    settle_noninteractive_exec: bool,
) -> ToolExecutionStatus {
    let start_time = Instant::now();
    let warning_fraction = registry.timeout_policy().warning_fraction();
    let mut warning_guard = TimeoutWarningGuard::new(
        name,
        start_time,
        tool_timeout,
        warning_fraction,
        Some(progress_reporter.clone()),
    );

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
                registry
                    .execute_public_tool_ref_prevalidated_with_exec_mode(
                        name,
                        args,
                        settle_noninteractive_exec,
                    )
                    .await
            } else {
                registry.execute_public_tool_ref(name, args).await
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

        let mut exec_future = Box::pin(tokio::time::timeout(tool_timeout, exec_future));
        let keepalive_started_at = tokio::time::Instant::now();
        let mut next_keepalive_at = keepalive_started_at + WAIT_KEEPALIVE_INITIAL;
        let wait_subject = format!("Tool '{}'", name);

        let control = loop {
            let cancel_notifier = ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);
            let keepalive_sleep = tokio::time::sleep_until(next_keepalive_at);
            tokio::pin!(keepalive_sleep);

            let control = tokio::select! {
                biased;
                _ = &mut cancel_notifier => {
                    if let Err(_e) = ctrl_c_state.check_cancellation() {
                        token.cancel();
                        ExecutionControl::Cancelled
                    } else {
                        token.cancel();
                        ExecutionControl::Continue
                    }
                }
                result = &mut exec_future => {
                    match result {
                        Ok(val) => ExecutionControl::Completed(val),
                        Err(_) => ExecutionControl::TimedOut,
                    }
                }
                _ = &mut keepalive_sleep => {
                    let elapsed = keepalive_started_at.elapsed();
                    progress_reporter
                        .set_message(wait_keepalive_message(&wait_subject, elapsed))
                        .await;
                    next_keepalive_at += WAIT_KEEPALIVE_INTERVAL;
                    continue;
                }
            };
            break control;
        };

        match control {
            ExecutionControl::Continue => continue,
            ExecutionControl::Cancelled => {
                terminate_active_exec_sessions(registry, name, "cancelled").await;
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
                        ToolExecutionStatus::Failure {
                            error: ToolExecutionError::from_anyhow(
                                name,
                                &error,
                                0,
                                false,
                                false,
                                Some("unified_runloop"),
                            )
                            .with_tool_call_context(name, args),
                        }
                    }
                };
            }
            ExecutionControl::TimedOut => {
                token.cancel();
                terminate_active_exec_sessions(registry, name, "timed out").await;
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

async fn terminate_active_exec_sessions(registry: &ToolRegistry, tool_name: &str, reason: &str) {
    if let Err(err) = registry.terminate_all_exec_sessions_async().await {
        debug!(
            target: "vtcode.tool.exec",
            tool = tool_name,
            cancel_reason = reason,
            error = %err,
            "failed to terminate exec sessions after tool interruption"
        );
    }
}

fn is_retry_safe_tool(registry: &ToolRegistry, name: &str, args: &Value) -> bool {
    registry.is_retry_safe_call(name, args)
}

fn retry_delay_for_status(
    status: &ToolExecutionStatus,
    attempt: usize,
    retry_allowed: bool,
    retry_policy: &RetryPolicy,
) -> Option<Duration> {
    if !retry_allowed {
        return None;
    }

    match status {
        ToolExecutionStatus::Timeout { error } | ToolExecutionStatus::Failure { error } => {
            retry_policy
                .decision_for_tool_execution_error(error, attempt as u32)
                .delay
        }
        _ => None,
    }
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
        let delay = RetryPolicy::from_retries(2, RETRY_BACKOFF_BASE, MAX_RETRY_BACKOFF, 2.0)
            .delay_for_attempt(0);
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
        let policy = RetryPolicy::from_retries(2, RETRY_BACKOFF_BASE, MAX_RETRY_BACKOFF, 2.0);

        assert!(retry_delay_for_status(&timeout_status, 0, true, &policy).is_some());
        assert!(retry_delay_for_status(&timeout_status, 0, false, &policy).is_none());
    }

    #[test]
    fn retry_delay_skips_policy_and_validation_failures() {
        let denied = ToolExecutionStatus::Failure {
            error: ToolExecutionError::from_anyhow(
                "tool",
                &anyhow!("tool denied by policy"),
                0,
                false,
                false,
                Some("test"),
            ),
        };
        let invalid = ToolExecutionStatus::Failure {
            error: ToolExecutionError::from_anyhow(
                "tool",
                &anyhow!("invalid arguments: missing field"),
                0,
                false,
                false,
                Some("test"),
            ),
        };
        let policy = RetryPolicy::from_retries(2, RETRY_BACKOFF_BASE, MAX_RETRY_BACKOFF, 2.0);

        assert!(retry_delay_for_status(&denied, 0, true, &policy).is_none());
        assert!(retry_delay_for_status(&invalid, 0, true, &policy).is_none());
    }

    #[test]
    fn retry_delay_allows_network_and_rate_limit_failures() {
        let network = ToolExecutionStatus::Failure {
            error: ToolExecutionError::from_anyhow(
                "tool",
                &anyhow!("network connection reset"),
                0,
                false,
                false,
                Some("test"),
            ),
        };
        let rate_limit = ToolExecutionStatus::Failure {
            error: ToolExecutionError::from_anyhow(
                "tool",
                &anyhow!("429 Too Many Requests"),
                0,
                false,
                false,
                Some("test"),
            ),
        };
        let policy = RetryPolicy::from_retries(2, RETRY_BACKOFF_BASE, MAX_RETRY_BACKOFF, 2.0);

        assert!(retry_delay_for_status(&network, 0, true, &policy).is_some());
        assert!(retry_delay_for_status(&rate_limit, 0, true, &policy).is_some());
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
