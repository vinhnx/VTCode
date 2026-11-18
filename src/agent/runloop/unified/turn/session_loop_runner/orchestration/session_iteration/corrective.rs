//! Corrective and auto-continue gate for the session interaction loop.
//!
//! Runs the preflight corrective turn plus tracker/plan-mode auto-continue
//! scheduling after each turn, queuing synthetic follow-ups within budget.

use std::sync::Arc;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::tools::ToolRegistry;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};

use super::super::super::support::latest_assistant_result_text;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;
use crate::agent::runloop::unified::turn::session::slash_commands::SharedGoalTracker;

/// Mutable corrective-gate state threaded through auto-continue scheduling.
pub(super) struct CorrectiveGate<'a> {
    pub session_stats: &'a mut SessionStats,
    pub runtime: &'a mut AgentRuntime,
    pub renderer: &'a mut AnsiRenderer,
    pub tool_registry: &'a ToolRegistry,
    pub plan_session: &'a PlanningWorkflowSessionState,
    pub goal_tracker: &'a SharedGoalTracker,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub vt_cfg: &'a Option<VTCodeConfig>,
    pub outcome_result: &'a RunLoopTurnLoopResult,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub next_turn_input: &'a str,
    pub turn_refused: bool,
    pub refused_turn_rolled_back: bool,
    pub session_end_reason: SessionEndReason,
}

/// How the interaction loop proceeds after the corrective gate.
pub(super) enum CorrectiveFlow {
    ContinueLoop,
    BreakLoop,
    Proceed { tracker_auto_continue_exhausted: bool },
}

