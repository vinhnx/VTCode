use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, ToolTimeoutCategory};

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::wait_feedback::{
    resolve_warning_delay, wait_timeout_warning_message,
};

use super::{MIN_TIMEOUT_WARNING_HEADROOM, ToolExecutionStatus};

/// Guard that ensures timeout warning tasks are cancelled when the tool attempt ends early
pub(super) struct TimeoutWarningGuard {
    cancel_token: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl TimeoutWarningGuard {
    pub(super) fn new(
        tool_name: &str,
        start_time: Instant,
        tool_timeout: Duration,
        warning_fraction: f32,
        progress_reporter: Option<ProgressReporter>,
    ) -> Self {
        let cancel_token = CancellationToken::new();
        let handle = spawn_timeout_warning_task(
            tool_name.to_string(),
            start_time,
            cancel_token.clone(),
            tool_timeout,
            warning_fraction,
            progress_reporter,
        );
        Self {
            cancel_token,
            handle,
        }
    }

    pub(super) async fn cancel(&mut self) {
        self.cancel_token.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

/// Create a timeout error for a tool execution
pub(crate) fn create_timeout_error(
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

pub(super) fn spawn_timeout_warning_task(
    tool_name: String,
    start_time: Instant,
    cancel_token: CancellationToken,
    tool_timeout: Duration,
    warning_fraction: f32,
    progress_reporter: Option<ProgressReporter>,
) -> Option<JoinHandle<()>> {
    let fraction = warning_fraction.clamp(0.1, 0.95);
    let warning_delay = resolve_warning_delay(
        tool_timeout,
        tool_timeout.mul_f32(fraction),
        MIN_TIMEOUT_WARNING_HEADROOM,
    )?;

    Some(tokio::spawn(async move {
        tokio::select! {
            _ = cancel_token.cancelled() => {}
            _ = tokio::time::sleep(warning_delay) => {
                let elapsed_secs = start_time.elapsed().as_secs();
                let timeout_secs = tool_timeout.as_secs();
                let remaining_secs = tool_timeout
                    .saturating_sub(Duration::from_secs(elapsed_secs))
                    .as_secs();
                if let Some(progress_reporter) = progress_reporter {
                    let wait_subject = format!("Tool '{}'", tool_name);
                    progress_reporter
                        .set_message(wait_timeout_warning_message(
                            &wait_subject,
                            tool_timeout,
                            Duration::from_secs(remaining_secs),
                        ))
                        .await;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_warning_updates_progress_message_after_delay() {
        let reporter = ProgressReporter::new();
        let start = Instant::now();
        let handle = spawn_timeout_warning_task(
            "unified_exec".to_string(),
            start,
            CancellationToken::new(),
            Duration::from_millis(5_200),
            0.1,
            Some(reporter.clone()),
        )
        .expect("warning task should spawn");

        tokio::time::sleep(Duration::from_millis(50)).await;
        let before = reporter.progress_info().await;
        assert!(before.message.is_empty());

        let after = wait_for_message(&reporter).await;
        assert!(after.message.contains("is nearing the"));
        assert!(
            after
                .message
                .contains(vtcode_commons::stop_hints::STOP_HINT_INLINE)
        );

        handle.await.expect("warning task should complete");
    }

    #[tokio::test]
    async fn cancelled_timeout_warning_does_not_update_progress_message() {
        let reporter = ProgressReporter::new();
        let cancel_token = CancellationToken::new();
        let handle = spawn_timeout_warning_task(
            "unified_exec".to_string(),
            Instant::now(),
            cancel_token.clone(),
            Duration::from_millis(5_200),
            0.1,
            Some(reporter.clone()),
        )
        .expect("warning task should spawn");

        cancel_token.cancel();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let progress = reporter.progress_info().await;
        assert!(progress.message.is_empty());

        handle.await.expect("warning task should complete");
    }

    async fn wait_for_message(
        reporter: &ProgressReporter,
    ) -> crate::agent::runloop::unified::progress::ProgressInfo {
        for _ in 0..10 {
            let info = reporter.progress_info().await;
            if !info.message.is_empty() {
                return info;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        reporter.progress_info().await
    }
}
