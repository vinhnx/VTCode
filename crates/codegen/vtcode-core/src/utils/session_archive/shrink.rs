use anyhow::Result;

use super::message::SessionMessage;
use super::metadata::{SessionArchiveMetadata, SessionContinuationRecommendedAction};
use super::settings::{history_persistence_enabled, session_history_settings};
use super::snapshot::SessionSnapshot;
use crate::llm::provider::MessageContent;

pub(crate) fn truncate_preview(input: &str, max_chars: usize) -> String {
    vtcode_commons::formatting::truncate_within(input, max_chars, "…")
}

pub(crate) fn compact_snapshot_to_max_bytes(
    mut snapshot: SessionSnapshot,
    max_bytes: usize,
) -> Result<SessionSnapshot> {
    if max_bytes == 0 {
        minimize_snapshot_payload(&mut snapshot);
        return Ok(snapshot);
    }

    while serde_json::to_vec(&snapshot)?.len() > max_bytes {
        if trim_oldest_snapshot_entries(&mut snapshot) {
            continue;
        }
        if strip_snapshot_overhead(&mut snapshot) {
            continue;
        }
        if shrink_snapshot_strings(&mut snapshot) {
            continue;
        }
        break;
    }

    if serde_json::to_vec(&snapshot)?.len() > max_bytes {
        minimize_snapshot_payload(&mut snapshot);
        let _ = shrink_snapshot_strings(&mut snapshot);
    }

    Ok(snapshot)
}

pub(crate) fn prepare_snapshot_for_write(snapshot: SessionSnapshot) -> Result<Option<SessionSnapshot>> {
    if !history_persistence_enabled() {
        return Ok(None);
    }

    let max_bytes = session_history_settings().max_bytes;
    let snapshot = match max_bytes {
        Some(max_bytes) => compact_snapshot_to_max_bytes(snapshot, max_bytes)?,
        None => snapshot,
    };

    Ok(Some(snapshot))
}

fn minimize_snapshot_payload(snapshot: &mut SessionSnapshot) {
    snapshot.messages.clear();
    snapshot.transcript.clear();
    snapshot.distinct_tools.clear();
    snapshot.error_logs.clear();
    if let Some(progress) = snapshot.progress.as_mut() {
        progress.recent_messages.clear();
        progress.tool_summaries.clear();
        progress.token_usage = None;
        progress.max_context_tokens = None;
        progress.loaded_skills.clear();
    }
}

fn trim_oldest_snapshot_entries(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = false;

    if snapshot.messages.len() > 1 {
        snapshot.messages.remove(0);
        changed = true;
    }

    if snapshot.transcript.len() > 1 {
        snapshot.transcript.remove(0);
        changed = true;
    }

    if let Some(progress) = snapshot.progress.as_mut()
        && progress.recent_messages.len() > 1
    {
        progress.recent_messages.remove(0);
        changed = true;
    }

    changed
}

fn strip_snapshot_overhead(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = false;

    if !snapshot.transcript.is_empty() {
        snapshot.transcript.clear();
        changed = true;
    }
    if !snapshot.distinct_tools.is_empty() {
        snapshot.distinct_tools.clear();
        changed = true;
    }
    if !snapshot.error_logs.is_empty() {
        snapshot.error_logs.clear();
        changed = true;
    }

    if let Some(progress) = snapshot.progress.as_mut() {
        if !progress.tool_summaries.is_empty() {
            progress.tool_summaries.clear();
            changed = true;
        }
        if progress.token_usage.take().is_some() {
            changed = true;
        }
        if progress.max_context_tokens.take().is_some() {
            changed = true;
        }
        if !progress.loaded_skills.is_empty() {
            progress.loaded_skills.clear();
            changed = true;
        }
    }

    changed
}

fn shrink_snapshot_strings(snapshot: &mut SessionSnapshot) -> bool {
    let mut changed = shrink_snapshot_metadata(&mut snapshot.metadata);

    for transcript in &mut snapshot.transcript {
        changed |= shrink_string(transcript);
    }

    for message in &mut snapshot.messages {
        changed |= shrink_session_message(message);
    }

    for error_log in &mut snapshot.error_logs {
        changed |= shrink_string(&mut error_log.message);
    }

    if let Some(progress) = snapshot.progress.as_mut() {
        for message in &mut progress.recent_messages {
            changed |= shrink_session_message(message);
        }
        if let Some(token_usage) = progress.token_usage.as_mut() {
            changed |= shrink_string(token_usage);
        }
    }

    changed
}

