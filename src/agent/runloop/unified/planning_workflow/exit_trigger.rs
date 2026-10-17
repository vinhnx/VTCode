use std::sync::Arc;

use tokio::sync::Notify;
use vtcode_config::VTCodeConfig;
use vtcode_core::exec::events::PlanApprovalDecision;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::unified::planning_workflow::{
    PlanApprovalRequestContext, PlanApprovalRoute, PlanApprovalTelemetryContext, PlanArtifactError,
    PlanExecutionContext, PlanExecutionTarget, PlanningFinishReason, PlanningIntent,
    assistant_recently_prompted_implementation, complete_approved_plan_handoff, detect_planning_intent,
    execute_plan_approval, finish_planning_workflow, load_plan_text_for_approval, plan_approval_route,
    plan_repair_directive_for_error, resolve_plan_execution_target,
};
use crate::agent::runloop::unified::planning_workflow_state::{
    PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT, PlanningWorkflowSessionState, short_confirmation_hint,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::context::last_user_message;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::PLAN_MODE_AUTO_CONTINUE_MARKER;

const PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS: &str = "Planning workflow: implementation intent detected from your message. Exiting planning mode and proceeding with execution.";
pub(crate) const PLANNING_WORKFLOW_MISSING_PLAN_SYNTHESIS_DIRECTIVE: &str = "Planning recovery: implementation was requested, but no completed plan draft exists yet, and implementation starts only after a plan is approved. Synthesize one compact `<proposed_plan>` from the repository evidence already gathered, with Summary, numbered steps in the form `Action -> files: [path] -> verify: [command]`, Validation, and short Assumptions. The approval prompt appears once the plan is emitted, so there is no need to ask for approval. Valid `verify:` examples include `cargo nextest run -p vtcode`, `cargo check --locked`, `rg -n 'symbol' src/file.rs`, `sed -n '1,40p' docs/file.md`, and `grep -n 'symbol' src/file.rs`; `run checks` and `git diff --check` are invalid. Every `verify:` command must exit 0 when the step succeeds; absence checks that expect `grep`/`rg` exit 1 (no matches) never satisfy completion and must instead use a linter/verifier that exits 0 (e.g. `npx markdownlint-cli2 README.md`). This pass only synthesizes from gathered evidence, so reply with the plan and no tool calls.";

/// Whether the last user message is a harness-generated plan-mode auto-continue
/// directive rather than a genuine user submission.
///
/// The directive's phrase `do not implement` normalizes to the `STAY_PHRASES`
/// entry `"do not implement"`, so `detect_planning_intent` classifies it as
/// `StayInPlanning`. When the exit trigger consumes such a directive as a
/// user-initiated stay signal, the turn breaks before any LLM request runs, the
/// completed-turn fallback fires, and the outer loop re-queues another
/// identical directive — producing the observed infinite plan-mode loop
/// (checkpoint turn_857). The opening marker is authoritative: real users do
/// not prefix their message with it.
pub(crate) fn is_plan_mode_auto_continue_directive(text: &str) -> bool {
    text.contains(PLAN_MODE_AUTO_CONTINUE_MARKER)
}

pub(crate) struct PlanningExitContext<'a> {
    pub(crate) session: &'a mut InlineSession,
    pub(crate) ctrl_c_state: &'a Arc<CtrlCState>,
    pub(crate) ctrl_c_notify: &'a Arc<Notify>,
    pub(crate) vt_cfg: Option<&'a VTCodeConfig>,
    pub(crate) skip_confirmations: bool,
    pub(crate) full_auto: bool,
    pub(crate) context_usage_percent: u8,
    pub(crate) telemetry: PlanApprovalTelemetryContext<'a>,
}

/// Outcome of checking whether the planning workflow should exit this turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanningTransition {
    /// No planning transition; continue the turn normally.
    None,
    /// User approved the plan; proceed with execution.
    ExitAndImplement { target: PlanExecutionTarget },
    /// User wants to stay in planning mode.
    StayInPlanning,
    /// User abandoned the current plan without starting execution.
    CancelPlanning,
}

impl PlanningTransition {
    /// Convert this transition into the `TurnLoopResult::Completed` variant
    /// and an optional primary-agent switch command.
    #[inline]
    pub(crate) fn into_result_and_target(self) -> (TurnLoopResult, Option<PlanExecutionTarget>) {
        match self {
            PlanningTransition::None => (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None),
            PlanningTransition::ExitAndImplement { target } => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: true }, Some(target))
            }
            PlanningTransition::StayInPlanning => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None)
            }
            PlanningTransition::CancelPlanning => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None)
            }
        }
    }

    /// Whether the turn loop should break after this transition.
    #[inline]
    pub(crate) fn should_break(&self) -> bool {
        !matches!(self, PlanningTransition::None)
    }
}

