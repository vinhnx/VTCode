use super::*;

pub(super) fn last_tool_response_text(history: &[uni::Message], call_id: &str, window_start: usize) -> Option<String> {
    history.get(window_start..)?.iter().rev().find_map(|message| {
        (message.role == uni::MessageRole::Tool && message.tool_call_id.as_deref() == Some(call_id))
            .then(|| message.content.as_text().to_string())
    })
}

pub(super) fn verifier_response_has_nonzero_exit(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("exit_code").and_then(serde_json::Value::as_i64))
        .is_some_and(|code| code != 0)
}

/// Only a fresh yielded result for this owned session permits another wait.
/// A retained handle alone cannot prove that the latest admission succeeded.
pub(super) fn verifier_response_is_running_session(text: &str, session_id: &str) -> bool {
    use vtcode_core::tools::command_args::session_id_text;
    let Ok(output) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let returned_session = session_id_text(&output)
        .or_else(|| output.get("next_wait_args").and_then(session_id_text))
        .or_else(|| output.get("next_continue_args").and_then(session_id_text));
    returned_session == Some(session_id)
        && output.get("exit_code").and_then(serde_json::Value::as_i64).is_none()
        && output.get("error").is_none()
        && output.get("success").and_then(serde_json::Value::as_bool) != Some(false)
        && ![
            "cancelled",
            "blocked",
            "not_executed",
            "termination_requested",
            "reused_recent_result",
        ]
        .iter()
        .any(|key| output.get(*key).and_then(serde_json::Value::as_bool) == Some(true))
}
