//! Turn preparation gate for the session interaction loop.
//!
//! Validates the next turn input, splits runtime borrows, captures the
//! refused-turn rollback, prepares the prompt checkpoint lease, and injects
//! transient system notes before request execution.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::{AgentRuntime, RuntimeSteering};
use vtcode_core::core::agent::snapshots::PromptCheckpointLease;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider::{Message, MessageRole};
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionMessage;
use vtcode_ui::tui::app::{InlineHandle, ProgressGuard};

use super::super::super::support::{
    RefusedTurnRollback, ack_peer_messages, build_unrelated_dirty_worktree_note, checkpoint_unavailable_notice,
    prepare_transient_turn_notes, skipped_paths_turn_warning,
};
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, harness_event, harness_session_message_event,
};
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::background_completion::PendingBackgroundCompletions;
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;

/// Mutable turn-preparation state threaded through the prep gate.
pub(super) struct TurnPrep<'a> {
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub renderer: &'a mut AnsiRenderer,
    pub last_turn_result: &'a mut Option<RunLoopTurnLoopResult>,
    pub session_end_reason: &'a mut SessionEndReason,
    pub next_checkpoint_turn: &'a mut usize,
    pub pending_background_completions: &'a mut PendingBackgroundCompletions,
    pub pending_matrix_note: &'a mut Option<String>,
    pub tool_registry: &'a ToolRegistry,
    pub config: &'a CoreAgentConfig,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub checkpoint_manager: &'a Option<vtcode_core::core::agent::snapshots::SnapshotManager>,
    pub agent_touched_paths: &'a BTreeSet<PathBuf>,
    pub turn_progress: &'a ProgressGuard,
    pub next_turn_input: &'a str,
    pub thread_id: &'a str,
    pub turn_id: &'a str,
    pub executing_approved_plan: bool,
    pub active_task_follow_up: bool,
    pub completed_turn_prompt_message_index: Option<usize>,
    pub preparation_started_at: Instant,
}

/// How the interaction loop proceeds after turn preparation.
#[allow(
    clippy::large_enum_variant,
    reason = "Matched immediately at the call site; never stored."
)]
pub(super) enum TurnPrepFlow<'a> {
    ContinueLoop,
    BreakLoop,
    Proceed {
        working_history: &'a mut Vec<Message>,
        runtime_steering: &'a mut RuntimeSteering,
        refused_turn_rollback: RefusedTurnRollback,
        transient_system_notes: Vec<String>,
        prompt_checkpoint_lease: Option<PromptCheckpointLease>,
    },
}