/// Run preflight-corrective and tracker/plan auto-continue scheduling.
pub(super) async fn run_corrective_gate(gate: CorrectiveGate<'_>) -> Result<CorrectiveFlow> {
    let CorrectiveGate {
        session_stats,
        runtime,
        renderer,
        tool_registry,
        plan_session,
        goal_tracker,
        harness_emitter,
        vt_cfg,
        outcome_result,
        ctrl_c_state,
        next_turn_input,
        turn_refused,
        refused_turn_rolled_back,
        session_end_reason,
    } = gate;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;
    let mut tracker_auto_continue_exhausted = false;
    let max_corrective_turns = tracker_continue::tracker_cross_turn_turns(vt_cfg.as_ref());
    let corrective_budget_remaining = if tool_registry.is_planning_active() {
        session_stats.plan_continuation_turns() < max_corrective_turns
    } else {
        session_stats.tracker_continuation_turns() < max_corrective_turns
    };
    // Validation owns one corrective turn independently of tracker progress
    // and model refusal prose. Permission, verification and stop gates remain
    // authoritative; queue failure falls through to the existing handoff.
    if session_stats.preflight_recovery.corrective_pending()
        && !session_stats.navigation_recovery.blocked()
        && !turn_refused
        && crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
        && tracker_continue::tracker_auto_continue_enabled(vt_cfg.as_ref())
        && corrective_budget_remaining
        && !session_stats.verification_snapshot().0
        && (matches!(outcome_result, RunLoopTurnLoopResult::Completed { .. })
            || matches!(outcome_result, RunLoopTurnLoopResult::Blocked { reason }
                        if tracker_continue::tracker_auto_continue_is_recoverable_block(reason.as_deref())
                            && !reason.as_deref().is_some_and(tracker_continue::is_pending_verification_block)))
        && !(tool_registry.is_planning_active()
            && crate::agent::runloop::unified::planning_workflow::persisted_plan_is_ready(
                &tool_registry.planning_workflow_state(),
            )
            .await)
    {
        let follow_up = format!(
            "{} Tools are available under current policy. Resume the original request by correcting the rejected arguments against the current tool schema. Omit unused fields; do not repeat an unchanged rejected call. This is the single automatic corrective turn; no user reply is needed.",
            crate::agent::runloop::unified::state::PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX
        );
        if runtime.try_queue_synthetic_follow_up_input(follow_up).is_ok() {
            session_stats.preflight_recovery.begin_corrective_turn();
            if tool_registry.is_planning_active() {
                session_stats.record_plan_continuation_turn_with_limit(max_corrective_turns);
            } else {
                session_stats.record_tracker_continuation_turn_with_limit(max_corrective_turns);
            }
            return Ok(CorrectiveFlow::ContinueLoop);
        }
        tracing::warn!("Preflight corrective queue full; falling through to handoff");
    }
    {
        let planning_active = tool_registry.is_planning_active();
        let tracker_kill_switch =
            auto_continue_kill_switch(outcome_result, ctrl_c_state, vt_cfg.as_ref(), session_stats);
        let is_verification_block = matches!(outcome_result, RunLoopTurnLoopResult::Blocked { reason }
                if reason.as_deref().is_some_and(tracker_continue::is_pending_verification_block));
        let turn_completed = matches!(outcome_result, RunLoopTurnLoopResult::Completed { .. });
        let blocked_reason = match outcome_result {
            RunLoopTurnLoopResult::Blocked { reason } => reason.as_deref(),
            _ => None,
        };
        let tracker_adoption_allowed = tool_registry.tracker_adopted_for_request();
        let incomplete = if tracker_kill_switch && !planning_active && tracker_adoption_allowed {
            let probe = tracker_continue::probe_tracker_incomplete(tool_registry).await;
            // Complete clears the cache so auto-queue stops after tracker finishes.
            session_stats.apply_tracker_probe(probe).map(|items| items.to_vec())
        } else {
            None
        };
        // Progress-reset: any newly completed tracker step restores
        // the cross-turn auto-continue episode budget plus the
        // tracker continuation budget. Verification recovery stays
        // bounded to one corrective pass for the original request.
        if tracker_kill_switch
            && tracker_adoption_allowed
            && let Some(completed) = tracker_continue::tracker_completed_count(tool_registry).await
            && session_stats.note_tracker_completed_count(completed)
        {
            session_stats.reset_tracker_continuation_budget();
        }
        let max_turns = tracker_continue::tracker_cross_turn_turns(vt_cfg.as_ref());
        // A rolled-back refused turn left no final text; the latest
        // assistant message belongs to an earlier turn.
        let final_text = if refused_turn_rolled_back {
            None
        } else {
            latest_assistant_result_text(&runtime.state.messages)
        };
        // Adoption gate: a fresh informational turn must not be
        // pulled into unrelated workspace tracker work. Completed
        // turns only auto-queue tracker work adopted by this
        // request through successful tracker use or explicit
        // continuation. Blocked recoverable ends keep their own
        // classifier; restored sessions use the same adoption gate.
        let final_text_is_safety_handoff = vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(
            final_text.as_deref().unwrap_or(""),
        );
        let final_text_requires_user_input =
            vtcode_core::core::agent::completion::tracker_final_text_requires_user_input(
                final_text.as_deref().unwrap_or(""),
            );
        let should_queue = tracker_continue::should_queue_tracker_auto_continue(
            tracker_kill_switch,
            planning_active,
            turn_completed,
            blocked_reason,
            is_verification_block,
            incomplete.as_deref(),
            max_turns,
            final_text_is_safety_handoff,
            final_text_requires_user_input,
        ) && (!turn_completed || tracker_adoption_allowed);
        // Persisted planning outcomes control continuations. Approval,
        // input waits, exhausted requests, and hard stops stay idle.
        let plan_state = tool_registry.planning_workflow_state();
        let plan_ready_for_approval = planning_active
            && crate::agent::runloop::unified::planning_workflow::persisted_plan_is_ready(&plan_state).await;
        let should_queue_plan = tracker_continue::should_queue_plan_mode_auto_continue(
            &plan_session.recovery,
            tracker_continue::PlanningAutoContinueGates {
                enabled: tracker_kill_switch,
                planning_active,
                plan_ready_for_approval,
                turn_completed,
                verification_blocked: is_verification_block,
                refused: turn_refused,
                hard_budget_exhausted: planning_budget_exhausted(plan_session.is_budget_exhausted(), blocked_reason),
                stop_requested: crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_some(),
                cross_turn_turns: max_turns,
            },
        );
        if should_queue_plan {
            let follow_up = tracker_continue::plan_mode_continue_follow_up();
            let directive = tracker_continue::plan_mode_auto_continue_directive();
            let budget_remaining = session_stats.plan_continuation_turns() < max_turns;
            let queued = budget_remaining
                && crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
                && match runtime.try_queue_synthetic_follow_up_input(follow_up) {
                    Ok(()) => {
                        session_stats.record_plan_continuation_turn_with_limit(max_turns);
                        Arc::make_mut(&mut runtime.state.messages)
                            .push(vtcode_core::llm::provider::Message::system(directive));
                        // Queued auto-continue stays quiet: the next turn starts
                        // immediately, so a TUI info line is noise. Exhausted /
                        // queue-full paths below still inform the user.
                        tracing::debug!(
                            plan_turn = session_stats.plan_continuation_turns(),
                            max_turns,
                            "Queued plan-mode auto-continue without TUI echo"
                        );
                        true
                    }
                    Err(err) => {
                        tracing::warn!(
                            %err,
                            "Plan-mode auto-continue queue full; falling through to turn end"
                        );
                        let _ = renderer.line(
                                    MessageStyle::Warning,
                                    "[i] Plan-mode auto-continue could not resume automatically; planning remains active. Type `continue` to resume planning.",
                                );
                        tracker_auto_continue_exhausted = true;
                        false
                    }
                };
            if queued {
                if matches!(session_end_reason, SessionEndReason::Exit) {
                    return Ok(CorrectiveFlow::BreakLoop);
                }
                return Ok(CorrectiveFlow::ContinueLoop);
            }
            if !budget_remaining {
                let _ = renderer.line(
                            MessageStyle::Info,
                            "[i] Plan-mode auto-continue budget exhausted; planning remains active. Type `continue` to resume planning.",
                        );
                tracker_auto_continue_exhausted = true;
            }
            if planning_active && !plan_ready_for_approval {
                let _ = renderer.line(MessageStyle::Info, &tracker_continue::plan_progress_line("", false, 0, 0));
            }
        } else if should_queue {
            let incomplete = incomplete.unwrap_or_default();
            let (follow_up, directive) = if incomplete.is_empty() {
                let reason = blocked_reason.unwrap_or("recoverable block");
                (
                    tracker_continue::recoverable_blocked_continue_follow_up(reason),
                    tracker_continue::recoverable_blocked_auto_continue_directive(reason),
                )
            } else {
                (
                    tracker_continue::tracker_continue_follow_up(&incomplete),
                    tracker_continue::tracker_continue_directive(
                        tracker_continue::TRACKER_AUTO_CONTINUE_DIRECTIVE_LABEL,
                        &incomplete,
                    ),
                )
            };
            let budget_remaining = session_stats.tracker_continuation_turns() < max_turns;
            let queued = budget_remaining
                && crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
                && match runtime.try_queue_synthetic_follow_up_input(follow_up) {
                    Ok(()) => {
                        session_stats.record_tracker_continuation_turn_with_limit(max_turns);
                        Arc::make_mut(&mut runtime.state.messages)
                            .push(vtcode_core::llm::provider::Message::system(directive));
                        // Queued auto-continue stays quiet: the next turn starts
                        // immediately, so a TUI info line is noise (notably the
                        // plan-accept → build handoff with pending tracker
                        // steps). Exhausted / queue-full paths below still
                        // inform the user.
                        tracing::debug!(
                            tracker_turn = session_stats.tracker_continuation_turns(),
                            max_turns,
                            incomplete = incomplete.len(),
                            "Queued tracker auto-continue without TUI echo"
                        );
                        true
                    }
                    Err(err) => {
                        tracing::warn!(%err, "Tracker auto-continue queue full; falling through to turn end");
                        let _ = renderer.line(
                                    MessageStyle::Warning,
                                    if incomplete.is_empty() {
                                        "[i] Blocked-end auto-continue could not resume automatically. Type `continue` to retry the request."
                                    } else {
                                        "[i] Tracker auto-continue could not resume automatically; incomplete tracker steps remain. Type `continue` to resume remaining steps."
                                    },
                                );
                        tracker_auto_continue_exhausted = true;
                        false
                    }
                };
            if queued {
                if matches!(session_end_reason, SessionEndReason::Exit) {
                    return Ok(CorrectiveFlow::BreakLoop);
                }
                return Ok(CorrectiveFlow::ContinueLoop);
            }
            if !budget_remaining {
                let _ = renderer.line(
                            MessageStyle::Info,
                            if incomplete.is_empty() {
                                "[i] Blocked-end auto-continue budget exhausted. Type `continue` to retry the request."
                            } else {
                                "[i] Tracker auto-continue budget exhausted; incomplete tracker steps remain. Type `continue` to resume remaining steps."
                            },
                        );
                tracker_auto_continue_exhausted = true;
            }
        } else if planning_active && plan_ready_for_approval && turn_completed {
            session_stats.reset_plan_continuation_budget();
            let _ = renderer.line(MessageStyle::Info, &tracker_continue::plan_progress_line("", true, 0, 0));
        } else if planning_active
            && !plan_ready_for_approval
            && !turn_completed
            && !should_queue_plan
            && plan_session.recovery.outcome != vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput
        {
            let _ = renderer.line(MessageStyle::Info, &tracker_continue::plan_progress_line("", false, 0, 0));
        } else if !planning_active
            && session_stats.preflight_recovery.is_clear()
            && !crate::agent::runloop::unified::turn::is_internal_harness_follow_up(next_turn_input)
            && incomplete.as_ref().is_none_or(|items| items.is_empty())
        {
            // Tracker work cleared (or none) outside planning: reset the
            // episode budgets only at a user boundary. Internal correction
            // success must not restore consumed continuation turns.
            session_stats.reset_tracker_continuation_budget();
            session_stats.reset_plan_continuation_budget();
        }
    }
    // Goal-loop fallback runs last: none of the corrective/plan/tracker
    // branches queued work, so an Active goal may continue the thread.
    run_goal_gate_tail(GoalGateTail {
        session_stats,
        runtime,
        renderer,
        tool_registry,
        goal_tracker,
        harness_emitter,
        vt_cfg,
        outcome_result,
        ctrl_c_state,
        turn_refused,
        refused_turn_rolled_back,
        session_end_reason,
        incoming_exhausted: tracker_auto_continue_exhausted,
    })
    .await
}

