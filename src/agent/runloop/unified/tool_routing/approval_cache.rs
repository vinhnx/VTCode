use std::time::Duration;

use serde_json::Value;
use vtcode_core::tools::ApprovalRecorder;

use super::shell_approval::ApprovalLearningTarget;

pub(super) const APPROVAL_RECORD_TIMEOUT: Duration = Duration::from_millis(500);

/// Record an approval decision and wait for in-memory state to update.
///
/// Records under every key the target contributes — the exact invocation
/// first, then any broader [`LearnedPattern`](super::shell_approval::LearnedPattern)
/// family key — so future equivalent invocations can hit the auto-approve
/// classifier on either match.
///
/// This must run synchronously (not detached) so the next permission check in
/// the same session observes the incremented approval count and the
/// auto-approve classifier (`ApprovalRecorder::should_auto_approve`) can
/// promote the next invocation to `HitlDecision::Approved` without prompting.
/// The short per-write timeout bounds the async-lock acquisition path; the
/// actual disk persistence happens behind a synchronous file write and is not
/// preempted by the timeout — the JSON cache is intentionally tiny.
pub(super) async fn record_approval_blocking(
    recorder: &ApprovalRecorder,
    approval_target: &ApprovalLearningTarget,
    approved: bool,
) {
    for (key, label) in approval_target.iter_keys() {
        match tokio::time::timeout(
            APPROVAL_RECORD_TIMEOUT,
            recorder.record_approval(key, Some(label), approved, None),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::debug!(
                    approval_key = %key,
                    approved,
                    error = %err,
                    "Approval-pattern write failed"
                );
            }
            Err(_) => {
                tracing::debug!(
                    approval_key = %key,
                    approved,
                    timeout_ms = APPROVAL_RECORD_TIMEOUT.as_millis(),
                    "Approval-pattern write timed out"
                );
            }
        }
    }
}

pub(super) fn cache_key(tool_name: &str, tool_args: Option<&Value>) -> String {
    super::permission_prompt::shell_permission_cache_suffix(tool_name, tool_args)
        .map(|suffix| format!("{tool_name}:{suffix}"))
        .unwrap_or_else(|| tool_name.to_string())
}