/// Check whether the last user message signals a planning-workflow exit (approve,
/// implement, switch-to-build/auto) and execute the transition if so.
///
/// Returns the detected transition. The caller checks `should_break()` to decide
/// whether to break the turn loop.
pub(crate) async fn maybe_handle_planning_exit_trigger(
    renderer: &mut AnsiRenderer,
    tool_registry: &mut ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    working_history: &mut Vec<uni::Message>,
    auto_finish_planning_attempted: &mut bool,
    exit_context: PlanningExitContext<'_>,
) -> anyhow::Result<PlanningTransition> {
    if !tool_registry.is_planning_active() {
        return Ok(PlanningTransition::None);
    }

    if *auto_finish_planning_attempted {
        return Ok(PlanningTransition::None);
    }

    let Some(last_user_msg) = last_user_message(working_history) else {
        return Ok(PlanningTransition::None);
    };

    let text = last_user_msg.content.as_text();
    if is_plan_mode_auto_continue_directive(&text) {
        return Ok(PlanningTransition::None);
    }
    let assistant_prompted = assistant_recently_prompted_implementation(working_history);
    let intent = detect_planning_intent(&text, assistant_prompted);

    let transition = match intent {
        PlanningIntent::ExitAndImplement => {
            let plan = match load_plan_text_for_approval(tool_registry).await {
                Ok(plan) => plan,
                Err(PlanArtifactError::Missing) => {
                    display_status(
                        renderer,
                        "No completed plan draft exists yet. I will synthesize the plan from the gathered evidence before showing approval.",
                    )?;
                    // A textual `yes`/`implement` is not an approval when there
                    // is no persisted draft. Keep the user choice attached to
                    // this turn, but continue through one model request so the
                    // plan can be synthesized and then routed to the normal
                    // approval overlay. The turn-local guard prevents the same
                    // user message from re-entering this branch on the next loop
                    // iteration.
                    *auto_finish_planning_attempted = true;
                    working_history
                        .push(uni::Message::system(PLANNING_WORKFLOW_MISSING_PLAN_SYNTHESIS_DIRECTIVE.to_string()));
                    return Ok(PlanningTransition::None);
                }
                Err(error) => {
                    display_status(renderer, &format!("Plan approval is blocked: {error}"))?;
                    tracing::warn!(target: "vtcode.planning_workflow", error = %error, "persisted plan rejected before approval");
                    if plan_session.plan_validation_repair_allowed() {
                        plan_session.mark_plan_validation_repair_used();
                        // The error→feedback mapping and bounded repair policy
                        // live in the planning facade so this later-turn
                        // approval rejection path and the initial-plan
                        // rejection path share identical guidance.
                        working_history.push(uni::Message::system(plan_repair_directive_for_error(&error)));
                    }
                    *auto_finish_planning_attempted = true;
                    return Ok(PlanningTransition::StayInPlanning);
                }
            };

            *auto_finish_planning_attempted = true;

            let require_confirmation = exit_context
                .vt_cfg
                .map(|cfg| cfg.agent.require_plan_confirmation)
                .unwrap_or(true);
            let approval_route = plan_approval_route(
                require_confirmation,
                renderer.supports_inline_ui(),
                exit_context.skip_confirmations,
                exit_context.full_auto,
            );
            tracing::info!(
                target: "vtcode.planning_workflow",
                ?approval_route,
                "textual plan approval requested"
            );

            if approval_route == PlanApprovalRoute::Inline {
                // Resolve owned copies before the mutable `tool_registry`
                // borrow for the approval call begins.
                let approval_editor = exit_context.vt_cfg.map(|cfg| cfg.tools.editor.clone()).unwrap_or_default();
                let approval_workspace_root = tool_registry.workspace_root().clone();
                let outcome = execute_plan_approval(
                    tool_registry,
                    plan_session,
                    handle,
                    exit_context.session,
                    exit_context.ctrl_c_state,
                    exit_context.ctrl_c_notify,
                    PlanApprovalRequestContext {
                        plan: &plan,
                        skip_confirmations: exit_context.skip_confirmations,
                        full_auto: exit_context.full_auto,
                        context_usage_percent: exit_context.context_usage_percent,
                        editor: approval_editor,
                        workspace_root: approval_workspace_root,
                    },
                    exit_context.telemetry,
                )
                .await?;

                return Ok(match outcome {
                    TurnHandlerOutcome::SwitchPrimaryAgent(_execution_agent) => PlanningTransition::ExitAndImplement {
                        target: PlanExecutionTarget::build(
                            PlanExecutionContext::Current,
                            exit_context.skip_confirmations,
                        ),
                    },
                    TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target } => {
                        PlanningTransition::ExitAndImplement { target }
                    }
                    TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: true }) => {
                        PlanningTransition::ExitAndImplement {
                            target: PlanExecutionTarget::build(
                                PlanExecutionContext::Current,
                                exit_context.skip_confirmations,
                            ),
                        }
                    }
                    TurnHandlerOutcome::BreakWithPolicy {
                        result: TurnLoopResult::Completed { plan_approved_execution_pending: true },
                        target,
                    } => PlanningTransition::ExitAndImplement { target },
                    TurnHandlerOutcome::Break(_) | TurnHandlerOutcome::Continue => PlanningTransition::StayInPlanning,
                    TurnHandlerOutcome::BreakWithPolicy { .. } => PlanningTransition::StayInPlanning,
                });
            }

            display_status(renderer, PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS)?;
            let decision = if approval_route == PlanApprovalRoute::Automatic {
                PlanApprovalDecision::AutoAccept
            } else {
                PlanApprovalDecision::Execute
            };
            let skip_confirmations = approval_route == PlanApprovalRoute::Automatic;
            let target = resolve_plan_execution_target(
                decision,
                PlanExecutionContext::Current,
                skip_confirmations,
                exit_context.full_auto,
            );
            let handoff = complete_approved_plan_handoff(tool_registry, plan_session, handle, plan, target).await;
            let handoff = match handoff {
                Ok(handoff) => handoff,
                Err(error) => {
                    display_status(renderer, &format!("Plan execution is blocked: {error}"))?;
                    tracing::warn!(target: "vtcode.planning_workflow", error = %error, "textual approved-plan handoff blocked");
                    return Ok(PlanningTransition::StayInPlanning);
                }
            };
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                decision,
                approval_route == PlanApprovalRoute::Automatic,
            );
            PlanningTransition::ExitAndImplement { target: handoff.target }
        }
        PlanningIntent::StayInPlanning => {
            let hint = if load_plan_text_for_approval(tool_registry).await.is_ok() {
                short_confirmation_hint()
            } else {
                PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT
            };
            display_status(renderer, hint)?;
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                PlanApprovalDecision::Revise,
                false,
            );
            PlanningTransition::StayInPlanning
        }
        PlanningIntent::CancelPlanning => {
            display_status(renderer, "Planning workflow cancelled; the plan was not implemented.")?;
            super::resolve_plan_approval(
                plan_session,
                exit_context.telemetry.emitter,
                exit_context.telemetry.thread_id,
                exit_context.telemetry.turn_id,
                PlanApprovalDecision::Cancel,
                false,
            );
            let removed = super::clear_stale_recovery_directives_for_execution(working_history);
            if removed > 0 {
                tracing::info!(removed, "Cleared stale recovery directives when planning was cancelled");
            }
            finish_planning_workflow(tool_registry, plan_session, handle, PlanningFinishReason::Cancelled).await?;
            PlanningTransition::CancelPlanning
        }
        PlanningIntent::None => PlanningTransition::None,
    };

    Ok(transition)
}