/// Shared auto-continue kill switch: no stop/exit/cancel, auto-continue
/// enabled, and no blocking recovery state. Used by the corrective gate and
/// the goal tail gate alike.
fn auto_continue_kill_switch(
    outcome_result: &RunLoopTurnLoopResult,
    ctrl_c_state: &Arc<CtrlCState>,
    vt_cfg: Option<&VTCodeConfig>,
    session_stats: &SessionStats,
) -> bool {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;
    !matches!(outcome_result, RunLoopTurnLoopResult::Cancelled | RunLoopTurnLoopResult::Exit)
        && crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
        && tracker_continue::tracker_auto_continue_enabled(vt_cfg)
        && !session_stats.navigation_recovery.blocked()
        && !session_stats.preflight_recovery.exhausted()
        && !session_stats.preflight_recovery.corrective_pending()
}

/// Inputs for the goal-loop tail gate.
struct GoalGateTail<'a> {
    session_stats: &'a mut SessionStats,
    runtime: &'a mut AgentRuntime,
    renderer: &'a mut AnsiRenderer,
    tool_registry: &'a ToolRegistry,
    goal_tracker: &'a SharedGoalTracker,
    harness_emitter: Option<&'a HarnessEventEmitter>,
    vt_cfg: &'a Option<VTCodeConfig>,
    outcome_result: &'a RunLoopTurnLoopResult,
    ctrl_c_state: &'a Arc<CtrlCState>,
    turn_refused: bool,
    refused_turn_rolled_back: bool,
    session_end_reason: SessionEndReason,
    incoming_exhausted: bool,
}

