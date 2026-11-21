//! Durable-history fixtures for the resumed computer-use contract.
//!
//! A completed computer turn persists as ordinary messages: the assistant's
//! `computer_use` call plus its redacted text summary. An interrupted turn
//! can persist the call without its response. Both must replay as valid
//! provider requests with no native items and no screenshot bytes. The
//! per-provider contract assertions live beside each request builder; these
//! fixtures are the shared shape both builders must accept.

use crate::provider::{Message, ToolCall};

/// The `computer_use` tool call id used by the fixtures.
pub(crate) const TOOL_CALL_ID: &str = "call_cu_resume_1";

/// History after a completed computer turn: the assistant's call and the
/// redacted text summary the turn loop persisted.
pub(crate) fn completed_computer_turn_history() -> Vec<Message> {
    vec![
        Message::user("Check the preview page for layout drift.".to_string()),
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                TOOL_CALL_ID.to_string(),
                "computer_use".to_string(),
                "{\"task\":\"open the preview page and confirm the header renders\"}".to_string(),
            )],
        ),
        Message::tool_response(
            TOOL_CALL_ID.to_string(),
            "Computer turn completed: 2 actions (click, type) on browser target; screenshot sent to the model."
                .to_string(),
        ),
        Message::user("Now summarize what you saw.".to_string()),
    ]
}

/// History after an interrupted computer turn: the call persisted without
/// its response.
pub(crate) fn interrupted_computer_turn_history() -> Vec<Message> {
    vec![
        Message::user("Check the preview page for layout drift.".to_string()),
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                TOOL_CALL_ID.to_string(),
                "computer_use".to_string(),
                "{\"task\":\"open the preview page and confirm the header renders\"}".to_string(),
            )],
        ),
    ]
}

/// Scan serialized provider output for native items and screenshot bytes
/// that must never reach a durable provider request from replayed
/// computer-use history.
#[must_use]
pub(crate) fn leaked_native_artifacts(serialized: &str) -> Vec<&'static str> {
    const LEAK_MARKERS: &[&str] = &[
        "computer_call\"",
        "\"computer_call",
        "computer_toolset_",
        "\"toolset_name\":\"computer\"",
        "data:image/",
        "iVBORw0KGgo",
    ];
    LEAK_MARKERS
        .iter()
        .copied()
        .filter(|marker| serialized.contains(marker))
        .collect()
}

#[cfg(test)]
mod tests;
