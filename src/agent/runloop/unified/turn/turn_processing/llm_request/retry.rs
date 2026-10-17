use vtcode_core::llm::provider as uni;

/// Delegate LLM retryability checks to the canonical [`vtcode_commons::ErrorCategory`] classifier.
#[cfg(test)]
pub(super) fn is_retryable_llm_error(message: &str) -> bool {
    vtcode_commons::is_retryable_llm_error_message(message)
}

/// Preserve typed HTTP metadata through context wrappers when classifying retries.
pub(super) fn classify_llm_error(error: &anyhow::Error) -> vtcode_commons::ErrorCategory {
    vtcode_commons::classify_anyhow_error(error)
}

const RECENT_TOOL_RESPONSE_WINDOW: usize = 10;
const TOOL_RETRY_MAX_CHARS: usize = 1200;

pub(super) fn is_stream_timeout_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("stream request timed out")
        || msg.contains("streaming request timed out")
        || msg.contains("first token timed out")
        || msg.contains("first progress timed out")
}

pub(super) fn is_previous_response_chain_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("previous_response_not_found")
        || msg.contains("previous response missing")
        || msg.contains("invalid previous_response_id")
        || msg.contains("invalid previous response id")
        || (msg.contains("previous response with id") && msg.contains("not found"))
        || (msg.contains("previous_response_id") && msg.contains("not found"))
}

/// Detect the provider protocol failure raised when a tool result is not
/// causally paired with an earlier assistant tool call. Keep this matcher
/// narrow so ordinary 400 validation errors continue through their existing
/// fail-fast path.
pub(crate) fn is_unmatched_tool_result_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let is_bad_request = lower.contains("400")
        && (lower.contains("bad request")
            || lower.contains("http 400")
            || lower.contains("status: 400")
            || lower.contains("status 400")
            || lower.contains("400:")
            || lower.contains("(400)"));
    let mentions_tool_result = lower.contains("tool result") || lower.contains("tool_result");
    let mentions_mismatch = lower.contains("unmatched")
        || lower.contains("no matching")
        || lower.contains("does not match")
        || lower.contains("not associated")
        || lower.contains("must be a response to");

    is_bad_request && mentions_tool_result && mentions_mismatch
}

pub(super) fn has_recent_tool_responses(messages: &[uni::Message]) -> bool {
    messages
        .iter()
        .rev()
        .take(RECENT_TOOL_RESPONSE_WINDOW)
        .any(|message| message.role == uni::MessageRole::Tool)
}

pub(super) fn compact_tool_messages_for_retry(messages: &[uni::Message]) -> Vec<uni::Message> {
    let mut compacted = Vec::with_capacity(messages.len());
    for message in messages {
        if message.role != uni::MessageRole::Tool {
            compacted.push(message.clone());
            continue;
        }

        let text = message.content.as_text();
        // Fast path: byte length ≤ max chars guarantees char count ≤ max chars
        // (every char is ≥ 1 byte), avoiding the O(n) char scan.
        if text.len() <= TOOL_RETRY_MAX_CHARS || text.chars().count() <= TOOL_RETRY_MAX_CHARS {
            compacted.push(message.clone());
            continue;
        }

        let mut truncated = text.chars().take(TOOL_RETRY_MAX_CHARS).collect::<String>();
        if truncated.len() < text.len() {
            truncated.push_str("\n... [tool output truncated for retry]");
        }

        let mut cloned = message.clone();
        cloned.content = uni::MessageContent::text(truncated);
        compacted.push(cloned);
    }

    if compacted.is_empty() {
        messages.to_vec()
    } else {
        compacted
    }
}

pub(crate) fn llm_first_progress_timeout_secs(
    turn_timeout_secs: u64,
    planning_active: bool,
    supports_non_streaming: bool,
    provider_name: &str,
) -> u64 {
    // A single slow first-token follow-up on a large context (common after many
    // accumulated tool outputs) should not burn all retries too aggressively.
    // After first progress arrives, the stream is allowed to run to completion.
    let baseline = (turn_timeout_secs / 5).clamp(30, 180);
    let merge_gateway = provider_name.eq_ignore_ascii_case("merge-gateway");
    if !planning_active {
        // Merge Gateway first-frame silence budget is 120s; abandoning earlier
        // lets the gateway drain+bill the provider call while VT Code retries
        // non-streaming and re-pays the full prompt.
        if merge_gateway && supports_non_streaming {
            return baseline.max(120);
        }
        return baseline;
    }

    // Planning workflow requests usually include heavier context and can need
    // extra first-token latency budget before retries are useful.
    let planning_floor = if supports_non_streaming { 90 } else { 60 };
    let planning_budget = (turn_timeout_secs / 2).clamp(planning_floor, 180);
    let budget = baseline.max(planning_budget);
    if merge_gateway && supports_non_streaming {
        // Documented Gateway silence window also applies to planning turns.
        return budget.max(120);
    }
    budget
}