fn display_status(renderer: &mut AnsiRenderer, message: &str) -> anyhow::Result<()> {
    renderer.line(vtcode_core::utils::ansi::MessageStyle::Status, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_plan_synthesis_directive_keeps_inspection_verify_examples() {
        let text = PLANNING_WORKFLOW_MISSING_PLAN_SYNTHESIS_DIRECTIVE;
        assert!(text.contains("sed -n") && text.contains("grep -n"), "missing inspection examples: {text}");
        assert!(text.contains("git diff --check"), "missing invalid VCS example: {text}");
        assert!(
            text.contains("must exit 0") && text.contains("npx markdownlint-cli2"),
            "verify must require exit 0 with linter fallback: {text}"
        );
    }

    #[test]
    fn approved_plan_transition_preserves_target_policy_for_handoff() {
        let (result, target) = PlanningTransition::ExitAndImplement {
            target: PlanExecutionTarget::auto(PlanExecutionContext::Current),
        }
        .into_result_and_target();

        assert!(matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: true }));
        assert_eq!(target, Some(PlanExecutionTarget::auto(PlanExecutionContext::Current)));
    }

    #[test]
    fn manual_plan_transition_keeps_confirmation_prompts_without_agent_switch() {
        let (result, target) = PlanningTransition::ExitAndImplement {
            target: PlanExecutionTarget::build(PlanExecutionContext::Current, false),
        }
        .into_result_and_target();

        assert!(matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: true }));
        assert_eq!(target, Some(PlanExecutionTarget::build(PlanExecutionContext::Current, false)));
    }

    #[test]
    fn plan_mode_auto_continue_directive_is_not_a_user_stay_intent() {
        let directive = crate::agent::runloop::unified::turn::tool_outcomes::helpers::plan_mode_continue_follow_up();
        assert!(is_plan_mode_auto_continue_directive(&directive));

        let normalized = vtcode_core::planning::normalize_plan_intent(&directive);
        assert!(
            vtcode_core::planning::matches_stay_intent(&normalized),
            "auto-continue text must collide with the stay phrase to prove the guard matters"
        );
    }

    #[test]
    fn genuine_user_text_is_not_flagged_as_auto_continue() {
        assert!(!is_plan_mode_auto_continue_directive("continue planning"));
        assert!(!is_plan_mode_auto_continue_directive("yes, implement the plan"));
        assert!(!is_plan_mode_auto_continue_directive("keep researching the plan"));
    }
}
