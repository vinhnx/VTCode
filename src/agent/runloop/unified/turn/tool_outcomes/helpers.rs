use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use std::collections::HashMap;
use vtcode_core::llm::provider as uni;

pub(crate) fn limit_conversation_history(history: &mut Vec<uni::Message>, limit: usize) {
    if history.len() > limit {
        let remove_count = history.len() - limit;
        history.drain(0..remove_count);
    }
}

pub(crate) fn push_tool_response(
    history: &mut Vec<uni::Message>,
    tool_call_id: String,
    content: String,
) {
    history.push(uni::Message::tool_response(
        tool_call_id,
        content,
    ));
}

pub(crate) fn push_assistant_message(
    history: &mut Vec<uni::Message>,
    message: uni::Message,
) {
    history.push(message);
}

pub(crate) fn signature_key_for(name: &str, args: &serde_json::Value) -> String {
    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    format!("{}:{}", name, args_str)
}

pub(crate) fn resolve_max_tool_retries(
    _tool_name: &str,
    _vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
    // TODO: Re-implement per-tool retry configuration once config structure is verified.
    // Currently AgentConfig does not expose a 'tools' map.
    3
}

pub(crate) fn reasoning_duplicates_content(cleaned_reasoning: &str, content: &str) -> bool {
    let cleaned_content = vtcode_core::llm::providers::clean_reasoning_text(content);
    !cleaned_reasoning.is_empty()
        && !cleaned_content.is_empty()
        && cleaned_reasoning == cleaned_content
}

/// Updates the tool repetition tracker based on the execution outcome.
///
/// Only successful tool calls are counted towards repetition limits.
/// Failed, timed out, or cancelled calls are ignored for this purpose.
pub(crate) fn update_repetition_tracker(
    repeated_tool_attempts: &mut HashMap<String, usize>,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
) {
    if matches!(
        &outcome.status,
        ToolExecutionStatus::Success { .. }
    ) {
        let signature_key = signature_key_for(name, args);
        let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
        *current_count += 1;
    }
}
