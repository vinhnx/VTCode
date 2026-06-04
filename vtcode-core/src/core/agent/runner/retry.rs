use super::AgentRunner;
use crate::core::agent::task::{ContextItem, Task, TaskResults};
use crate::error::{ErrorCode, Result as VtCodeResult, VtCodeError};
use crate::retry::{RetryEvent, RetryPolicy, run_with_retry};
use crate::utils::colors::style;
use tracing::{info, warn};

/// Per-retry-loop context for the agent runner. Held by `&mut` inside
/// `run_with_retry` so the `on_event` and `operation` callbacks can
/// share access to the runner's mutable state without splitting borrows.
struct AgentRetryContext<'a> {
    runner: &'a mut AgentRunner,
    metrics: std::sync::Arc<crate::metrics::MetricsCollector>,
    policy_max_attempts: u32,
    task_id: String,
}

impl AgentRunner {
    /// Execute a task with automatic retry on transient failures
    ///
    /// Wraps `execute_task` with retry logic using exponential backoff.
    /// Retries only occur for transient errors (timeouts, network issues, 5xx errors).
    /// Non-retryable errors (auth failures, invalid requests) fail immediately.
    pub async fn execute_task_with_retry(
        &mut self,
        task: &Task,
        contexts: &[ContextItem],
        max_retries: u32,
    ) -> VtCodeResult<TaskResults> {
        use std::time::Duration;

        let policy = RetryPolicy::from_retries(
            max_retries,
            Duration::from_secs(2),
            Duration::from_secs(30),
            2.0,
        );
        let metrics = self.tool_registry.metrics_collector();
        let task_id = task.id.clone();
        let mut ctx = AgentRetryContext {
            runner: self,
            metrics,
            policy_max_attempts: policy.max_attempts,
            task_id: task_id.clone(),
        };

        run_with_retry(
            &policy,
            &mut ctx,
            |ctx, event| match event {
                RetryEvent::AttemptStart {
                    attempt,
                    max_attempts,
                } => {
                    info!(
                        attempt = attempt + 1,
                        max_attempts,
                        task_id = %ctx.task_id,
                        "agent task attempt starting"
                    );
                }
                RetryEvent::Success { attempt } if attempt > 0 => {
                    ctx.metrics.record_retry_success();
                    ctx.runner.runner_println(format_args!(
                        "{} Task succeeded after {} attempt(s)",
                        style("[✓]").green().bold(),
                        attempt + 1
                    ));
                    info!(
                        attempt = attempt + 1,
                        task_id = %ctx.task_id,
                        "agent task succeeded after retry"
                    );
                }
                RetryEvent::Success { .. } => {}
                RetryEvent::GiveUp {
                    attempt,
                    error,
                    decision,
                    category_was_retryable,
                } => {
                    if category_was_retryable && attempt + 1 == ctx.policy_max_attempts {
                        ctx.metrics.record_retry_exhausted();
                    }
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = ctx.policy_max_attempts,
                        task_id = %ctx.task_id,
                        error = %error,
                        category = ?decision.category,
                        "agent task attempt failed (non-retryable)"
                    );
                }
                RetryEvent::Backoff {
                    attempt,
                    error,
                    decision,
                    delay,
                    ..
                } => {
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = ctx.policy_max_attempts,
                        task_id = %ctx.task_id,
                        error = %error,
                        category = ?decision.category,
                        "agent task attempt failed"
                    );
                    ctx.metrics.record_retry_attempt();
                    ctx.runner.runner_println(format_args!(
                        "{} Task failed (attempt {}/{}), retrying in {}s...",
                        style("[Warning]").red().bold(),
                        attempt + 1,
                        ctx.policy_max_attempts,
                        delay.as_secs()
                    ));
                    info!(
                        delay_ms = delay.as_millis() as u64,
                        next_attempt = attempt + 2,
                        task_id = %ctx.task_id,
                        category = ?decision.category,
                        "backing off before retry"
                    );
                }
                RetryEvent::Exhausted { .. } => {
                    warn!(
                        task_id = %ctx.task_id,
                        attempts = ctx.policy_max_attempts,
                        "agent task failed after all retries"
                    );
                    ctx.runner.runner_println(format_args!(
                        "{} Task failed after {} attempts",
                        style("[Error]").red().bold(),
                        ctx.policy_max_attempts
                    ));
                }
            },
            |ctx| {
                let task = task.clone();
                let contexts = contexts.to_vec();
                let runner = &mut *ctx.runner;
                Box::pin(async move { runner.execute_task(&task, &contexts).await })
            },
            move |_policy| {
                VtCodeError::execution(
                    ErrorCode::ToolExecutionFailed,
                    format!(
                        "agent task '{task_id}' exhausted the retry loop without an error payload"
                    ),
                )
            },
        )
        .await
    }
}
