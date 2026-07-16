use std::time::Duration;

use serde_json::Value;
use url::Url;
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

/// Extract the domain from a `web_fetch` URL argument for cache key purposes.
///
/// Returns `Some("example.com")` for `https://example.com/path`, `None` if the
/// URL is missing or unparseable. The domain is normalised to lowercase so that
/// `https://Example.COM/` and `https://example.com/` share one cache entry.
pub(super) fn web_fetch_domain(tool_args: Option<&Value>) -> Option<String> {
    let url = tool_args?.as_object()?.get("url")?.as_str()?;
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

pub(super) fn cache_key(tool_name: &str, tool_args: Option<&Value>) -> String {
    // For non-shell tools, the shell suffix is None, so the key normally
    // degrades to the bare tool name.
    if let Some(suffix) =
        super::permission_prompt::shell_permission_cache_suffix(tool_name, tool_args)
    {
        return format!("{tool_name}:{suffix}");
    }

    use vtcode_core::config::constants::tools::{CODE_SEARCH, FETCH_URL, WEB_FETCH};

    if tool_name == CODE_SEARCH
        && let Some(args) = tool_args
        && let Some(identity) = vtcode_core::tools::normalised_code_search_identity(args)
    {
        return format!("{tool_name}:{identity}");
    }

    // For web_fetch / fetch_url, key by domain so that approving
    // `https://example.com` does not auto-approve `https://other.com`.
    if (tool_name == WEB_FETCH || tool_name == FETCH_URL)
        && let Some(domain) = web_fetch_domain(tool_args)
    {
        return format!("{tool_name}:{domain}");
    }

    tool_name.to_string()
}
