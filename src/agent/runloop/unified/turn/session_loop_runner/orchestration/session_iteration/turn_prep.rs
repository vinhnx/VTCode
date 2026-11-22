//! Turn preparation gate for the session interaction loop.
//!
//! Validates the next turn input, splits runtime borrows, captures the
//! refused-turn rollback, prepares the prompt checkpoint lease, and injects
//! transient system notes before request execution.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::{AgentRuntime, RuntimeSteering};
use vtcode_core::core::agent::snapshots::{
    PromptCheckpointLease, checkpoint_failure_is_recoverable, classify_begin_prompt_error,
};
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider::Message;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::session_archive::SessionMessage;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineHandle, ProgressGuard};

use super::super::super::support::{
    RefusedTurnRollback, ack_peer_messages, build_unrelated_dirty_worktree_note, checkpoint_unavailable_notice,
    degraded_checkpoint_warning, prepare_transient_turn_notes, restore_kept_prompt, skipped_paths_turn_warning,
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
    pub pending_monitor_note: &'a mut Option<String>,
    pub tool_registry: &'a ToolRegistry,
    pub goal_tracker: &'a crate::agent::runloop::unified::turn::session::slash_commands::SharedGoalTracker,
    pub required_goal_activation: Option<uuid::Uuid>,
    pub automatic_goal_limit: Option<u8>,
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

fn admit_prepared_goal(tracker: &mut vtcode_memory::GoalTracker, expected: uuid::Uuid, limit: Option<u8>) -> bool {
    tracker.active_activation_id() == Some(expected)
        && limit.is_none_or(|limit| tracker.claim_auto_continue_turn(limit).is_some())
}

fn emit_prep_failure(emitter: Option<&HarnessEventEmitter>, reports_terminal: bool, reason: &str) -> Result<()> {
    if reports_terminal && let Some(emitter) = emitter {
        emitter.emit(crate::agent::runloop::unified::inline_events::harness::turn_failed_event(reason, None))?;
    }
    Ok(())
}

/// Prepare the next turn: validate input, split borrows, checkpoint, notes.
#[expect(clippy::cognitive_complexity, reason = "turn preparation; split per stage")]
pub(super) async fn run_turn_prep<'a>(runtime: &'a mut AgentRuntime, gate: TurnPrep<'_>) -> Result<TurnPrepFlow<'a>> {
    let TurnPrep {
        plan_session,
        renderer,
        last_turn_result,
        session_end_reason,
        next_checkpoint_turn,
        pending_background_completions,
        pending_matrix_note,
        pending_monitor_note,
        tool_registry,
        goal_tracker,
        required_goal_activation,
        automatic_goal_limit,
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
    // A new turn owns a fresh admission window. Abandon receipts left over
    // from the previous turn so their inputs fall through to the ordinary
    // queue exactly once instead of being stranded on a dead channel.
    runtime.drain_rejected_steers();
    runtime.open_admission();
    ctrl_c_state.reject_pending_steers();
    if next_turn_input.trim().is_empty() {
        return Ok(TurnPrepFlow::ContinueLoop);
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
    let (session_state, runtime_steering) = runtime.split_mut();
    let working_history = Arc::make_mut(&mut session_state.messages);
    let refused_turn_rollback =
        RefusedTurnRollback::capture(working_history, completed_turn_prompt_message_index, next_turn_input);
    let reports_terminal = required_goal_activation.is_none() || !active_task_follow_up;
    let reject_superseded_goal = |history: &mut Vec<Message>| -> Result<()> {
        refused_turn_rollback.apply(history);
        emit_prep_failure(harness_emitter.as_ref(), reports_terminal, "goal changed before turn admission")?;
        handle.set_activity_state(ActivityState::Idle);
        Ok(())
    };
    if let Some(expected) = required_goal_activation
        && goal_tracker.lock().await.active_activation_id() != Some(expected)
    {
        reject_superseded_goal(working_history)?;
        return Ok(TurnPrepFlow::ContinueLoop);
    }
    macro_rules! preparation_wait {
        ($future:expr) => {{
            match crate::agent::runloop::unified::stop_requests::await_with_stop(ctrl_c_state, ctrl_c_notify, $future)
                .await
            {
                Some(value) => value,
                None => {
                    if !reports_terminal {
                        refused_turn_rollback.apply(working_history);
                    }
                    emit_prep_failure(harness_emitter.as_ref(), reports_terminal, "turn cancelled during preparation")?;
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
    // Disabled checkpoints skip the lease entirely: no lock, no capture, and
    // the turn proceeds exactly like a checkpoint-less session.
    let prompt_checkpoint_lease = if let Some(manager) = checkpoint_manager.as_ref().filter(|manager| manager.enabled())
    {
        handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::SavingCheckpoint);
        let prefix = completed_turn_prompt_message_index
            .unwrap_or(working_history.len())
            .min(working_history.len());
        let conversation: Vec<_> = working_history[..prefix].iter().map(SessionMessage::from).collect();
        let session_id = tool_registry.harness_context_snapshot().session_id;
        let lease_result = preparation_wait!(manager.begin_prompt_with_cancellation(
            *next_checkpoint_turn,
            &session_id,
            next_turn_input,
            &conversation,
            CancellationToken::new()
        ));
        match lease_result {
            Ok(Some(lease)) => {
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
            }
            // Cooperative cancellation: retain the prompt and retry rather
            // than dropping the user's request.
            Ok(None) => {
                // Drain the independent worker before retrying this prompt.
                let _ = preparation_wait!(dirty_worktree_task);
                tracing::debug!("Checkpoint preparation cancelled; prompt retained in input");
                let _ = renderer.line(MessageStyle::Info, "Prompt kept — checkpoint cancelled. Retry when ready.");
                if reports_terminal {
                    restore_kept_prompt(working_history, handle, completed_turn_prompt_message_index, next_turn_input);
                } else {
                    refused_turn_rollback.apply(working_history);
                    handle.set_activity_state(ActivityState::Idle);
                }
                if required_goal_activation.is_some() {
                    emit_prep_failure(
                        harness_emitter.as_ref(),
                        reports_terminal,
                        "goal turn checkpoint preparation did not complete",
                    )?;
                }
                return Ok(TurnPrepFlow::ContinueLoop);
            }
            // Deterministic failures (storage, capture, worker) carry no
            // concurrent writer, so the turn proceeds without rewind for it
            // instead of wedging the session. The counter stays: no record
            // was written, so the turn-end snapshot keeps this turn's number.
            Err(err) if checkpoint_failure_is_recoverable(classify_begin_prompt_error(&err)) => {
                tracing::warn!(error = %err, "Checkpoint unavailable; proceeding without rewind for this turn");
                let _ = renderer.line(MessageStyle::Warning, &degraded_checkpoint_warning(&err));
                None
            }
            Err(err) => {
                // Drain the independent worker before retrying this prompt.
                let _ = preparation_wait!(dirty_worktree_task);
                tracing::warn!(error = %err, "Checkpoint unavailable; prompt retained in input");
                let message = checkpoint_unavailable_notice(&err);
                let _ = renderer.line(MessageStyle::Info, &message);
                if reports_terminal {
                    restore_kept_prompt(working_history, handle, completed_turn_prompt_message_index, next_turn_input);
                } else {
                    refused_turn_rollback.apply(working_history);
                    handle.set_activity_state(ActivityState::Idle);
                }
                if required_goal_activation.is_some() {
                    emit_prep_failure(
                        harness_emitter.as_ref(),
                        reports_terminal,
                        "goal turn checkpoint preparation did not complete",
                    )?;
                }
                return Ok(TurnPrepFlow::ContinueLoop);
            }
        }
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
    let (mut transient_system_notes, peer_delivery) = preparation_wait!(prepare_transient_turn_notes(
        config.workspace.as_path(),
        tool_registry,
        unrelated_dirty_note,
    ));
    if let Some(expected) = required_goal_activation
        && !admit_prepared_goal(&mut *goal_tracker.lock().await, expected, automatic_goal_limit)
    {
        reject_superseded_goal(working_history)?;
        return Ok(TurnPrepFlow::ContinueLoop);
    }
    // Apply request-scoped state only after the prepared input is admitted.
    if !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input) {
        tool_registry.begin_tracker_request(
            executing_approved_plan
                || crate::agent::runloop::unified::turn::context::tracker_request_explicitly_adopts(next_turn_input),
        );
    }
    let user_revision = !executing_approved_plan
        && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input)
        && (!active_task_follow_up
            || working_history.last().is_some_and(|message| {
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
    let wake_notes = [
        pending_background_completions.take_transient_note(),
        pending_matrix_note.take(),
        pending_monitor_note.take(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if !wake_notes.is_empty() {
        let before_peer_note = transient_system_notes
            .len()
            .saturating_sub(usize::from(peer_delivery.note.is_some()));
        transient_system_notes.insert(before_peer_note, wake_notes.join("\n"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_goal_admission_charges_only_a_current_automatic_turn() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = vtcode_memory::GoalTracker::new(dir.path().to_path_buf());
        tracker.create_goal("goal-id".into(), "Ship it".into(), None, 0, "2026-01-01T00:00:00Z".into(), None);
        let activation = tracker.active_activation_id().unwrap();
        assert!(admit_prepared_goal(&mut tracker, activation, None));
        assert_eq!(tracker.auto_continue_turns(), 0);
        assert!(tracker.pause(vtcode_memory::GoalPauseReason::User));
        assert!(tracker.resume());
        assert!(!admit_prepared_goal(&mut tracker, activation, Some(1)));
        let resumed = tracker.active_activation_id().unwrap();
        assert!(admit_prepared_goal(&mut tracker, resumed, Some(1)));
        assert!(!admit_prepared_goal(&mut tracker, resumed, Some(1)));
        assert_eq!(tracker.auto_continue_turns(), 1);
    }

    #[test]
    fn goal_prep_failure_only_publishes_for_an_owned_turn() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events.jsonl");
        let emitter = HarnessEventEmitter::new(events.clone()).unwrap();
        emit_prep_failure(Some(&emitter), false, "superseded synthetic").unwrap();
        emit_prep_failure(Some(&emitter), true, "superseded explicit prompt").unwrap();
        let log = std::fs::read_to_string(events).unwrap();
        assert!(log.contains("superseded explicit prompt"));
        assert!(!log.contains("superseded synthetic"));
    }
}
