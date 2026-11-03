//! Turn-outcome gate for the session interaction loop.
//!
//! Applies the finished turn to history, checkpoints, and persistence; runs
//! the plan-handoff, approved-plan execution, verification-recovery, and
//! blocked-handoff routing; then reports whether the loop continues.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use tokio::sync::{Notify, RwLock};
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider::{LLMProvider, ToolDefinition};
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::skills::types::Skill;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::InlineHandle;

use super::super::super::blocked_handoff::write_blocked_handoff_after_checkpoint;
use super::super::super::handoff::build_approved_plan_execution_prompt;
use super::super::super::plan_seed::load_active_plan_seed;
use super::super::super::support::{
    ExecutionSummaryStatus, RefusedTurnRollback, approved_plan_execution_summary, build_withdrawn_turn_changes_note,
    format_workspace_relative_paths, remove_transient_system_notes,
};
use super::super::super::turn_tail::{TurnPersistenceTail, complete_turn_persistence_tail};
use super::super::super::{blocked_handoff, verification_recovery};
use super::super::session_bootstrap::persist_primary_agent;
use super::{corrective, plan_handoff};
use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::{HarnessEventEmitter, harness_event};
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::{CrossTurnTracker, TurnRunId};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    effective_max_tool_calls_for_approved_plan_execution, resolve_safety_tool_call_limits,
};

/// Mutable outcome-gate state threaded through post-turn finalization.
pub(super) struct OutcomeGate<'a> {
    pub turn_result: Result<TurnLoopOutcome>,
    pub aborted_turn_diagnostics: Option<SnapshotTurnDiagnostics>,
    pub session_limit_granted: bool,
    pub refused_turn_rollback: &'a RefusedTurnRollback,
    pub transient_system_notes: &'a [String],
    pub cross_turn_read_sigs: &'a Vec<String>,
    pub cross_turn_written: &'a HashSet<String>,
    pub cross_turn_shell_cmd: &'a Option<String>,
    pub cross_turn_failed_shell_key: &'a Option<String>,
    pub cross_turn_out_of_band_progress: bool,
    pub planning_active: bool,
    pub next_turn_input: &'a String,
    pub completed_turn_prompt_message_index: Option<usize>,
    pub turn_started_at: &'a Instant,
    pub turn_id: &'a str,
    pub turn_run_id: &'a TurnRunId,
    pub history_snapshot_bytes: usize,
    pub max_tool_calls_per_turn: usize,
    pub max_tool_wall_clock_secs: u64,
    pub executing_approved_plan: bool,
    pub config: &'a CoreAgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub handle: &'a InlineHandle,
    pub renderer: &'a mut AnsiRenderer,
    pub input_status_state: &'a mut InputStatusState,
    pub default_placeholder: &'a Option<String>,
    pub session_archive: &'a mut Option<SessionArchive>,
    pub session_stats: &'a mut SessionStats,
    pub runtime: &'a mut AgentRuntime,
    pub tool_registry: &'a mut ToolRegistry,
    pub context_manager: &'a mut ContextManager,
    pub agent_touched_paths: &'a mut BTreeSet<PathBuf>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub session_end_reason: &'a mut SessionEndReason,
    pub next_checkpoint_turn: &'a mut usize,
    pub last_turn_result: &'a mut Option<RunLoopTurnLoopResult>,
    pub last_turn_response_was_fallback: &'a mut bool,
    pub last_turn_diagnostics: &'a mut Option<SnapshotTurnDiagnostics>,
    pub last_approved_plan_summary_status: &'a mut Option<ExecutionSummaryStatus>,
    pub approved_plan_execution_turn: &'a mut bool,
    pub pending_approved_plan_execution_input: &'a mut bool,
    pub active_primary_agent: &'a mut ActivePrimaryAgentState,
    pub lifecycle_hooks: &'a mut Option<LifecycleHookEngine>,
    pub mcp_catalog_initialized: &'a mut bool,
    pub pending_mcp_refresh: &'a mut bool,
    pub session_skip_confirmations: &'a mut bool,
    pub tools: &'a Arc<RwLock<Vec<ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub provider_client: &'a dyn LLMProvider,
    pub loaded_skills: &'a Arc<RwLock<HashMap<String, Skill>>>,
    pub plan_session: &'a PlanningWorkflowSessionState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub safety_validator: &'a Arc<ToolCallSafetyValidator>,
    pub cross_turn_tracker: &'a mut CrossTurnTracker,
}

