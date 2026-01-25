use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, ToolTimeoutCategory};

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