/// Prepare the next turn: validate input, split borrows, checkpoint, notes.
pub(super) async fn run_turn_prep<'a>(runtime: &'a mut AgentRuntime, gate: TurnPrep<'_>) -> Result<TurnPrepFlow<'a>> {
    let TurnPrep {
        plan_session,
        renderer,
        last_turn_result,
        session_end_reason,
        next_checkpoint_turn,
        pending_background_completions,
        pending_matrix_note,
        tool_registry,
        config,
        handle,
        ctrl_c_state,
        ctrl_c_notify,
        harness_emitter,
        checkpoint_manager,
        agent_touched_paths,
        turn_progress,
        next_turn_input,
        thread_id,
        turn_id,
        executing_approved_plan,
        active_task_follow_up,
        completed_turn_prompt_message_index,
        preparation_started_at,
    } = gate;
    if next_turn_input.trim().is_empty() {
        return Ok(TurnPrepFlow::ContinueLoop);
    }
    if !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input) {
        tool_registry.begin_tracker_request(
            executing_approved_plan
                || crate::agent::runloop::unified::turn::context::tracker_request_explicitly_adopts(next_turn_input),
        );
    }
    if let Some(emitter) = harness_emitter.as_ref() {
        use vtcode_core::exec::events::InputOrigin;
        let origin = if executing_approved_plan {
            InputOrigin::PlanApproval
        } else if crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input) {
            InputOrigin::Continuation
        } else if active_task_follow_up {
            InputOrigin::Correction
        } else {
            InputOrigin::User
        };
        let task_id = emitter.begin_task_turn(turn_id, next_turn_input, origin)?;
        if let Some(validator) = emitter.decision_validator() {
            tool_registry.set_decision_evidence_validator(validator);
        }
        tool_registry.set_harness_task(Some(task_id));
    }
    let user_revision = !executing_approved_plan
        && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input)
        && (!active_task_follow_up
            || runtime.state.messages.last().is_some_and(|message| {
                message.metadata.as_ref().and_then(|metadata| metadata.source()) == Some("user_input")
            }));
    if user_revision
        && tool_registry.is_planning_active()
        && crate::agent::runloop::unified::planning_workflow::recovery_state::begin_user_revision(
            &mut plan_session.recovery,
        )
        && let Some(emitter) = harness_emitter.as_ref()
    {
        emitter
            .persist_checkpoint(&crate::agent::runloop::unified::planning_workflow::recovery_state::checkpoint_event(
                &plan_session.recovery,
                thread_id,
                turn_id,
            ))
            .await?;
    }
    // An explicit user turn after a terminal plan rejection rearms the
    // validation-repair budget. Planning mode parks recovery in
    // AwaitingInput/AwaitingApproval; execution mode (no recovery cycle)
    // sets a one-shot pending flag. Internal follow-ups must not consume
    // the rearm, so the pending flag is taken last for short-circuiting.
    let planning_retry_ready = tool_registry.is_planning_active()
        && matches!(
            plan_session.recovery.outcome,
            vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput
                | vtcode_core::exec::events::planning::PlanningOutcome::AwaitingApproval
        );
    if !active_task_follow_up
        && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input)
        && (planning_retry_ready || plan_session.take_execution_plan_rejection_pending())
    {
        plan_session.begin_explicit_recovery_cycle_for_user_input();
        if tool_registry.is_planning_active() {
            crate::agent::runloop::unified::planning_workflow::recovery_state::record_clarification(
                &mut plan_session.recovery,
                next_turn_input,
                false,
            );
        }
    }
    let (session_state, runtime_steering) = runtime.split_mut();
    let working_history = Arc::make_mut(&mut session_state.messages);
    let refused_turn_rollback =
        RefusedTurnRollback::capture(working_history, completed_turn_prompt_message_index, next_turn_input);
    macro_rules! preparation_wait {
        ($future:expr) => {{
            match crate::agent::runloop::unified::stop_requests::await_with_stop(ctrl_c_state, ctrl_c_notify, $future)
                .await
            {
                Some(value) => value,
                None => {
                    if let Some(emitter) = harness_emitter.as_ref() {
                        let _ =
                            emitter.emit(crate::agent::runloop::unified::inline_events::harness::turn_failed_event(
                                "turn cancelled during preparation",
                                None,
                            ));
                    }
                    // Preparation has not admitted tools. Keep the user's
                    // request as a draft, and require a new submission.
                    handle.set_placeholder(Some(
                        vtcode_config::constants::ui::CHAT_INPUT_PLACEHOLDER_INTERRUPTED.to_owned(),
                    ));
                    handle.set_activity_state(ActivityState::Idle);
                    *last_turn_result = Some(RunLoopTurnLoopResult::Cancelled);
                    if ctrl_c_state.is_exit_requested() {
                        *session_end_reason = SessionEndReason::Exit;
                        return Ok(TurnPrepFlow::BreakLoop);
                    }
                    ctrl_c_state.mark_cancel_handled();
                    *session_end_reason = SessionEndReason::Cancelled;
                    return Ok(TurnPrepFlow::ContinueLoop);
                }
            }
        }};
    }
    let workspace_buf = config.workspace.clone();
    let touched_clone = agent_touched_paths.clone();
    let dirty_worktree_task =
        tokio::task::spawn_blocking(move || build_unrelated_dirty_worktree_note(&workspace_buf, &touched_clone));
    let checkpoint_started_at = Instant::now();
    let prompt_checkpoint_lease = if let Some(manager) = checkpoint_manager.as_ref() {
        handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::SavingCheckpoint);
        let prefix = completed_turn_prompt_message_index
            .unwrap_or(working_history.len())
            .min(working_history.len());
        let conversation: Vec<_> = working_history[..prefix].iter().map(SessionMessage::from).collect();
        let session_id = tool_registry.harness_context_snapshot().session_id;
        // A missing lease (cooperative cancellation) shares the
        // unavailable-checkpoint recovery below: retain the prompt
        // and retry instead of panicking on a supposedly
        // impossible arm.
        let lease = match preparation_wait!(manager.begin_prompt_with_cancellation(
            *next_checkpoint_turn,
            &session_id,
            next_turn_input,
            &conversation,
            CancellationToken::new()
        ))
        .and_then(|lease| lease.context("checkpoint preparation cancelled"))
        {
            Ok(lease) => lease,
            Err(err) => {
                // Drain the independent worker before retrying this prompt.
                let _ = preparation_wait!(dirty_worktree_task);
                tracing::warn!(error = %err, "Checkpoint unavailable; prompt retained in input");
                let message = checkpoint_unavailable_notice(&format!("{err:#}"));
                let _ = renderer.line(MessageStyle::Info, &message);
                // The prompt message was already appended to history by the
                // interaction loop (or the approved-plan handoff). Remove it
                // so a retry does not duplicate, and restore the text.
                if let Some(index) = completed_turn_prompt_message_index {
                    if index < working_history.len() {
                        working_history.truncate(index);
                    }
                    handle.set_input(next_turn_input.to_owned());
                    handle.force_redraw();
                } else if working_history.last().is_some_and(|message| {
                    message.role == MessageRole::User && message.content.as_text().trim() == next_turn_input.trim()
                }) {
                    working_history.pop();
                    handle.set_input(next_turn_input.to_owned());
                    handle.force_redraw();
                }
                return Ok(TurnPrepFlow::ContinueLoop);
            }
        };
        if let Some(emitter) = harness_emitter.as_ref() {
            let detail = match lease.skipped_paths().filter(|skipped| !skipped.is_empty()) {
                Some(skipped) => {
                    format!("Before prompt {next_checkpoint_turn} snapshot saved ({} skipped)", skipped.total)
                }
                None => format!("Before prompt {next_checkpoint_turn} snapshot saved"),
            };
            let _ = emitter.emit(harness_event(
                vtcode_core::exec::events::HarnessEventKind::SnapshotCreated,
                Some(detail),
                None,
                None,
                None,
            ));
        }
        if let Some(skipped) = lease.skipped_paths().filter(|skipped| !skipped.is_empty()) {
            tracing::warn!(skipped = %skipped.summary(), "Checkpoint skipped paths; turn proceeds degraded");
            let _ = renderer.line(MessageStyle::Warning, &skipped_paths_turn_warning(skipped));
        }
        *next_checkpoint_turn = next_checkpoint_turn.saturating_add(1);
        Some(lease)
    } else {
        None
    };
    tracing::debug!(target: "vtcode.response_latency", operation_id = turn_progress.operation().id(),
                checkpoint_ms = checkpoint_started_at.elapsed().as_secs_f64() * 1000.0, "checkpoint preparation complete");
    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::PreparingContext);
    let unrelated_dirty_note = match preparation_wait!(dirty_worktree_task) {
        Ok(Ok(Some(note))) => Some(note),
        Ok(Err(err)) => {
            tracing::warn!(
                error = %err,
                "Failed to inspect unrelated dirty worktree entries before turn"
            );
            None
        }
        Err(error) => {
            tracing::warn!(%error, "Dirty worktree inspection worker failed");
            None
        }
        Ok(Ok(None)) => None,
    };
    let (transient_system_notes, peer_delivery) = preparation_wait!(prepare_transient_turn_notes(
        config.workspace.as_path(),
        tool_registry,
        unrelated_dirty_note,
        match (pending_background_completions.take_transient_note(), pending_matrix_note.take()) {
            (Some(background), Some(matrix)) => Some(format!("{background}\n{matrix}")),
            (background, matrix) => background.or(matrix),
        },
    ));
    working_history.extend(transient_system_notes.iter().cloned().map(Message::system));
    // Ack only after admission: a stop accepted between preparation and this
    // extend leaves the mail queued for redelivery instead of losing it.
    let received = ack_peer_messages(
        config.workspace.as_path(),
        &tool_registry.harness_context_snapshot().session_id,
        &peer_delivery.pending,
    );
    if let Some(emitter) = harness_emitter.as_ref() {
        use vtcode_core::exec::events::HarnessEventKind;
        for receipt in &received {
            let _ = emitter.emit(harness_session_message_event(
                HarnessEventKind::SessionMessageReceived,
                Some(format!("{} from session {}", receipt.kind_label, receipt.peer_session)),
                Some(receipt.peer_session.clone()),
                Some(receipt.message_id.clone()),
            ));
        }
        for expired_id in &peer_delivery.expired_ids {
            let _ = emitter.emit(harness_session_message_event(
                HarnessEventKind::SessionMessageExpired,
                Some("peer message expired before delivery".to_string()),
                None,
                Some(expired_id.clone()),
            ));
        }
    }
    tracing::debug!(target: "vtcode.response_latency", operation_id = turn_progress.operation().id(),
                preparation_ms = preparation_started_at.elapsed().as_secs_f64() * 1000.0,
                accepted_to_prepared_ms = turn_progress.operation().started_at().elapsed().as_secs_f64() * 1000.0,
                "turn preparation complete");
    Ok(TurnPrepFlow::Proceed {
        working_history,
        runtime_steering,
        refused_turn_rollback,
        transient_system_notes,
        prompt_checkpoint_lease,
    })
}
