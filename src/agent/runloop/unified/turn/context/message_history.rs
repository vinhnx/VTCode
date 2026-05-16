use vtcode_commons::diff_paths::{
    is_diff_addition_line, is_diff_deletion_line, is_diff_header_line,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::providers::ReasoningSegment;

pub(super) fn reasoning_duplicates_content(reasoning: &str, content: &str) -> bool {
    let r = reasoning.trim();
    let c = content.trim();
    if r.is_empty() || c.is_empty() {
        return false;
    }
    r == c || r.contains(c) || c.contains(r)
}

pub(super) fn build_combined_reasoning(
    reasoning: &[ReasoningSegment],
    detail_reasoning: Option<&str>,
) -> Option<String> {
    let capacity = reasoning
        .iter()
        .map(|segment| segment.text.len())
        .sum::<usize>()
        + reasoning.len().saturating_sub(1);
    let mut combined_reasoning = String::with_capacity(capacity);

    for segment in reasoning {
        if !combined_reasoning.is_empty() {
            combined_reasoning.push('\n');
        }
        combined_reasoning.push_str(&segment.text);
    }

    if combined_reasoning.trim().is_empty()
        && let Some(detail_reasoning) = detail_reasoning
    {
        return Some(detail_reasoning.to_string());
    }

    if combined_reasoning.is_empty() {
        None
    } else {
        Some(combined_reasoning)
    }
}

pub(super) fn parse_reasoning_detail_value(detail: &str) -> serde_json::Value {
    let trimmed = detail.trim();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        return parsed;
    }
    serde_json::Value::String(detail.to_string())
}

pub(super) fn push_assistant_message(history: &mut Vec<uni::Message>, msg: uni::Message) {
    if let Some(last) = history.last_mut()
        && last.role == uni::MessageRole::Assistant
        && last.tool_calls.is_none()
        && last.phase == msg.phase
    {
        last.content = msg.content;
        last.reasoning = msg.reasoning;
        last.reasoning_details = msg.reasoning_details;
    } else {
        history.push(msg);
    }
}

pub(super) fn should_suppress_redundant_diff_recap(
    history: &[uni::Message],
    assistant_text: &str,
) -> bool {
    if assistant_text.trim().is_empty() {
        return false;
    }
    if !is_redundant_diff_recap_text(assistant_text) {
        return false;
    }
    if !has_recent_git_diff_tool_output(history) {
        return false;
    }
    if !last_user_requested_diff_view(history) {
        return false;
    }
    if last_user_requested_diff_analysis(history) {
        return false;
    }
    true
}

fn is_redundant_diff_recap_text(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("diff for ")
        || lower.starts_with("the diff shows")
        || lower.starts_with("changes in ")
        || lower.starts_with("```diff")
        || lower.starts_with("diff preview changes")
        || lower.contains("\n**diff preview changes**")
        || (trimmed.contains("```") && is_diff_like_fenced_recap(trimmed))
}

fn is_diff_like_fenced_recap(text: &str) -> bool {
    let mut has_fence = false;
    let mut has_diff_marker = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            has_fence = true;
            continue;
        }
        if is_diff_header_line(trimmed)
            || is_diff_addition_line(trimmed)
            || is_diff_deletion_line(trimmed)
        {
            has_diff_marker = true;
        }
    }
    has_fence && has_diff_marker
}

fn has_recent_git_diff_tool_output(history: &[uni::Message]) -> bool {
    history
        .iter()
        .rev()
        .take(12)
        .any(message_is_git_diff_tool_output)
}

fn message_is_git_diff_tool_output(message: &uni::Message) -> bool {
    if message.role != uni::MessageRole::Tool {
        return false;
    }

    let content = message.content.as_text();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content.as_ref()) {
        if value
            .get("content_type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|content_type| content_type == "git_diff")
        {
            return true;
        }
        if value
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| command.trim_start().starts_with("git diff"))
        {
            return true;
        }
        if value
            .get("output")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|output| output.contains("diff --git "))
        {
            return true;
        }
    }

    content.contains("\"content_type\":\"git_diff\"") || content.contains("diff --git ")
}

fn last_user_message_text(history: &[uni::Message]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .map(|message| message.content.as_text().to_ascii_lowercase())
}

fn last_user_requested_diff_view(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    [
        "show diff",
        "git diff",
        "view diff",
        "show changes",
        "what changed",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn last_user_requested_diff_analysis(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    ["analy", "explain", "summar", "review", "why", "interpret"]
        .iter()
        .any(|needle| text.contains(needle))
}