fn shrink_session_message(message: &mut SessionMessage) -> bool {
    let mut changed = false;
    changed |= shrink_message_content(&mut message.content);

    if let Some(reasoning) = message.reasoning.as_mut() {
        changed |= shrink_string(reasoning);
    }
    if message.reasoning_details.take().is_some() {
        changed = true;
    }
    if let Some(tool_call_id) = message.tool_call_id.as_mut() {
        changed |= shrink_string(tool_call_id);
    }
    if let Some(origin_tool) = message.origin_tool.as_mut() {
        changed |= shrink_string(origin_tool);
    }
    if let Some(tool_calls) = message.tool_calls.as_mut() {
        for tool_call in tool_calls.iter_mut() {
            changed |= shrink_string(&mut tool_call.id);
            changed |= shrink_string(&mut tool_call.call_type);
            if let Some(function) = tool_call.function.as_mut() {
                changed |= shrink_string(&mut function.name);
                changed |= shrink_string(&mut function.arguments);
            }
            if let Some(text) = tool_call.text.as_mut() {
                changed |= shrink_string(text);
            }
            if let Some(thought_signature) = tool_call.thought_signature.as_mut() {
                changed |= shrink_string(thought_signature);
            }
        }
    }

    changed
}

fn shrink_snapshot_metadata(metadata: &mut SessionArchiveMetadata) -> bool {
    let mut changed = false;

    changed |= shrink_string(&mut metadata.workspace_label);
    changed |= shrink_string(&mut metadata.workspace_path);
    changed |= shrink_string(&mut metadata.model);
    changed |= shrink_string(&mut metadata.provider);
    changed |= shrink_string(&mut metadata.theme);
    changed |= shrink_string(&mut metadata.reasoning_effort);
    changed |= shrink_optional_string(&mut metadata.debug_log_path);
    changed |= shrink_optional_string(&mut metadata.prompt_cache_lineage_id);
    changed |= shrink_optional_string(&mut metadata.external_thread_id);
    changed |= shrink_optional_string(&mut metadata.parent_session_id);
    changed |= shrink_optional_string(&mut metadata.primary_agent);

    for skill in &mut metadata.loaded_skills {
        changed |= shrink_string(skill);
    }

    if let Some(continuation) = metadata.continuation_metadata.as_mut() {
        if continuation.max_budget_usd_micros == Some(0) {
            continuation.max_budget_usd_micros = None;
            changed = true;
        }
        if continuation.actual_cost_usd_micros == Some(0) {
            continuation.actual_cost_usd_micros = None;
            changed = true;
        }
        if !continuation.summary_available
            && continuation.recommended_action == Some(SessionContinuationRecommendedAction::ContinueFromSummary)
        {
            continuation.recommended_action = None;
            changed = true;
        }
    }

    changed
}

fn shrink_message_content(content: &mut MessageContent) -> bool {
    match content {
        MessageContent::Text(text) => shrink_string(text),
        MessageContent::Parts(parts) => {
            let mut changed = false;
            for part in parts {
                changed |= match part {
                    crate::llm::provider::ContentPart::Text { text } => shrink_string(text),
                    crate::llm::provider::ContentPart::Image { data, mime_type, .. } => {
                        shrink_string(data) | shrink_string(mime_type)
                    }
                    crate::llm::provider::ContentPart::File { filename, file_id, file_data, file_url, .. } => {
                        shrink_optional_string(filename)
                            | shrink_optional_string(file_id)
                            | shrink_optional_string(file_data)
                            | shrink_optional_string(file_url)
                    }
                };
            }
            changed
        }
    }
}

fn shrink_optional_string(value: &mut Option<String>) -> bool {
    value.as_mut().is_some_and(shrink_string)
}

fn shrink_string(value: &mut String) -> bool {
    const MIN_RETAINED_CHARS: usize = 8;
    const TRUNCATION_MARKER: &str = "...";

    if value.len() <= MIN_RETAINED_CHARS + TRUNCATION_MARKER.len() {
        return false;
    }

    let keep_len = (value.len() / 2).max(MIN_RETAINED_CHARS);
    let prefix_len = keep_len.saturating_sub(TRUNCATION_MARKER.len());
    value.truncate(prefix_len);
    value.push_str(TRUNCATION_MARKER);
    true
}
