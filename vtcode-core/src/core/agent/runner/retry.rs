use super::AgentRunner;
use crate::core::agent::task::{ContextItem, Task, TaskResults};
use crate::utils::colors::style;
use anyhow::{Result, anyhow};
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
    ) -> Result<TaskResults> {
        use crate::core::orchestrator_retry::is_retryable_error;
        use tokio::time::{Duration, sleep};

        let mut delay_secs = 2u64;
        let max_delay_secs = 30u64;
        let backoff_multiplier = 2.0f64;

        for attempt in 0..=max_retries {
            info!(
                attempt = attempt + 1,
                max_attempts = max_retries + 1,
                task_id = %task.id,
                "agent task attempt starting"
            );

            match self.execute_task(task, contexts).await {
                Ok(result) => {
                    if attempt > 0 {
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
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = max_retries + 1,
                        task_id = %task.id,
                        error = %err,
                        "agent task attempt failed"
                    );

                    // Check if this error should be retried
                    if !is_retryable_error(&err) {
                        warn!(task_id = %task.id, error = %err, "non-retryable error");
                        return Err(err);
                    }

                    // If this is not the last attempt, wait before retrying
                    if attempt < max_retries {
                        let backoff_duration = Duration::from_secs(delay_secs);

                        // Notify user about retry with visible message
                        self.runner_println(format_args!(
                            "{} Task failed (attempt {}/{}), retrying in {}s...",
                            style("[Warning]").red().bold(),
                            attempt + 1,
                            max_retries + 1,
                            delay_secs
                        ));

                        info!(
                            delay_secs,
                            next_attempt = attempt + 2,
                            task_id = %task.id,
                            "backing off before retry"
                        );

                        sleep(backoff_duration).await;

                        // Apply exponential backoff with cap
                        delay_secs = std::cmp::min(
                            (delay_secs as f64 * backoff_multiplier) as u64,
                            max_delay_secs,
                        );
                    } else {
                        // Last attempt failed
                        warn!(
                            task_id = %task.id,
                            attempts = max_retries + 1,
                            "agent task failed after all retries"
                        );

                        self.runner_println(format_args!(
                            "{} Task failed after {} attempts",
                            style("[Error]").red().bold(),
                            max_retries + 1
                        ));

                        return Err(anyhow!(
                            "Agent task '{}' failed after {} attempts: {}",
                            task.id,
                            max_retries + 1,
                            err
                        ));
                    }
                }
            }
        }

        unreachable!("Retry loop should always return within the loop")
    }
}
