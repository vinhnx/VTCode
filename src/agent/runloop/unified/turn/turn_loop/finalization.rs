//! Turn outcome events and bounded exec cleanup, in lifecycle order.

use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use tokio::time::timeout;
use vtcode_core::core::agent::events::{tool_invocation_completed_event, tool_output_completed_event};
use vtcode_core::exec::events::{ToolCallStatus, ToolOutcome, Usage as HarnessUsage};
use vtcode_core::llm::provider as uni;

use super::notifications::emit_turn_outcome_notification;
use super::usage_accounting::has_turn_usage;
use super::{
    ASSISTANT_TEXT_RESPONSE_CAP_REASON, TurnLoopContext, UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT, effective_vt_cfg,
};
use crate::agent::runloop::unified::inline_events::harness::{
    harness_event, turn_blocked_event, turn_completed_event, turn_completed_event_with_sessions, turn_failed_event,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;

/// Outer budget for terminating exec sessions during turn-level Exit/Cancelled
/// teardown. With `Immediate` PTY mode a healthy close is milliseconds; the cap
/// only fires on a wedged backend close. On timeout the session-end backstop
/// re-runs (2 s cap) and the OS reaps any remainder at process exit — never
/// park the fullscreen TUI waiting for a child.
const TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// Finalize the turn: terminate sessions if needed, emit outcome events,
/// and send notifications.
pub(super) async fn finalize_turn(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &[uni::Message],
    result: &TurnLoopResult,
    turn_usage: &HarnessUsage,
) {
    use vtcode_commons::program_status::{ProgramState, ProgramStatusUpdate};
    let state = match result {
        TurnLoopResult::Completed { .. } => ProgramState::Done,
        TurnLoopResult::Aborted => ProgramState::Error,
        TurnLoopResult::Blocked { .. } => ProgramState::Blocked,
        TurnLoopResult::Cancelled | TurnLoopResult::Exit => ProgramState::Idle,
    };
    ctx.handle.program_status(ProgramStatusUpdate::Outcome(state));
    if let TurnLoopResult::Blocked { reason } = result {
        let reason_text = reason.as_deref().unwrap_or("turn blocked");
        let outcome = if reason_text.contains(ASSISTANT_TEXT_RESPONSE_CAP_REASON) {
            "safety_cap_handoff"
        } else {
            "blocked_handoff"
        };
        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "turn_handoff",
            outcome,
            reason = reason_text,
            run_id = %ctx.harness_state.run_id.0,
            turn_id = %ctx.harness_state.turn_id.0,
            "turn handoff metric"
        );
    }
    if matches!(result, TurnLoopResult::Cancelled) {
        terminate_exec_sessions_bounded(
            ctx.tool_registry.terminate_active_exec_sessions_for_exit_async(),
            "active exec sessions after turn cancellation",
        )
        .await;
    } else if matches!(result, TurnLoopResult::Exit) {
        terminate_exec_sessions_bounded(
            ctx.tool_registry.terminate_all_exec_sessions_for_exit_async(),
            "all exec sessions after turn exit",
        )
        .await;
    }
    if let Some(emitter) = ctx.harness_emitter {
        // Exit is a graceful user-initiated action, not a failure
        let event = match result {
            TurnLoopResult::Exit => turn_completed_event(turn_usage.clone()),
            TurnLoopResult::Completed { .. } => {
                // Capture live exec sessions only for the completed path so
                // `turn.completed` carries the same bounded id set as
                // `SnapshotTurnDiagnostics` for cross-turn resume correlation.
                // Backend-checked, newest first, capped at 4 by the helper.
                // Stops skip this optional lookup so reader contention cannot
                // delay accepted exit.
                let in_progress_ids: Vec<String> = crate::agent::runloop::unified::stop_requests::await_with_stop(
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    ctx.tool_registry
                        .in_progress_exec_sessions(vtcode_core::exec::events::MAX_IN_PROGRESS_EXEC_SESSIONS),
                )
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.id.as_str().to_string())
                .collect();
                if in_progress_ids.is_empty() {
                    turn_completed_event(turn_usage.clone())
                } else {
                    turn_completed_event_with_sessions(turn_usage.clone(), in_progress_ids)
                }
            }
            TurnLoopResult::Aborted => {
                turn_failed_event("turn aborted", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
            TurnLoopResult::Cancelled => {
                turn_failed_event("turn cancelled", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
            TurnLoopResult::Blocked { .. } => {
                turn_failed_event("turn blocked", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
        };
        if let Err(e) = emitter.emit(event) {
            tracing::debug!(error = %e, "harness turn outcome event emission failed");
        }
        if let TurnLoopResult::Blocked { reason } = result {
            let message = reason.clone().unwrap_or_else(|| "turn blocked".to_string());
            // Blocked-call fuse telemetry (last tool, enforced caps) captured
            // at trip time; blocks that did not trip the fuse keep the
            // previous `None`/zero fields.
            let fuse_telemetry = ctx.harness_state.take_blocked_tool_recovery_telemetry();
            let (last_tool, consecutive_cap, total_cap, blocked_streak, blocked_total) = fuse_telemetry
                .map(|telemetry| {
                    (
                        Some(telemetry.last_tool),
                        telemetry.consecutive_cap,
                        telemetry.total_cap,
                        telemetry.blocked_streak,
                        telemetry.blocked_total,
                    )
                })
                .unwrap_or((
                    None,
                    0,
                    0,
                    ctx.harness_state.consecutive_blocked_tool_calls,
                    ctx.harness_state.blocked_tool_calls,
                ));
            let blocked_event = vtcode_core::exec::events::TurnBlockedEvent {
                completed_at: None,
                message: message.clone(),
                last_tool,
                blocked_streak,
                blocked_total,
                consecutive_cap,
                total_cap,
                recovery_active: ctx.harness_state.is_recovery_active() || ctx.harness_state.recovery_pass_used(),
                usage: has_turn_usage(turn_usage).then(|| turn_usage.clone()),
            };
            if let Err(e) = emitter.emit(turn_blocked_event(blocked_event)) {
                tracing::debug!(error = %e, "harness turn.blocked event emission failed");
            }
            if let Err(e) = emitter.emit(harness_event(
                vtcode_core::exec::events::HarnessEventKind::TurnBlocked,
                Some(message),
                None,
                None,
                None,
            )) {
                tracing::debug!(error = %e, "harness TurnBlocked event emission failed");
            }
        }

        // Close streamed tool-call items whose calls never reached the
        // pipeline. Entries only survive here when the call was rejected
        // pre-flight, dropped from a batch, or interrupted by the turn ending;
        // dispatched calls remove themselves on execution or rejection.
        for (tool_call_id, streamed) in ctx.harness_state.take_all_streamed_tool_call_item_ids() {
            let failed = ToolCallStatus::Failed;
            let raw_id = (!tool_call_id.trim().is_empty()).then_some(tool_call_id.as_str());
            let _ = emitter.emit(tool_invocation_completed_event(
                streamed.item_id.clone(),
                &streamed.tool_name,
                None,
                raw_id,
                failed.clone(),
                ToolOutcome::Cancelled,
            ));
            let _ = emitter.emit(tool_output_completed_event(
                streamed.item_id,
                raw_id,
                failed,
                None,
                None,
                UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT,
            ));
        }
    }
    let _ = crate::agent::runloop::unified::stop_requests::await_with_stop(
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        emit_turn_outcome_notification(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            working_history,
            ctx.config.workspace.as_path(),
            ctx.harness_state,
            result,
        ),
    )
    .await;
}

/// Terminate exec sessions during turn-level Exit/Cancelled teardown, bounded
/// by [`TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT`]. A timeout only skips waiting: the
/// session-end backstop re-runs the sweep and the OS reaps any remainder at
/// process exit.
async fn terminate_exec_sessions_bounded(termination: impl Future<Output = Result<()>>, phase: &str) {
    match timeout(TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT, termination).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::warn!(error = %err, "Failed to terminate {phase}"),
        Err(_elapsed) => tracing::warn!("timed out terminating {phase}; continuing teardown"),
    }
}
