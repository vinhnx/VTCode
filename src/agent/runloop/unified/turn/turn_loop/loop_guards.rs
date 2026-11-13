//! Loop guards: bounded checks that continue, break, or let the turn proceed.

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::path::PathBuf;

use anyhow::Result;
use vtcode_core::llm::provider as uni;

use super::responses::{
    APPROVED_PLAN_STALE_PAUSE_RECOVERY_DIRECTIVE, MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES,
    STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
use crate::agent::runloop::unified::turn::turn_loop_helpers::is_stale_approved_plan_pause_response;
use crate::agent::runloop::unified::turn::turn_processing::{
    PendingVerificationTextOutcome, execute_harness_auto_verification,
};
use vtcode_core::utils::ansi::MessageStyle;

/// Handle a stale approved-plan pause echo: discard it and either retry
/// with a bounded recovery directive or end Blocked. Returns `None` when
/// no stale pause is present and the caller should proceed.
pub(super) async fn handle_stale_approved_plan_pause(
    turn_processing_ctx: &mut TurnProcessingContext<'_>,
    repeated_tool_attempts: &mut LoopTracker,
    turn_modified_files: &mut BTreeSet<PathBuf>,
    processing_result: &TurnProcessingResult,
    step_count: usize,
    current_max_tool_loops: usize,
    tool_repeat_limit: usize,
) -> Result<Option<ControlFlow<TurnLoopResult>>> {
    // A recovery directive from the planning turn can leak into the first
    // approved-plan build request. If the model echoes that stale state as
    // its answer, do not commit it to the transcript or end execution;
    // give the fresh write-capable turn a bounded, explicit retry instead.
    let stale_approved_plan_pause = matches!(
        &processing_result,
        TurnProcessingResult::TextResponse { text, .. }
            if turn_processing_ctx.is_approved_plan_execution()
                && is_stale_approved_plan_pause_response(text)
    );
    if stale_approved_plan_pause
        && turn_processing_ctx.tool_registry.completion_verification_pending()
        && !turn_processing_ctx.in_tool_free_recovery_synthesis()
    {
        // The stale text was discarded (never committed), so no content
        // is available for completion-claim detection; pass empty text.
        match turn_processing_ctx.handle_pending_verification_text_response(
            &mut *repeated_tool_attempts,
            "",
            turn_modified_files,
        )? {
            PendingVerificationTextOutcome::Block { reason } => {
                return Ok(Some(ControlFlow::Break(TurnLoopResult::Blocked { reason: Some(reason) })));
            }
            // The stale pause was discarded, so the normal text path never
            // sees it: execute the verifier here rather than looping on
            // another pause echo.
            PendingVerificationTextOutcome::AutoVerify { command } => {
                match execute_harness_auto_verification(
                    &mut *turn_processing_ctx,
                    &mut *repeated_tool_attempts,
                    &mut *turn_modified_files,
                    step_count,
                    current_max_tool_loops,
                    tool_repeat_limit,
                    command,
                )
                .await?
                {
                    TurnHandlerOutcome::Continue => return Ok(Some(ControlFlow::Continue(()))),
                    TurnHandlerOutcome::Break(outcome_result) => {
                        return Ok(Some(ControlFlow::Break(outcome_result)));
                    }
                    // Agent-switch/policy outcomes have no producer for a lone
                    // verifier execution today (`pending_primary_agent` is
                    // never set by exec paths); keep the turn alive on
                    // them, loudly, so a future producer surfaces in logs
                    // instead of vanishing.
                    TurnHandlerOutcome::SwitchPrimaryAgent(_)
                    | TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { .. }
                    | TurnHandlerOutcome::BreakWithPolicy { .. } => {
                        tracing::warn!(
                            "harness auto-verification produced an unexpected agent-switch/policy outcome; keeping the turn alive"
                        );
                        return Ok(Some(ControlFlow::Continue(())));
                    }
                }
            }
        }
    }
    if stale_approved_plan_pause {
        // The stale pause text is discarded (never committed to history),
        // so it must not consume the generic text-response budget; that
        // budget is recorded exactly once when the replacement response
        // is processed. Retries stay bounded by
        // `MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES`, and the generic cap at
        // the top of the loop remains responsible for runaway prose.
        // Once the retries are exhausted the turn ends Blocked without
        // storing the stale text, so it can never become a successful
        // final answer.
        if turn_processing_ctx.harness_state.approved_plan_recovery_retries() < MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES {
            turn_processing_ctx.harness_state.record_approved_plan_recovery_retry();
            turn_processing_ctx
                .working_history
                .push(uni::Message::system(APPROVED_PLAN_STALE_PAUSE_RECOVERY_DIRECTIVE.to_string()));
            let _ = turn_processing_ctx
                .renderer
                .line(MessageStyle::Info, "Approved-plan execution resumed after clearing stale recovery state.");
            return Ok(Some(ControlFlow::Continue(())));
        }
        return Ok(Some(ControlFlow::Break(TurnLoopResult::Blocked {
            reason: Some(STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON.to_string()),
        })));
    }

    Ok(None)
}