/// Goal-loop fallback gate: per-turn accounting plus one auto-continue
/// attempt. An Active goal keeps Completed turns going until paused, cleared,
/// stopped, or budget-cut. Completed-only continuation keeps hard budget
/// stops (a handoff, never auto-continue) idle.
async fn run_goal_gate_tail(tail: GoalGateTail<'_>) -> Result<CorrectiveFlow> {
    let GoalGateTail {
        session_stats,
        runtime,
        renderer,
        tool_registry,
        goal_tracker,
        harness_emitter,
        vt_cfg,
        outcome_result,
        ctrl_c_state,
        turn_refused,
        refused_turn_rolled_back,
        session_end_reason,
        incoming_exhausted,
    } = tail;
    let idle = |exhausted: bool| CorrectiveFlow::Proceed {
        tracker_auto_continue_exhausted: incoming_exhausted || exhausted,
    };
    let mut tracker = goal_tracker.lock().await;
    if tracker.has_goal() {
        tracker.account_elapsed();
        tracker.record_token_observation(session_stats.total_tokens_i64());
        if let Err(err) = tracker.persist() {
            tracing::warn!("goal snapshot persist failed: {err:#}");
        }
    }
    let objective = if tracker.is_active() {
        tracker.objective().unwrap_or_default().to_string()
    } else {
        return Ok(idle(false));
    };
    if let Some(emitter) = harness_emitter
        && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
    {
        tracing::warn!("goal event emit failed: {err:#}");
    }
    drop(tracker);
    let max_turns =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_cross_turn_turns(vt_cfg.as_ref());
    let turn_completed = matches!(outcome_result, RunLoopTurnLoopResult::Completed { .. });
    let final_text = if refused_turn_rolled_back {
        None
    } else {
        latest_assistant_result_text(&runtime.state.messages)
    };
    let completion_claimed = goal_claims_completion(final_text.as_deref())
        && matches!(outcome_result, RunLoopTurnLoopResult::Completed { .. } | RunLoopTurnLoopResult::Blocked { .. })
        && !turn_refused;
    if completion_claimed {
        let mut tracker = goal_tracker.lock().await;
        if tracker.complete() {
            if let Err(err) = tracker.persist() {
                tracing::warn!("goal snapshot persist failed: {err:#}");
            }
            if let Some(emitter) = harness_emitter
                && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
            {
                tracing::warn!("goal event emit failed: {err:#}");
            }
            let done = format!("[i] Goal complete: {}", tracker.objective().unwrap_or_default());
            let _ = renderer.line(MessageStyle::Info, &done);
        }
        return Ok(idle(false));
    }
    {
        let mut tracker = goal_tracker.lock().await;
        if tracker.enforce_budget() {
            if let Err(err) = tracker.persist() {
                tracing::warn!("goal snapshot persist failed: {err:#}");
            }
            if let Some(emitter) = harness_emitter
                && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
            {
                tracing::warn!("goal event emit failed: {err:#}");
            }
            let _ = renderer.line(
                MessageStyle::Info,
                "[i] Goal hit its token budget and is now limited. Clear it or set a new goal to continue.",
            );
            return Ok(idle(false));
        }
    }
    {
        let tracker = goal_tracker.lock().await;
        if let Some(line) = crate::agent::runloop::unified::turn::session::slash_commands::goal_indicator_line(&tracker)
        {
            let _ = renderer.line(MessageStyle::Info, &line);
        }
    }
    let should_queue = auto_continue_kill_switch(outcome_result, ctrl_c_state, vt_cfg.as_ref(), session_stats)
        && !tool_registry.is_planning_active()
        && turn_completed
        && !vtcode_core::core::agent::completion::tracker_final_text_requires_user_input(
            final_text.as_deref().unwrap_or(""),
        )
        && !vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(
            final_text.as_deref().unwrap_or(""),
        )
        && !matches!(outcome_result, RunLoopTurnLoopResult::Blocked { reason }
        if reason.as_deref().is_some_and(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::is_pending_verification_block
        ))
        && !turn_refused
        // Already-queued input runs first; the gate re-runs after that
        // turn, so idling here only defers the follow-up by one turn.
        && !runtime.has_pending_follow_up_inputs();
    if !should_queue {
        return Ok(idle(false));
    }
    match run_goal_auto_continue(GoalAutoContinue {
        goal_tracker,
        runtime,
        renderer,
        ctrl_c_state,
        objective,
        max_turns,
    })
    .await?
    {
        GoalContinueOutcome::Queued => {
            if matches!(session_end_reason, SessionEndReason::Exit) {
                return Ok(CorrectiveFlow::BreakLoop);
            }
            Ok(CorrectiveFlow::ContinueLoop)
        }
        GoalContinueOutcome::Idle { exhausted } => Ok(idle(exhausted)),
    }
}

