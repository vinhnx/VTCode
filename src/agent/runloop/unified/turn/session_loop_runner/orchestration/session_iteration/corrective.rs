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
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::super::super::support::latest_assistant_result_text;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;

/// Mutable corrective-gate state threaded through auto-continue scheduling.
pub(super) struct CorrectiveGate<'a> {
    pub session_stats: &'a mut SessionStats,
    pub runtime: &'a mut AgentRuntime,
    pub renderer: &'a mut AnsiRenderer,
    pub tool_registry: &'a ToolRegistry,
    pub plan_session: &'a PlanningWorkflowSessionState,
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
            !matches!(outcome_result, RunLoopTurnLoopResult::Cancelled | RunLoopTurnLoopResult::Exit)
                && crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
                && tracker_continue::tracker_auto_continue_enabled(vt_cfg.as_ref())
                && !session_stats.navigation_recovery.blocked()
                && !session_stats.preflight_recovery.exhausted()
                && !session_stats.preflight_recovery.corrective_pending();
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
    Ok(CorrectiveFlow::Proceed { tracker_auto_continue_exhausted })
}

fn planning_budget_exhausted(planning_budget_exhausted: bool, blocked_reason: Option<&str>) -> bool {
    planning_budget_exhausted
        || blocked_reason.is_some_and(|reason| {
            crate::agent::runloop::unified::run_loop_context::budget_exhaustion_cause(reason).is_some()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::core::agent::session::AgentSessionState;

    #[test]
    fn planning_budget_stop_includes_runtime_block_reason() {
        assert!(planning_budget_exhausted(false, Some("Tool-call budget exhausted for this turn (32/32).")));
        assert!(planning_budget_exhausted(false, Some("Tool wall-clock budget exhausted for this turn (600s).")));
        assert!(planning_budget_exhausted(true, None));
        assert!(!planning_budget_exhausted(false, Some("Turn ended with a recovery fallback")));
        assert!(!planning_budget_exhausted(false, None));
    }

    #[tokio::test]
    async fn budget_stop_does_not_queue_a_corrective_follow_up() {
        let workspace = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
        for (reason, should_queue) in [
            (
                "Tool-call budget exhausted for this turn (32/32). Changes remain unverified; no additional verifier was scheduled.",
                false,
            ),
            ("Tool wall-clock budget exhausted for this turn (600s).", false),
            ("Tool loop budget exhausted before a final response.", false),
            (
                "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained.",
                true,
            ),
        ] {
            let mut stats = SessionStats::default();
            let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 4, 16_000), None, None);
            let mut renderer = AnsiRenderer::stdout();
            let plan = PlanningWorkflowSessionState::default();
            let outcome = RunLoopTurnLoopResult::Blocked { reason: Some(reason.into()) };
            let ctrl_c = Arc::new(CtrlCState::new());
            let flow = run_corrective_gate(CorrectiveGate {
                session_stats: &mut stats,
                runtime: &mut runtime,
                renderer: &mut renderer,
                tool_registry: &registry,
                plan_session: &plan,
                vt_cfg: &None,
                outcome_result: &outcome,
                ctrl_c_state: &ctrl_c,
                next_turn_input: "original request",
                turn_refused: false,
                refused_turn_rolled_back: false,
                session_end_reason: SessionEndReason::Completed,
            })
            .await
            .unwrap();
            assert_eq!(runtime.has_pending_follow_up_inputs(), should_queue, "{reason}");
            assert_eq!(matches!(flow, CorrectiveFlow::ContinueLoop), should_queue, "{reason}");
            if !should_queue {
                runtime.try_queue_follow_up_input("continue".into()).unwrap();
                assert_eq!(runtime.run_until_idle().as_deref(), Some("continue"));
            }
        }
    }
}
