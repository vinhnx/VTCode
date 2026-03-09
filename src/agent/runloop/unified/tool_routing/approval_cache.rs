use std::time::Duration;

use serde_json::Value;
use vtcode_core::tools::{ApprovalRecorder, RiskLevel};

use super::shell_approval::ApprovalLearningTarget;

pub(super) const APPROVAL_RECORD_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) fn spawn_approval_record_task(
    recorder: &ApprovalRecorder,
    approval_target: &ApprovalLearningTarget,
    approved: bool,
) {
    // Intentionally detached: approval-pattern persistence is non-critical for the
    // current request path and bounded by a short timeout.
    let recorder = recorder.clone();
    let approval_key = approval_target.approval_key.clone();
    let display_label = approval_target.display_label.clone();
    tokio::spawn(async move {
        match tokio::time::timeout(
            APPROVAL_RECORD_TIMEOUT,
            recorder.record_approval(&approval_key, Some(&display_label), approved, None),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::debug!(
                    approval_key = %approval_key,
                    approved,
                    error = %err,
                    "Approval-pattern write failed"
                );
            }
            Err(_) => {
                tracing::debug!(
                    approval_key = %approval_key,
                    approved,
                    timeout_ms = APPROVAL_RECORD_TIMEOUT.as_millis(),
                    "Approval-pattern write timed out"
                );
            }
        }
    });
}

pub(super) fn approval_history_can_skip_prompt(
    hook_requires_prompt: bool,
    shell_approval_reason: Option<&str>,
    risk_level: RiskLevel,
) -> bool {
    !hook_requires_prompt && shell_approval_reason.is_none() && risk_level != RiskLevel::Critical
}

pub(super) fn cache_key(tool_name: &str, tool_args: Option<&Value>) -> String {
    super::permission_prompt::shell_permission_cache_suffix(tool_name, tool_args)
        .map(|suffix| format!("{tool_name}:{suffix}"))
        .unwrap_or_else(|| tool_name.to_string())
}