/// Inputs for one goal auto-continue attempt.
struct GoalAutoContinue<'a> {
    goal_tracker: &'a SharedGoalTracker,
    runtime: &'a mut AgentRuntime,
    renderer: &'a mut AnsiRenderer,
    ctrl_c_state: &'a Arc<CtrlCState>,
    objective: String,
    max_turns: u8,
}

/// Outcome of one goal auto-continue attempt.
enum GoalContinueOutcome {
    /// A follow-up was queued; the loop must continue (or break on Exit).
    Queued,
    /// Stayed idle; `exhausted` reports budget/queue failure upstream.
    Idle { exhausted: bool },
}

/// Queue one goal-loop follow-up within budget, mirroring the tracker branch.
async fn run_goal_auto_continue(gate: GoalAutoContinue<'_>) -> Result<GoalContinueOutcome> {
    let GoalAutoContinue {
        goal_tracker,
        runtime,
        renderer,
        ctrl_c_state,
        objective,
        max_turns,
    } = gate;
    // Reserve budget first: a failed queue consumes one turn rather than looping.
    let Some(turn_no) = goal_tracker.lock().await.claim_auto_continue_turn(max_turns) else {
        let _ = renderer.line(
            MessageStyle::Info,
            "[i] Goal auto-continue budget exhausted; the goal stays active. Type `continue` to resume it.",
        );
        return Ok(GoalContinueOutcome::Idle { exhausted: true });
    };
    if crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_some() {
        return Ok(GoalContinueOutcome::Idle { exhausted: false });
    }
    let follow_up = format!(
        "{}\"{objective}\". Build on prior turns in this thread; if blocked or needing input, say so instead of stopping silently. {}",
        crate::agent::runloop::unified::turn::session::slash_commands::GOAL_AUTO_CONTINUE_PREFIX,
        crate::agent::runloop::unified::turn::session::slash_commands::GOAL_COMPLETION_CONTRACT
    );
    let directive = format!("Goal auto-continue (turn {turn_no} of {max_turns}): keep pursuing \"{objective}\".");
    match runtime.try_queue_synthetic_follow_up_input(follow_up) {
        Ok(()) => {
            Arc::make_mut(&mut runtime.state.messages).push(vtcode_core::llm::provider::Message::system(directive));
            // Queued auto-continue stays quiet like tracker/plan: the next
            // turn starts immediately.
            tracing::debug!(goal_turn = turn_no, max_turns, "Queued goal auto-continue without TUI echo");
            Ok(GoalContinueOutcome::Queued)
        }
        Err(err) => {
            tracing::warn!(%err, "Goal auto-continue queue full; falling through to turn end");
            let _ = renderer.line(
                MessageStyle::Warning,
                "[i] Goal auto-continue could not resume automatically; the goal stays active. Type `continue` to resume it.",
            );
            Ok(GoalContinueOutcome::Idle { exhausted: true })
        }
    }
}

/// Whether the model's final text claims the active goal is achieved: the
/// first non-empty line starts with the exact `GOAL ACHIEVED:` sentinel.
/// Same trust class as the sibling final-text classifiers; a spurious claim
/// completes visibly and the user can re-set in one command.
fn goal_claims_completion(final_text: Option<&str>) -> bool {
    final_text.is_some_and(|text| {
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .is_some_and(|line| line.starts_with("GOAL ACHIEVED:"))
    })
}

fn planning_budget_exhausted(planning_budget_exhausted: bool, blocked_reason: Option<&str>) -> bool {
    planning_budget_exhausted
        || blocked_reason.is_some_and(|reason| {
            crate::agent::runloop::unified::run_loop_context::budget_exhaustion_cause(reason).is_some()
        })
}
#[cfg(test)]
#[path = "corrective_tests.rs"]
mod tests;
