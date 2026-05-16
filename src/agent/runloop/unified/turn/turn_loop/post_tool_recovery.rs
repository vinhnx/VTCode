//! Turn-loop helpers for recovering after tool output when the follow-up LLM phase fails.

use anyhow::Result;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::{
    POST_TOOL_RECOVERY_REASON, POST_TOOL_RESUME_DIRECTIVE,
    RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostToolFailureRecovery {
    NotApplicable,
    RetryToolFree,
    StopAfterDirective,
}

pub(super) fn has_tool_response_since(messages: &[uni::Message], baseline_len: usize) -> bool {
    messages
        .get(baseline_len..)
        .is_some_and(|recent| recent.iter().any(|msg| msg.role == uni::MessageRole::Tool))
}

fn ensure_recent_system_message(working_history: &mut Vec<uni::Message>, content: &str) {
    let already_present = working_history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == content
    });
    if already_present {
        return;
    }

    working_history.push(uni::Message::system(content.to_string()));
}

pub(super) fn ensure_post_tool_resume_directive(working_history: &mut Vec<uni::Message>) {
    ensure_recent_system_message(working_history, POST_TOOL_RESUME_DIRECTIVE);
}

pub(crate) fn prepare_post_tool_tool_free_recovery(
    working_history: &mut Vec<uni::Message>,
    reason: &str,
) {
    ensure_post_tool_resume_directive(working_history);
    ensure_recent_system_message(working_history, reason);
}

pub(super) fn maybe_recover_after_post_tool_llm_failure(
    renderer: &mut AnsiRenderer,
    working_history: &mut Vec<uni::Message>,
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    failure_stage: &'static str,
    allow_tool_free_retry: bool,
) -> Result<PostToolFailureRecovery> {
    let has_partial_tool_progress =
        has_tool_response_since(working_history, turn_history_start_len);
    if !has_partial_tool_progress {
        return Ok(PostToolFailureRecovery::NotApplicable);
    }

    let err_cat = vtcode_commons::classify_anyhow_error(err);
    let transient_hint = if err_cat.is_retryable() {
        " (transient — may resolve on retry)"
    } else {
        ""
    };
    let summary = format!(
        "Tool execution completed, but the model follow-up failed{}. Output above is valid.",
        transient_hint,
    );
    renderer.line(MessageStyle::Info, &summary)?;
    renderer.line(
        MessageStyle::Info,
        &format!("Follow-up error category: {}", err_cat.user_label()),
    )?;
    if !err_cat.is_retryable() {
        renderer.line(
            MessageStyle::Info,
            "Tip: rerun with a narrower prompt or switch provider/model for the follow-up.",
        )?;
    }
    ensure_post_tool_resume_directive(working_history);

    let action = if err_cat.is_retryable() && allow_tool_free_retry {
        prepare_post_tool_tool_free_recovery(working_history, POST_TOOL_RECOVERY_REASON);
        renderer.line(
            MessageStyle::Info,
            "[!] Follow-up failed after tool execution; scheduling a final tool-free recovery pass.",
        )?;
        PostToolFailureRecovery::RetryToolFree
    } else {
        PostToolFailureRecovery::StopAfterDirective
    };

    tracing::warn!(
        error = %err,
        step = step_count,
        stage = failure_stage,
        category = ?err_cat,
        retryable = err_cat.is_retryable(),
        recovery_action = ?action,
        "Recovered turn after post-tool LLM phase failure"
    );
    Ok(action)
}

pub(super) fn complete_turn_after_failed_tool_free_recovery(
    working_history: &mut Vec<uni::Message>,
    failure_stage: &'static str,
    err: Option<&anyhow::Error>,
) -> TurnLoopResult {
    let has_recent_fallback = working_history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    });
    if !has_recent_fallback {
        working_history.push(
            uni::Message::assistant(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER.to_string())
                .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        );
    }

    if let Some(err) = err {
        tracing::warn!(
            stage = failure_stage,
            error = %err,
            "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
        );
    } else {
        tracing::warn!(
            stage = failure_stage,
            "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
        );
    }

    TurnLoopResult::Completed
}

pub(super) fn normalize_tool_free_recovery_break_outcome(
    working_history: &mut Vec<uni::Message>,
    outcome_result: TurnLoopResult,
    tool_free_recovery: bool,
) -> TurnLoopResult {
    let should_fallback = tool_free_recovery
        && matches!(
            outcome_result,
            TurnLoopResult::Blocked {
                reason: Some(ref reason)
            } if reason.contains("tool-free synthesis pass")
        );

    if should_fallback {
        return complete_turn_after_failed_tool_free_recovery(
            working_history,
            "handle_turn_processing_result.tool_free_recovery_contract_violation",
            None,
        );
    }

    outcome_result
}