/// How the interaction loop proceeds after the outcome gate.
pub(super) enum OutcomeFlow {
    ContinueLoop,
    BreakLoop,
}

/// Apply the turn outcome and route the loop.
pub(super) async fn run_outcome_gate(gate: OutcomeGate<'_>) -> Result<OutcomeFlow> {
    let OutcomeGate {
        turn_result,
        aborted_turn_diagnostics,
        session_limit_granted,
        refused_turn_rollback,
        transient_system_notes,
        cross_turn_read_sigs,
        cross_turn_written,
        cross_turn_shell_cmd,
        cross_turn_failed_shell_key,
        cross_turn_out_of_band_progress,
        planning_active,
        next_turn_input,
        completed_turn_prompt_message_index,
        turn_started_at,
        turn_id,
        turn_run_id,
        history_snapshot_bytes,
        max_tool_calls_per_turn,
        max_tool_wall_clock_secs,
        executing_approved_plan,
        config,
        vt_cfg,
        handle,
        renderer,
        input_status_state,
        default_placeholder,
        session_archive,
        session_stats,
        runtime,
        tool_registry,
        context_manager,
        agent_touched_paths,
        ctrl_c_state,
        ctrl_c_notify,
        harness_emitter,
        session_end_reason,
        next_checkpoint_turn,
        last_turn_result,
        last_turn_response_was_fallback,
        last_turn_diagnostics,
        last_approved_plan_summary_status,
        approved_plan_execution_turn,
        pending_approved_plan_execution_input,
        active_primary_agent,
        lifecycle_hooks,
        mcp_catalog_initialized,
        pending_mcp_refresh,
        session_skip_confirmations,
        tools,
        tool_catalog,
        async_mcp_manager,
        provider_client,
        loaded_skills,
        plan_session,
        tool_result_cache,
        safety_validator,
        cross_turn_tracker,
    } = gate;
    // Turn prep hands history through runtime; re-derive it here so
    // the gate owns a single sequential borrow of runtime state.
    let working_history = Arc::make_mut(&mut runtime.state.messages);
    let outcome = match turn_result {
        Ok(outcome) => outcome,
        Err(err) => {
            crate::agent::runloop::unified::status_line::clear_input_status(handle, &mut *input_status_state);
            handle.set_activity_state(ActivityState::Idle);
            let _ = renderer.line_if_not_empty(MessageStyle::Output);
            handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Outcome(
                vtcode_commons::program_status::ProgramState::Error,
            ));
            tracing::error!("Turn execution error: {}", err);
            let _ = renderer.line(MessageStyle::Error, &format!("Error: {err}"));
            TurnLoopOutcome {
                result: RunLoopTurnLoopResult::Aborted,
                turn_modified_files: BTreeSet::new(),
                turn_touched_files: BTreeSet::new(),
                turn_diagnostics: aborted_turn_diagnostics.unwrap_or_default(),
                pending_primary_agent: None,
                pending_plan_execution_target: None,
                plan_approved_execution_pending: false,
                final_response_was_fallback: false,
                refused: false,
            }
        }
    };
    if session_limit_granted && !matches!(&outcome.result, RunLoopTurnLoopResult::Blocked { .. }) {
        // A successful grant resumes the pending Build turn. Do
        // not leave the previous blocked placeholder/state visible
        // or let a grant-in-flight turn be finalized as no response.
        input_status_state.is_blocked = false;
        handle.set_placeholder(default_placeholder.clone());
        handle.set_activity_state(ActivityState::Idle);
    }
    // A refused request must not stay in model-visible history:
    // the next request would replay it and be refused again. Roll
    // back before any post-turn history edits, checkpointing, or
    // persistence; the refusal notice already reached the
    // transcript and the harness event stream.
    let turn_refused = outcome.refused;
    let refused_turn_rolled_back = turn_refused && refused_turn_rollback.apply(working_history);
    if refused_turn_rolled_back {
        // The rollback removed the turn's tool calls, not their
        // effects on disk; name the files it changed so the next
        // turn does not reason from stale contents.
        if let Some(note) = build_withdrawn_turn_changes_note(config.workspace.as_path(), &outcome.turn_modified_files)
        {
            working_history.push(vtcode_core::llm::provider::Message::system(note));
        }
    } else {
        remove_transient_system_notes(working_history, transient_system_notes);
    }

    // Cross-turn loop detection: fingerprint this turn's actions and
    // inject a warning if a loop or stuck pattern is detected. The
    // warning describes activity from a rolled-back refused turn,
    // which the model no longer sees, so it is not injected then.
    if let Some(cross_turn_warning) = cross_turn_tracker.seal_turn_with_progress(
        cross_turn_read_sigs,
        cross_turn_written,
        cross_turn_shell_cmd.as_deref(),
        cross_turn_failed_shell_key.as_deref(),
        cross_turn_out_of_band_progress,
        planning_active,
    ) && !refused_turn_rolled_back
    {
        tracing::warn!(warning = %cross_turn_warning, "Cross-turn loop detector triggered");
        working_history.push(vtcode_core::llm::provider::Message::system(cross_turn_warning));
    }

    agent_touched_paths.extend(
        outcome
            .turn_modified_files
            .iter()
            .map(|path| normalize_workspace_path(config.workspace.as_path(), path)),
    );
    agent_touched_paths.extend(context_manager.tracked_instruction_activity_paths());
    let outcome_result = outcome.result.clone();
    let execution_modified_files = outcome.turn_modified_files.clone();
    let switch_primary_agent = outcome.pending_primary_agent.clone();
    let plan_execution_target = outcome.pending_plan_execution_target;
    let has_primary_agent_switch = switch_primary_agent.is_some() || plan_execution_target.is_some();
    let plan_execution_context = plan_execution_target
        .map_or(crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current, |target| {
            target.execution_context
        });
    let plan_skip_confirmations = plan_execution_target.is_some_and(|target| target.skip_confirmations);
    let plan_approved_execution_pending = outcome.plan_approved_execution_pending;
    let final_response_was_fallback = outcome.final_response_was_fallback;
    *last_turn_result = Some(outcome_result.clone());
    *last_turn_response_was_fallback = final_response_was_fallback;
    let turn_elapsed = turn_started_at.elapsed();
    let mut turn_diagnostics = outcome.turn_diagnostics.clone();
    turn_diagnostics.elapsed_ms = turn_elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    *last_turn_diagnostics = Some(turn_diagnostics.clone());
    let show_turn_timer = resolve_show_turn_timer(vt_cfg.as_ref());
    let harness_snapshot = tool_registry.harness_context_snapshot();
    if let Err(err) = crate::agent::runloop::unified::turn::apply_turn_outcome(
        outcome,
        crate::agent::runloop::unified::turn::TurnOutcomeContext {
            conversation_history: Arc::make_mut(&mut runtime.state.messages),
            completed_turn_prompt: Some(next_turn_input.as_str()),
            completed_turn_prompt_message_index,
            renderer: &mut *renderer,
            handle,
            ctrl_c_state,
            default_placeholder,
            checkpoint_manager: None,
            next_checkpoint_turn: &mut *next_checkpoint_turn,
            session_end_reason: &mut *session_end_reason,
            turn_elapsed,
            show_turn_timer,
            session_id: &harness_snapshot.session_id,
            runtime_turn_id: Some(turn_id),
            harness_emitter: harness_emitter.as_ref(),
        },
    )
    .await
    {
        tracing::error!("Failed to apply turn outcome: {}", err);
        renderer
            .line(MessageStyle::Error, &format!("Failed to finalize turn: {err}"))
            .ok();
    }
    if executing_approved_plan {
        handle.set_activity_state(ActivityState::Idle);
    }
    // Plan-mode "switch to build/auto agent" handoff: perform the
    // primary-agent switch now so the chosen agent executes the plan.
    // This mirrors the `PlanApproved` handoff in the interaction loop
    // (session.rs): mutate `active_primary_agent` and refresh the TUI
    // handle display. The full `handle_select_primary_agent` requires
    // `InteractionLoopContext`, which is unavailable here because the
    // plan-confirmation popup is rendered inside the turn loop rather
    // than the inline interaction loop.
    //
    // Plan *entry* (`start_planning` confirmation) also lands here via
    // `pending_primary_agent = "plan"`. That destination is the plan
    // agent itself, not an approved-plan execution agent, so it uses
    // direct selection instead of the write-capable resolver.
    if plan_handoff::run_plan_handoff(plan_handoff::PlanHandoff {
        active_primary_agent: &mut *active_primary_agent,
        renderer: &mut *renderer,
        tool_registry: &mut *tool_registry,
        lifecycle_hooks: &mut *lifecycle_hooks,
        mcp_catalog_initialized: &mut *mcp_catalog_initialized,
        pending_mcp_refresh: &mut *pending_mcp_refresh,
        session_archive: &mut *session_archive,
        runtime: &mut *runtime,
        session_skip_confirmations: &mut *session_skip_confirmations,
        vt_cfg: &mut *vt_cfg,
        config,
        turn_run_id,
        tools,
        tool_catalog,
        async_mcp_manager,
        provider_client,
        handle,
        session_stats,
        loaded_skills,
        turn_diagnostics: &turn_diagnostics,
        harness_session_id: &harness_snapshot.session_id,
        harness_emitter,
        plan_execution_target,
        switch_primary_agent,
        history_snapshot_bytes,
        max_tool_wall_clock_secs,
        turn_elapsed,
        next_checkpoint_turn: *next_checkpoint_turn,
        plan_skip_confirmations,
    })
    .await?
    {
        return Ok(OutcomeFlow::ContinueLoop);
    }
    if plan_approved_execution_pending && !has_primary_agent_switch {
        *session_skip_confirmations = plan_skip_confirmations;
        handle.set_skip_confirmations(*session_skip_confirmations);
    }
    if plan_approved_execution_pending {
        let fresh_context = matches!(
            plan_execution_context,
            crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh
        );
        if fresh_context {
            handle.set_activity_state(ActivityState::PreparingFreshExecutionThread);
        }
        let mut plan_seed = load_active_plan_seed(tool_registry).await;
        if fresh_context && plan_seed.is_none() {
            handle.set_activity_state(ActivityState::Idle);
            if let Err(error) = renderer.line(
                MessageStyle::Error,
                "Fresh execution could not start because the approved plan was not found. The plan was retained; please retry approval.",
            ) {
                if let Some(emitter) = harness_emitter.as_ref() {
                    emitter.finish_after_unexpected_exit().await;
                }
                return Err(error);
            }
            return Ok(OutcomeFlow::ContinueLoop);
        }
        if fresh_context {
            let current_context_budget = vtcode_core::compaction::effective_context_budget(
                vt_cfg.as_ref(),
                provider_client,
                &crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model(
                    &config.model,
                    active_primary_agent.active(),
                ),
            );
            let previous_context_usage_percent = context_manager.context_usage_percent(current_context_budget);
            plan_seed = load_active_plan_seed(tool_registry).await.or(plan_seed.take());
            handle.set_activity_state(ActivityState::RestoringApprovedPlan);
            runtime.clear_pending_follow_up_inputs();
            runtime.state.clear_conversation_history();
            context_manager.reset_for_fresh_execution();
            session_stats.reset_for_fresh_execution();
            let build_tool_limit = effective_max_tool_calls_for_approved_plan_execution(max_tool_calls_per_turn);
            let max_session_turns = vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.max_conversation_turns)
                .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_CONVERSATION_TURNS);
            let (max_per_turn, max_per_session) =
                resolve_safety_tool_call_limits(build_tool_limit, max_session_turns, false);
            safety_validator.reset_for_fresh_execution(max_per_turn, max_per_session);
            crate::agent::runloop::unified::planning_workflow::emit_context_reset(
                harness_emitter.as_ref(),
                turn_run_id.0.clone(),
                turn_id.to_owned(),
                previous_context_usage_percent,
            );
            handle.set_activity_state(ActivityState::StartingBuild);
        }
        *approved_plan_execution_turn = true;
        *pending_approved_plan_execution_input = true;
        let execution_directive = build_approved_plan_execution_prompt(plan_execution_context, plan_seed.as_deref());
        runtime
            .state
            .messages_mut()
            .push(vtcode_core::llm::provider::Message::system(execution_directive));
        handle.set_activity_state(ActivityState::Building);
        persist_primary_agent(&mut *session_archive, active_primary_agent);
    }
    if executing_approved_plan {
        let summary = approved_plan_execution_summary(
            tool_registry,
            &outcome_result,
            final_response_was_fallback,
            !execution_modified_files.is_empty(),
        )
        .await;
        *last_approved_plan_summary_status = Some(summary.status);
        let changed_files = format_workspace_relative_paths(config.workspace.as_path(), &execution_modified_files);
        let _ = renderer.line(
                MessageStyle::Info,
                &format!(
                    "Execution summary: {}; changed files: {changed_files}; verification: see the final response and task tracker; blockers: {}.",
                    summary.status.as_str(),
                    summary.blocker.as_deref().unwrap_or("none")
                ),
            );
    }
    // Stop can arrive after the provider finishes. Do not hold
    // exit behind cache locks or checkpoint maintenance; archive
    // workers retain ownership of their atomic writes.
    let checkpoint_outcome =
        match crate::agent::runloop::unified::stop_requests::await_with_stop(ctrl_c_state, ctrl_c_notify, async {
            vtcode_core::tools::cache::FILE_CACHE.check_pressure_and_evict().await;
            tool_result_cache.write().await.check_pressure_and_evict();
            let blocked_turn = matches!(&outcome_result, RunLoopTurnLoopResult::Blocked { .. });
            persist_primary_agent(&mut *session_archive, active_primary_agent);
            complete_turn_persistence_tail(TurnPersistenceTail {
                outcome: match &outcome_result {
                    RunLoopTurnLoopResult::Completed { .. } => "completed",
                    RunLoopTurnLoopResult::Aborted => "aborted",
                    RunLoopTurnLoopResult::Cancelled => "cancelled",
                    RunLoopTurnLoopResult::Exit => "exit",
                    RunLoopTurnLoopResult::Blocked { .. } => "blocked",
                },
                history_snapshot_bytes,
                timeout_secs: max_tool_wall_clock_secs,
                elapsed_ms: turn_elapsed.as_millis(),
                blocked_turn,
                turn_diagnostics: Some(turn_diagnostics),
                runtime: &mut *runtime,
                session_archive: &mut *session_archive,
                next_checkpoint_turn: *next_checkpoint_turn,
                session_stats,
                loaded_skills,
                workspace: config.workspace.as_path(),
                session_id: &harness_snapshot.session_id,
                vt_cfg: vt_cfg.as_ref(),
            })
            .await
        })
        .await
        {
            Some(checkpoint) => checkpoint,
            None => {
                if ctrl_c_state.is_exit_requested() {
                    *session_end_reason = SessionEndReason::Exit;
                    return Ok(OutcomeFlow::BreakLoop);
                }
                ctrl_c_state.mark_cancel_handled();
                *session_end_reason = SessionEndReason::Cancelled;
                *last_turn_result = Some(RunLoopTurnLoopResult::Cancelled);
                handle.set_activity_state(ActivityState::Idle);
                return Ok(OutcomeFlow::ContinueLoop);
            }
        };
    // Tracker-aware outer auto-continue after checkpoint/persistence:
    // incomplete tracker work + recoverable turn end → queue the next
    // turn instead of nudging the user. Verification blocks keep their
    // existing recovery path first (handled below).
    // Set when tracker auto-queue was eligible but could not
    // resume (queue full / cross-turn budget exhausted) while
    // incomplete tracker steps remain. The exhausted-path info
    // line is the single user-facing nudge; suppress the generic
    // blocked-handoff "Type continue" stack and blocked placeholder
    // for this recoverable budget end.
    let tracker_auto_continue_exhausted = match corrective::run_corrective_gate(corrective::CorrectiveGate {
        session_stats: &mut *session_stats,
        runtime: &mut *runtime,
        renderer: &mut *renderer,
        tool_registry,
        plan_session,
        vt_cfg: &*vt_cfg,
        outcome_result: &outcome_result,
        ctrl_c_state,
        next_turn_input,
        turn_refused,
        refused_turn_rolled_back,
        session_end_reason: *session_end_reason,
    })
    .await?
    {
        corrective::CorrectiveFlow::ContinueLoop => return Ok(OutcomeFlow::ContinueLoop),
        corrective::CorrectiveFlow::BreakLoop => return Ok(OutcomeFlow::BreakLoop),
        corrective::CorrectiveFlow::Proceed { tracker_auto_continue_exhausted } => tracker_auto_continue_exhausted,
    };
    // A refusal is terminal for its request, not a stall to resume:
    // no blocked handoff, no stall reason for `continue` to replay.
    if let RunLoopTurnLoopResult::Blocked { reason } = &outcome_result
        && !turn_refused
    {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers as verification_gate;

        let base = reason.as_deref().unwrap_or("Turn blocked due to repeated failing behavior.");
        use verification_recovery::VerificationBlockResult;

        let recovery = verification_recovery::handle_verification_block(
            base,
            vt_cfg.as_ref(),
            tool_registry,
            &mut *runtime,
            &mut *session_stats,
        );
        // Tracker/plan budget ends already printed their exhausted nudge.
        let suppress_blocked_nudge = tracker_auto_continue_exhausted
            && matches!(recovery, VerificationBlockResult::NotVerification)
            && verification_gate::tracker_auto_continue_is_recoverable_block(Some(base));
        let base_owned = match recovery {
            VerificationBlockResult::NotVerification => base.to_owned(),
            VerificationBlockResult::Queued { status_line } => {
                let _ = renderer.line(MessageStyle::Info, &status_line);
                if matches!(session_end_reason, SessionEndReason::Exit) {
                    return Ok(OutcomeFlow::BreakLoop);
                }
                return Ok(OutcomeFlow::ContinueLoop);
            }
            VerificationBlockResult::Handoff { reason } => reason,
        };
        let summary = blocked_handoff::blocker_summary_with_diagnostics(
            &base_owned,
            last_turn_diagnostics.as_ref(),
            &session_stats.sorted_tools(),
        );
        if !suppress_blocked_nudge {
            write_blocked_handoff_after_checkpoint(
                &config.workspace,
                &harness_snapshot.session_id,
                &summary,
                checkpoint_outcome.blocked_handoff_resume(),
                &mut *renderer,
                harness_emitter.as_ref(),
                Some(handle),
                tool_registry.is_planning_active(),
            );
        } else {
            // Keep forensics artifacts without the user-nudge stack.
            blocked_handoff::persist_blocked_handoff_quiet(
                &config.workspace,
                &harness_snapshot.session_id,
                &summary,
                checkpoint_outcome.blocked_handoff_resume(),
                tool_registry.is_planning_active(),
            );
        }
    }
    match &outcome_result {
        RunLoopTurnLoopResult::Completed { .. } => {
            let handoff_resolved = vtcode_core::core::agent::blocked_handoff::clear_current_blocked_handoff_for_session(
                &config.workspace,
                &harness_snapshot.session_id,
            );
            match handoff_resolved {
                Ok(true) => {
                    if let Some(emitter) = harness_emitter.as_ref() {
                        let _ = emitter.emit(harness_event(
                            vtcode_core::exec::events::HarnessEventKind::BlockedHandoffResolved,
                            Some("Blocked handoff resolved (turn recovered; task may remain incomplete)".to_owned()),
                            None,
                            None,
                            None,
                        ));
                    }
                }
                Ok(false) => {}
                Err(err) => tracing::warn!(error = %err, "Failed to resolve current blocked handoff"),
            }
            session_stats.mark_turn_stalled(false, None);
            session_stats.reset_verification_recovery_episode();
        }
        RunLoopTurnLoopResult::Aborted => {
            session_stats.mark_turn_stalled(true, Some("Turn aborted due to an execution error.".to_string()));
        }
        RunLoopTurnLoopResult::Blocked { .. } if turn_refused => {
            handle
                .set_placeholder(Some("Request declined · Rephrase it, or use /model to switch models...".to_string()));
            // Blocked status makes the next input restore the
            // default placeholder.
            input_status_state.is_blocked = true;
            session_stats.mark_turn_stalled(false, None);
        }
        RunLoopTurnLoopResult::Blocked { reason } => {
            // Plan-mode QoL: a blocked placeholder that still says
            // `continue` re-blocks on the next turn. Name the mode
            // switch explicitly; the transcript lines written above
            // carry the full reasoning. Never auto-switch here.
            // Plan-mode guidance wins over the tracker-exhausted
            // hint because it names the actionable mode switch.
            if tool_registry.is_planning_active() {
                handle.set_placeholder(Some(
                    "Plan blocked (read-only) · `continue` to keep planning, `/mode build` to implement...".to_string(),
                ));
            } else if tracker_auto_continue_exhausted {
                handle.set_placeholder(Some(
                    "Tracker auto-continue exhausted · Type 'continue' to resume remaining steps...".to_string(),
                ));
            } else {
                handle.set_placeholder(Some(
                    "Turn blocked · Type 'continue' to retry or describe changes...".to_string(),
                ));
            }
            input_status_state.is_blocked = true;
            session_stats.mark_turn_stalled(
                true,
                reason
                    .clone()
                    .or_else(|| Some("Turn blocked due to repeated failing tool behavior.".to_string())),
            );
            if !renderer.supports_inline_ui()
                && session_stats.auto_permission_prompt_fallback_active()
                && session_stats.last_auto_permission_denial().is_some()
            {
                *session_end_reason = SessionEndReason::Error;
                return Ok(OutcomeFlow::BreakLoop);
            }
        }
        _ => {
            session_stats.mark_turn_stalled(false, None);
            // Cancelled/Exit ends the stall episode: a later
            // verification block starts with a fresh auto-recovery
            // budget and failure count instead of inheriting them.
            session_stats.reset_verification_recovery_episode();
        }
    }
    if matches!(session_end_reason, SessionEndReason::Exit) {
        return Ok(OutcomeFlow::BreakLoop);
    }
    Ok(OutcomeFlow::ContinueLoop)
}

/// Resolve whether the per-turn elapsed timer line renders.
///
/// Missing config means default config, and the timer defaults off
/// (`ui.show_turn_timer`), so `None` must not re-enable it.
fn resolve_show_turn_timer(vt_cfg: Option<&VTCodeConfig>) -> bool {
    vt_cfg.is_some_and(|cfg| cfg.ui.show_turn_timer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_disables_turn_timer() {
        assert!(!resolve_show_turn_timer(None));
    }

    #[test]
    fn default_config_disables_turn_timer() {
        assert!(!resolve_show_turn_timer(Some(&VTCodeConfig::default())));
    }

    #[test]
    fn explicit_config_opt_in_enables_turn_timer() {
        let mut cfg = VTCodeConfig::default();
        cfg.ui.show_turn_timer = true;
        assert!(resolve_show_turn_timer(Some(&cfg)));
    }
}
