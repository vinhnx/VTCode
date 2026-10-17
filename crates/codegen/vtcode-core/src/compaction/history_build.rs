//! Compacted-history builders and provider-detail stripping.

use super::*;

pub(crate) fn response_compaction_detail(response: &LLMResponse) -> Option<Value> {
    response.reasoning_details.as_ref()?.iter().find_map(|detail| {
        let parsed = serde_json::from_str::<Value>(detail).ok()?;
        let parsed = match parsed {
            Value::String(serialized) => serde_json::from_str::<Value>(&serialized).ok()?,
            value => value,
        };
        let summary = parsed
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        (parsed.get("type").and_then(Value::as_str) == Some("compaction") && summary.is_some()).then_some(parsed)
    })
}

pub(crate) fn build_summary_compacted_history(
    history: &[Message],
    summary: impl AsRef<str>,
    config: &CompactionConfig,
    include_continuity_tail: bool,
    tail_target_tokens: usize,
) -> Vec<Message> {
    build_compacted_history_with_leading(
        history,
        Message::system(format!("{SUMMARY_PREFIX}{}", summary.as_ref().trim())),
        config,
        include_continuity_tail,
        tail_target_tokens,
        false,
    )
}

pub(crate) fn build_provider_compacted_history(
    history: &[Message],
    compaction_detail: Value,
    config: &CompactionConfig,
    include_continuity_tail: bool,
    tail_target_tokens: usize,
) -> Vec<Message> {
    let signed_compaction = compaction_detail
        .get("signature")
        .and_then(Value::as_str)
        .is_some_and(|signature| !signature.trim().is_empty());
    let history_without_provider_compaction = history
        .iter()
        .cloned()
        .filter_map(strip_provider_compaction_detail)
        .collect::<Vec<_>>();
    build_compacted_history_with_leading(
        &history_without_provider_compaction,
        Message::assistant(String::new()).with_reasoning_details(Some(vec![compaction_detail])),
        config,
        include_continuity_tail,
        tail_target_tokens,
        signed_compaction,
    )
}

fn build_compacted_history_with_leading(
    history: &[Message],
    leading: Message,
    config: &CompactionConfig,
    include_continuity_tail: bool,
    tail_target_tokens: usize,
    strip_pre_compaction_thinking: bool,
) -> Vec<Message> {
    let (retention_history, continuity) = split_continuity_history_with_target(history, tail_target_tokens);
    let retained_users = collect_retained_user_messages(
        retention_history,
        config.retained_user_message_tokens,
        config.retained_user_messages,
    );
    let retained_users = if strip_pre_compaction_thinking {
        retained_users.into_iter().map(strip_anthropic_thinking_details).collect()
    } else {
        retained_users
    };
    let mut compacted = Vec::with_capacity(retained_users.len().saturating_add(1));
    compacted.push(leading);
    compacted.extend(retained_users);
    if include_continuity_tail {
        for message in continuity {
            compacted.push(if strip_pre_compaction_thinking {
                strip_anthropic_thinking_details(message)
            } else {
                message
            });
        }
    }
    compacted
}

fn strip_anthropic_thinking_details(mut message: Message) -> Message {
    if message.role != MessageRole::Assistant {
        return message;
    }

    let Some(details) = message.reasoning_details.take() else {
        return message;
    };
    let retained = details
        .into_iter()
        .filter(|detail| !is_reasoning_detail_type(detail, "thinking"))
        .filter(|detail| !is_reasoning_detail_type(detail, "redacted_thinking"))
        .collect::<Vec<_>>();
    message.reasoning_details = (!retained.is_empty()).then_some(retained);
    message
}

fn strip_provider_compaction_detail(mut message: Message) -> Option<Message> {
    if message.role != MessageRole::Assistant {
        return Some(message);
    }

    let Some(details) = message.reasoning_details.take() else {
        return Some(message);
    };
    let retained = details
        .into_iter()
        .filter(|detail| !is_compaction_detail(detail))
        .collect::<Vec<_>>();
    message.reasoning_details = (!retained.is_empty()).then_some(retained);

    let has_content = !message.content.trim().is_empty();
    let has_reasoning = message.reasoning.as_ref().is_some_and(|reasoning| !reasoning.trim().is_empty());
    let has_tool_calls = message.tool_calls.as_ref().is_some_and(|tool_calls| !tool_calls.is_empty());
    let has_other_metadata = message.tool_call_id.is_some()
        || message.phase.is_some()
        || message.origin_tool.is_some()
        || message.metadata.is_some()
        || message.clear_at.is_some();
    (has_content || has_reasoning || message.reasoning_details.is_some() || has_tool_calls || has_other_metadata)
        .then_some(message)
}

pub(crate) fn is_compaction_detail(detail: &Value) -> bool {
    is_reasoning_detail_type(detail, "compaction")
}

fn is_reasoning_detail_type(detail: &Value, expected_type: &str) -> bool {
    let value = match detail {
        Value::String(serialized) => serde_json::from_str::<Value>(serialized).ok(),
        value => Some(value.clone()),
    };
    value.as_ref().and_then(|value| value.get("type")).and_then(Value::as_str) == Some(expected_type)
}

pub(crate) fn bounded_protocol_group(group: &[Message], token_budget: usize) -> Vec<Message> {
    let per_message_budget = (token_budget / group.len().max(1)).max(4);
    group
        .iter()
        .map(|message| bounded_message_preview(message, per_message_budget))
        .collect()
}
