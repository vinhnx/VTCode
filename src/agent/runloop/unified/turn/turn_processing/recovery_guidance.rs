use std::path::Path;

use vtcode_core::llm::provider::{self as uni, AssistantPhase};

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool || message.tool_call_id.is_some() || message.tool_calls.is_some()
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
        RecoveryMode::ToolFreeSynthesis => "[!] Empty model response detected; scheduling a final recovery pass.",
    }
}

fn recovery_empty_response_fallback_intro(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => "I couldn't continue because the model returned no answer twice in a row.",
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

/// Last-resort fallback used when `recovery_empty_response_fallback_message`
/// somehow returns empty or whitespace-only content. Ensures the user always
/// sees a non-empty final answer rather than a silent blank line.
pub(super) fn recovery_empty_fallback_safety_message(mode: RecoveryMode) -> String {
    match mode {
        RecoveryMode::ToolEnabledRetry => "I couldn't generate a response on this turn because the model returned an \
             empty answer twice in a row. Please retry the request. The prior turn \
             state has been preserved."
            .to_string(),
        RecoveryMode::ToolFreeSynthesis => "I couldn't synthesize a final answer from this turn. The most recent tool \
             outputs (if any) are in the conversation history above. Please retry with \
             a more specific question or rephrase the request."
            .to_string(),
    }
}

/// Push a recovery fallback assistant message directly into the working
/// history, bypassing `handle_assistant_response`'s skip-empty guard. The
/// fallback is the user's only view of what happened when the model returns
/// nothing during recovery; without forcing the push, the conversation can
/// end with a silent blank assistant message.
pub(super) fn push_recovery_fallback_assistant_message(
    history: &mut Vec<uni::Message>,
    content: &str,
    phase: Option<AssistantPhase>,
) {
    if content.trim().is_empty() {
        return;
    }
    let msg = uni::Message::assistant(content.to_string()).with_phase(phase);
    history.push(msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_message_is_non_empty_for_all_modes() {
        for mode in [RecoveryMode::ToolEnabledRetry, RecoveryMode::ToolFreeSynthesis] {
            let msg = recovery_empty_fallback_safety_message(mode);
            assert!(!msg.trim().is_empty(), "safety message must be non-empty");
            assert!(msg.len() > 40, "safety message should be substantial (got {} chars)", msg.len());
        }
    }

    #[test]
    fn fallback_message_always_includes_intro_and_guidance() {
        for mode in [RecoveryMode::ToolEnabledRetry, RecoveryMode::ToolFreeSynthesis] {
            let intro = recovery_empty_response_fallback_intro(mode);
            let guidance = recovery_empty_response_fallback_guidance(mode);
            assert!(!intro.trim().is_empty());
            assert!(!guidance.trim().is_empty());
        }
    }

    #[test]
    fn push_recovery_fallback_skips_empty_content() {
        let mut history: Vec<uni::Message> = Vec::new();
        push_recovery_fallback_assistant_message(&mut history, "", None);
        push_recovery_fallback_assistant_message(&mut history, "   \n  ", None);
        assert!(history.is_empty());
    }

    #[test]
    fn push_recovery_fallback_includes_phase_when_provided() {
        let mut history: Vec<uni::Message> = Vec::new();
        push_recovery_fallback_assistant_message(&mut history, "summary", Some(AssistantPhase::FinalAnswer));
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].phase, Some(AssistantPhase::FinalAnswer));
        assert_eq!(history[0].content.as_text().as_ref(), "summary");
    }
}
