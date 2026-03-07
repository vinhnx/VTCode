use super::AgentRunner;
use crate::core::agent::task::{ContextItem, Task, TaskResults};
use crate::error::{ErrorCode, Result as VtCodeResult, VtCodeError};
use crate::retry::RetryPolicy;
use crate::utils::colors::style;
use tracing::{info, warn};

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
        use tokio::time::{Duration, sleep};

        let policy = RetryPolicy::from_retries(
            max_retries,
            Duration::from_secs(2),
            Duration::from_secs(30),
            2.0,
        );
        let metrics = self.tool_registry.metrics_collector();
        let mut last_error: Option<VtCodeError> = None;

        for attempt in 0..policy.max_attempts {
            info!(
                attempt = attempt + 1,
                max_attempts = policy.max_attempts,
                task_id = %task.id,
                "agent task attempt starting"
            );

            match self.execute_task(task, contexts).await {
                Ok(result) => {
                    if attempt > 0 {
                        metrics.record_retry_success();
                        // Notify user about successful retry
                        self.runner_println(format_args!(
                            "{} Task succeeded after {} attempt(s)",
                            style("[✓]").green().bold(),
                            attempt + 1
                        ));

                        info!(
                            attempt = attempt + 1,
                            task_id = %task.id,
                            "agent task succeeded after retry"
                        );
                    }
                    return Ok(result);
                }
                Err(err) => {
                    let typed_error = VtCodeError::from(err);
                    let decision = policy.decision_for_vtcode_error(&typed_error, attempt, None);
                    let exhausted_retryable_error =
                        typed_error.category.is_retryable() && attempt + 1 == policy.max_attempts;
                    last_error = Some(typed_error);

                    warn!(
                        attempt = attempt + 1,
                        max_attempts = policy.max_attempts,
                        task_id = %task.id,
                        error = %last_error.as_ref().expect("retry error should exist"),
                        category = ?decision.category,
                        "agent task attempt failed"
                    );

                    if !decision.retryable {
                        if exhausted_retryable_error {
                            metrics.record_retry_exhausted();
                        }
                        let err = last_error.expect("non-retryable error should exist");
                        warn!(
                            task_id = %task.id,
                            error = %err,
                            category = ?decision.category,
                            "non-retryable error"
                        );
                        return Err(err);
                    }

                    let backoff_duration = decision.delay.expect("retryable decisions need delay");
                    metrics.record_retry_attempt();

                    // Notify user about retry with visible message
                    self.runner_println(format_args!(
                        "{} Task failed (attempt {}/{}), retrying in {}s...",
                        style("[Warning]").red().bold(),
                        attempt + 1,
                        policy.max_attempts,
                        backoff_duration.as_secs()
                    ));

                    info!(
                        delay_ms = backoff_duration.as_millis() as u64,
                        next_attempt = attempt + 2,
                        task_id = %task.id,
                        category = ?decision.category,
                        "backing off before retry"
                    );

                    sleep(backoff_duration).await;
                }
            }
        }

        warn!(
            task_id = %task.id,
            attempts = policy.max_attempts,
            "agent task failed after all retries"
        );

        self.runner_println(format_args!(
            "{} Task failed after {} attempts",
            style("[Error]").red().bold(),
            policy.max_attempts
        ));

        Err(last_error.unwrap_or_else(|| {
            VtCodeError::execution(
                ErrorCode::ToolExecutionFailed,
                format!(
                    "agent task '{}' exhausted the retry loop without an error payload",
                    task.id
                ),
            )
        }))
    }
}
