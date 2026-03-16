use vtcode_core::llm::provider as uni;

/// Delegate LLM retryability checks to the canonical [`vtcode_commons::ErrorCategory`] classifier.
#[cfg(test)]
pub(super) fn is_retryable_llm_error(message: &str) -> bool {
    vtcode_commons::is_retryable_llm_error_message(message)
}

/// Classify an LLM error message into an [`vtcode_commons::ErrorCategory`] for
/// structured logging and user-facing hints.
pub(super) fn classify_llm_error(message: &str) -> vtcode_commons::ErrorCategory {
    vtcode_commons::classify_error_message(message)
}

const STREAM_TIMEOUT_FALLBACK_PROVIDERS: &[&str] = &[
    "huggingface",
    "ollama",
    "minimax",
    "deepseek",
    "moonshot",
    "zai",
    "openrouter",
    "lmstudio",
];

const RECENT_TOOL_RESPONSE_WINDOW: usize = 10;
const TOOL_RETRY_MAX_CHARS: usize = 1200;

pub(super) fn supports_streaming_timeout_fallback(provider_name: &str) -> bool {
    STREAM_TIMEOUT_FALLBACK_PROVIDERS
        .iter()
        .any(|provider| provider_name.eq_ignore_ascii_case(provider))
}

pub(super) fn is_stream_timeout_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("stream request timed out")
        || msg.contains("streaming request timed out")
        || msg.contains("llm request timed out after")
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
        if text.chars().count() <= TOOL_RETRY_MAX_CHARS {
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

pub(super) fn llm_attempt_timeout_secs(
    turn_timeout_secs: u64,
    plan_mode: bool,
    provider_name: &str,
) -> u64 {
    let baseline = (turn_timeout_secs / 5).clamp(30, 120);
    if !plan_mode {
        return baseline;
    }

    // Plan Mode requests usually include heavier context and can need
    // extra first-token latency budget before retries are useful.
    let plan_mode_floor = if supports_streaming_timeout_fallback(provider_name) {
        90
    } else {
        60
    };
    let plan_mode_budget = (turn_timeout_secs / 2).clamp(plan_mode_floor, 120);
    baseline.max(plan_mode_budget)
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
    let mut preview = message.chars().take(max_chars).collect::<String>();
    preview.push_str("... [truncated]");
    preview
}

pub(super) fn switch_to_non_streaming_retry_mode(
    use_streaming: &mut bool,
    stream_fallback_used: &mut bool,
) {
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