pub(super) const DEFAULT_LLM_RETRY_ATTEMPTS: usize = 3;
pub(super) const MAX_LLM_RETRY_ATTEMPTS: usize = 6;

pub(super) fn llm_retry_attempts(configured_task_retries: Option<u32>) -> usize {
    configured_task_retries
        .and_then(|value| usize::try_from(value).ok())
        .map(|value| value.saturating_add(1))
        .unwrap_or(DEFAULT_LLM_RETRY_ATTEMPTS)
        .clamp(1, MAX_LLM_RETRY_ATTEMPTS)
}

pub(super) fn compact_error_message(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message.to_string();
    }
    // Bounded identifier-aware truncation: a naive char-prefix cut can split
    // a field name mid-token (`reasoning_cont...`), hiding whether the
    // provider rejected `reasoning_content`, `reasoning_continuation`, or
    // another field. When the cut lands inside an identifier token, extend to
    // the token end (bounded) so the distinguishing name survives while the
    // trajectory record stays within its bounded preview budget. Full bodies remain in
    // `tracing::warn!` at the call sites.
    const MAX_IDENTIFIER_EXTENSION_CHARS: usize = 32;
    let mut chars = message.chars();
    let mut preview: String = chars.by_ref().take(max_chars).collect();
    // Peek the remainder without materializing it: error bodies can be
    // kilobytes of JSON/HTML, and trajectory records must stay bounded.
    let mut remainder = chars.peekable();
    let cut_inside_identifier = preview
        .chars()
        .next_back()
        .is_some_and(|last| last.is_alphanumeric() || last == '_')
        && remainder
            .peek()
            .is_some_and(|next| next.is_alphanumeric() || *next == '_' || *next == '-');
    if cut_inside_identifier {
        for _ in 0..MAX_IDENTIFIER_EXTENSION_CHARS {
            match remainder.peek() {
                Some(ch) if ch.is_alphanumeric() || *ch == '_' || *ch == '-' => {
                    preview.push(*ch);
                    remainder.next();
                }
                _ => break,
            }
        }
    }
    preview.push_str("... [truncated]");
    preview
}

pub(super) fn switch_to_non_streaming_retry_mode(use_streaming: &mut bool, stream_fallback_used: &mut bool) {
    *use_streaming = false;
    *stream_fallback_used = true;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostToolRetryAction {
    SwitchToNonStreaming,
    CompactToolContext,
}

pub(super) fn next_post_tool_retry_action(
    use_streaming: bool,
    supports_non_streaming: bool,
    compacted_tool_retry_used: bool,
    preserve_structured_tool_context: bool,
) -> Option<PostToolRetryAction> {
    if use_streaming && supports_non_streaming {
        return Some(PostToolRetryAction::SwitchToNonStreaming);
    }

    if preserve_structured_tool_context {
        return None;
    }

    if !compacted_tool_retry_used {
        return Some(PostToolRetryAction::CompactToolContext);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_error_message_preserves_short_input() {
        assert_eq!(compact_error_message("boom", 180), "boom");
        assert_eq!(compact_error_message("", 0), "");
    }

    #[test]
    fn compact_error_message_extends_split_identifier() {
        // Cut lands inside `reasoning_content`: the distinguishing suffix
        // must survive within a bounded extension.
        let message = format!("{}reasoning_content and more detail here", "x".repeat(170));
        let compacted = compact_error_message(&message, 180);
        assert!(compacted.contains("reasoning_content"), "{compacted}");
        assert!(compacted.ends_with("... [truncated]"));
        assert!(compacted.chars().count() <= 180 + 32 + "... [truncated]".chars().count());
    }

    #[test]
    fn compact_error_message_keeps_whitespace_cut_stable() {
        // Asymmetric pair to the identifier case: a cut on whitespace must
        // not grow, preserving the previous prefix behavior.
        let message = format!("{} done and more detail here", "x".repeat(175));
        let compacted = compact_error_message(&message, 180);
        assert!(compacted.starts_with(&"x".repeat(175)));
        assert!(compacted.ends_with("... [truncated]"));
    }

    #[test]
    fn compact_error_message_bounds_long_identifier_tail() {
        let message = format!("err {}{}", "y".repeat(179), "_tail_with_many_chars_beyond_cap_and_more");
        let compacted = compact_error_message(&message, 180);
        assert!(compacted.ends_with("... [truncated]"));
        assert!(compacted.chars().count() <= 180 + 32 + "... [truncated]".chars().count());
    }
}
