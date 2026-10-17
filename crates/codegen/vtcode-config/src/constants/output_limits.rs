//! Output truncation limits inspired by OpenAI Codex multi-tier truncation.
//! Prevents OOM with three independent size limits.
//! Reference: <https://openai.com/index/unrolling-the-codex-agent-loop/>

/// Maximum size for single agent message payloads (bytes) - 4 MB.
pub const MAX_AGENT_MESSAGES_SIZE: usize = 4 * 1024 * 1024;

/// Maximum size for entire message history payloads (bytes) - 24 MB.
pub const MAX_ALL_MESSAGES_SIZE: usize = 24 * 1024 * 1024;

/// Maximum retained error-log payload bytes in the in-memory collector (10 MiB).
pub const ERROR_LOG_BUFFER_SIZE_LIMIT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum size per line (bytes) - 256 KB.
/// Prevents OOM on malformed output with very long lines.
pub const MAX_LINE_LENGTH: usize = 256 * 1024;

/// Default message count limit for history.
pub const DEFAULT_MESSAGE_LIMIT: usize = 4_000;

/// Maximum message count limit.
pub const MAX_MESSAGE_LIMIT: usize = 20_000;

/// Maximum fallback preview size for one execution result (64 KiB).
/// Historical names are retained for API compatibility; these are per-result
/// limits, never an aggregate allowance. History compaction bounds accumulated
/// context without disabling tools or hiding subsequent evidence.
pub const TURN_PREVIEW_BUDGET_BYTES: usize = 64 * 1024;
/// Maximum fallback preview size for one planning result (96 KiB).
pub const TURN_PREVIEW_BUDGET_BYTES_PLANNING: usize = 96 * 1024;

/// Effective per-result preview limit for the active workflow mode.
#[inline]
pub const fn turn_preview_budget_bytes(planning_active: bool) -> usize {
    if planning_active {
        TURN_PREVIEW_BUDGET_BYTES_PLANNING
    } else {
        TURN_PREVIEW_BUDGET_BYTES
    }
}

/// Legacy small-preview threshold retained for API compatibility.
/// Per-result limiting no longer needs a separate verifier reserve.
pub const TINY_PREVIEW_BYPASS_BYTES: usize = 1024;

/// Legacy verifier reserve retained for API compatibility; no longer enforced.
pub const TURN_TINY_PREVIEW_BUDGET_BYTES: usize = 8 * 1024;

/// Truncation marker appended when content is cut off.
const TRUNCATION_MARKER: &str = "\n[... content truncated due to size limit ...]";

/// Collect content with lazy truncation (Codex pattern).
/// Marks truncated but continues draining to prevent pipe blocking.
///
/// # Arguments
/// * `output` - Accumulated output buffer
/// * `new_content` - New content to append
/// * `max_size` - Maximum allowed size
/// * `truncated` - Mutable flag tracking truncation state
///
/// # Returns
/// `true` if content was appended, `false` if truncated
#[inline]
fn collect_with_truncation(output: &mut String, new_content: &str, max_size: usize, truncated: &mut bool) -> bool {
    let new_size = output.len() + new_content.len();

    if new_size > max_size {
        if !*truncated {
            output.push_str(TRUNCATION_MARKER);
            *truncated = true;
        }
        return false;
    }

    output.push_str(new_content);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_within_limit() {
        let mut output = String::new();
        let mut truncated = false;

        assert!(collect_with_truncation(&mut output, "hello", 100, &mut truncated));
        assert_eq!(output, "hello");
        assert!(!truncated);
    }

    #[test]
    fn test_collect_at_limit_triggers_truncation() {
        let mut output = String::from("hello");
        let mut truncated = false;

        assert!(!collect_with_truncation(&mut output, " world that exceeds", 10, &mut truncated));
        assert!(output.contains(TRUNCATION_MARKER));
        assert!(truncated);
    }

    #[test]
    fn test_truncation_marker_appended_only_once() {
        let mut output = String::new();
        let mut truncated = false;

        // Triggers initial truncation
        collect_with_truncation(&mut output, "first content", 5, &mut truncated);
        let len_after_marker = output.len();
        assert!(truncated);

        // Should not append marker again
        collect_with_truncation(&mut output, "second content", 5, &mut truncated);
        assert_eq!(output.len(), len_after_marker);
    }

    #[test]
    fn turn_preview_budget_splits_by_workflow_mode() {
        assert_eq!(turn_preview_budget_bytes(false), 64 * 1024);
        assert_eq!(turn_preview_budget_bytes(true), 96 * 1024);
    }
}
