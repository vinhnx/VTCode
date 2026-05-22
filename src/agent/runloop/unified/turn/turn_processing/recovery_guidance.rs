use std::path::Path;

use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool
            || message.tool_call_id.is_some()
            || message.tool_calls.is_some()
    })
}

pub(super) fn empty_response_recovery_mode(history: &[uni::Message]) -> RecoveryMode {
    if has_recent_tool_activity(history) {
        RecoveryMode::ToolFreeSynthesis
    } else {
        RecoveryMode::ToolEnabledRetry
    }
}

pub(super) fn empty_response_recovery_reason(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => {
            "Model returned no answer. Continue autonomously with the next concrete action now. Tools remain available if needed; do not stop with a status update."
        }
        RecoveryMode::ToolFreeSynthesis => {
            "Model returned no answer after tool activity. Tools are disabled on the next pass; provide a direct textual response from the current context."
        }
    }
}

pub(super) fn empty_response_notice(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => {
            "[!] Empty model response detected; scheduling a retry pass with tools still enabled."
        }
        RecoveryMode::ToolFreeSynthesis => {
            "[!] Empty model response detected; scheduling a final recovery pass."
        }
    }
}

fn recovery_empty_response_fallback_intro(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => {
            "I couldn't continue because the model returned no answer twice in a row."
        }
        RecoveryMode::ToolFreeSynthesis => {
            "I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
        }
    }
}

fn recovery_empty_response_fallback_guidance(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => "Retry the turn from the current context.",
        RecoveryMode::ToolFreeSynthesis => {
            "Reuse the latest tool outputs already collected in this turn before retrying, and follow any `hint`, `next_action`, `fallback_tool`, or `fallback_tool_args` they already provide."
        }
    }
}

pub(super) fn recovery_empty_response_fallback_message(
    history: &[uni::Message],
    workspace_root: &Path,
    mode: RecoveryMode,
) -> String {
    let intro = recovery_empty_response_fallback_intro(mode);
    let guidance = recovery_empty_response_fallback_guidance(mode);

    let previews = crate::agent::runloop::unified::turn::compaction::build_recovery_context_previews_with_workspace(
        history,
        Some(workspace_root),
    );
    if previews.is_empty() {
        format!("{intro}\n\n{guidance}")
    } else if previews.len() == 1 {
        format!("{intro}\n\n{}\n\n{guidance}", previews[0])
    } else {
        format!("{intro}\n\n{}\n\n{guidance}", previews.join("\n"))
    }
}
