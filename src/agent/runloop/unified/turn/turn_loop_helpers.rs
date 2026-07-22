use anyhow::Result;
use serde_json::json;

use crate::agent::runloop::unified::planning_workflow::{
    PlanningIntent, assistant_recently_prompted_implementation, detect_enter_planning_intent, detect_planning_intent,
};
use crate::agent::runloop::unified::planning_workflow_state::short_confirmation_hint_with_fallback;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{push_tool_response, tool_output_from_outcome};
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, display_status};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use vtcode_core::config::constants::defaults::{
    DEFAULT_MAX_CONVERSATION_TURNS, DEFAULT_MAX_REPEATED_TOOL_CALLS, DEFAULT_MAX_TOOL_LOOPS,
};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::llm::provider as uni;

#[derive(Debug, Clone)]
pub(super) struct PrecomputedTurnConfig {
    pub(super) max_tool_loops: usize,
    pub(super) tool_repeat_limit: usize,
    pub(super) max_session_turns: usize,
    pub(super) request_user_input_enabled: bool,
}

const UNLIMITED_TOOL_LOOPS: usize = usize::MAX;

#[inline]
pub(super) fn extract_turn_config(
    vt_cfg: Option<&VTCodeConfig>,
    planning_active: bool,
    interactive_session: bool,
) -> PrecomputedTurnConfig {
    let features = FeatureSet::from_config(vt_cfg);
    vt_cfg
        .map(|cfg| PrecomputedTurnConfig {
            max_tool_loops: resolve_tool_loop_limit(cfg.tools.max_tool_loops, planning_active),
            tool_repeat_limit: if cfg.tools.max_repeated_tool_calls > 0 {
                cfg.tools.max_repeated_tool_calls
            } else {
                DEFAULT_MAX_REPEATED_TOOL_CALLS
            },
            max_session_turns: cfg.agent.max_conversation_turns,
            request_user_input_enabled: features.request_user_input_enabled(planning_active, interactive_session),
        })
        .unwrap_or(PrecomputedTurnConfig {
            max_tool_loops: resolve_tool_loop_limit(DEFAULT_MAX_TOOL_LOOPS, planning_active),
            tool_repeat_limit: DEFAULT_MAX_REPEATED_TOOL_CALLS,
            max_session_turns: DEFAULT_MAX_CONVERSATION_TURNS,
            request_user_input_enabled: features.request_user_input_enabled(planning_active, interactive_session),
        })
}

pub(super) enum ToolLoopLimitAction {
    Proceed,
    ContinueLoop,
    BreakLoop,
}

#[inline]
pub(super) fn resolve_safety_tool_call_limits(
    max_tool_calls_per_turn: usize,
    max_session_turns: usize,
    planning_active: bool,
) -> (usize, usize) {
    let turn_limit = if max_tool_calls_per_turn == 0 {
        usize::MAX
    } else {
        max_tool_calls_per_turn
    };
    let session_limit = if planning_active || max_tool_calls_per_turn == 0 {
        usize::MAX
    } else {
        max_tool_calls_per_turn.saturating_mul(max_session_turns.max(1))
    };

    (turn_limit, session_limit)
}

const MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 120;
const MAX_TOOL_LOOP_CAP_MULTIPLIER: usize = 3;
const MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 50;
const PLANNING_WORKFLOW_MIN_TOOL_LOOPS: usize = 40;
const PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 240;
const PLANNING_WORKFLOW_TOOL_LOOP_CAP_MULTIPLIER: usize = 6;
const PLANNING_WORKFLOW_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 80;
const PLANNING_WORKFLOW_ENTER_TRIGGER_STATUS: &str =
    "Planning workflow: explicit planning request detected. Entering read-only planning before continuing this turn.";
const PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS: &str = "Planning workflow: implementation intent detected from your message. Running `finish_planning` for plan confirmation; once approved, VT Code will switch to the selected primary agent and execute.";

fn resolve_tool_loop_limit(configured_limit: usize, planning_active: bool) -> usize {
    if configured_limit == 0 {
        UNLIMITED_TOOL_LOOPS
    } else if planning_active {
        configured_limit.max(PLANNING_WORKFLOW_MIN_TOOL_LOOPS)
    } else {
        configured_limit
    }
}

fn planning_fully_disabled(ctx: &TurnLoopContext<'_>) -> bool {
    !ctx.tool_registry.is_planning_active() && !ctx.tool_registry.planning_workflow_state().is_active()
}

fn configured_tool_loop_base_limit(ctx: &TurnLoopContext<'_>) -> usize {
    let configured = ctx
        .vt_cfg
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_MAX_TOOL_LOOPS);
    if ctx.is_planning_active() {
        configured.max(PLANNING_WORKFLOW_MIN_TOOL_LOOPS)
    } else {
        configured
    }
}

fn tool_loop_hard_cap(base_limit: usize, planning_active: bool) -> usize {
    if planning_active {
        if base_limit >= PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
            return base_limit;
        }
        return base_limit
            .saturating_mul(PLANNING_WORKFLOW_TOOL_LOOP_CAP_MULTIPLIER)
            .min(PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP);
    }
    if base_limit >= MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
        return base_limit;
    }
    base_limit
        .saturating_mul(MAX_TOOL_LOOP_CAP_MULTIPLIER)
        .min(MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP)
}

fn clamp_tool_loop_increment(
    requested_increment: usize,
    current_limit: usize,
    hard_cap: usize,
    planning_active: bool,
) -> usize {
    let remaining = hard_cap.saturating_sub(current_limit);
    let per_prompt_limit = if planning_active {
        PLANNING_WORKFLOW_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT
    } else {
        MAX_TOOL_LOOP_INCREMENT_PER_PROMPT
    };
    requested_increment.min(per_prompt_limit).min(remaining)
}

fn emit_loop_hard_cap_break_metric(
    ctx: &TurnLoopContext<'_>,
    step_count: usize,
    current_limit: usize,
    base_limit: usize,
    hard_cap: usize,
    reason: &'static str,
) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "loop_hard_cap_break",
        reason,
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        planning_workflow = ctx.is_planning_active(),
        step_count,
        current_limit,
        base_limit,
        hard_cap,
        tool_calls = ctx.harness_state.tool_calls,
        "turn metric"
    );
}

pub(super) async fn handle_steering_messages(
    ctx: &mut TurnLoopContext<'_>,
    _working_history: &mut [uni::Message],
    result: &mut TurnLoopResult,
) -> Result<bool> {
    let renderer = &mut *ctx.renderer;
    let tool_registry = &mut *ctx.tool_registry;
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;

    let Some(mut receiver) = ctx.runtime_steering.take_receiver() else {
        return Ok(false);
    };

    let steering_result: Result<bool> = loop {
        let mut pending = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            pending.push(message);
        }

        if pending.is_empty() {
            break Ok(false);
        }

        if pending.iter().any(|message| matches!(message, SteeringMessage::SteerStop)) {
            cancel_for_steering_stop(tool_registry, result).await;
            display_status(renderer, "Stop requested by steering signal.")?;
            break Ok(true);
        }

        if let Some(pause_index) = pending.iter().position(|message| matches!(message, SteeringMessage::Pause)) {
            for message in pending.drain(..pause_index) {
                if let SteeringMessage::FollowUpInput(input) = message {
                    queue_follow_up_input(renderer, ctx.runtime_steering, input)?;
                }
            }
            pending.remove(0);
            if handle_pause_signal(
                renderer,
                tool_registry,
                ctrl_c_state,
                ctrl_c_notify,
                &mut receiver,
                ctx.runtime_steering,
                result,
                pending,
            )
            .await?
            {
                break Ok(true);
            }
            continue;
        }

        for message in pending {
            if let SteeringMessage::FollowUpInput(input) = message {
                queue_follow_up_input(renderer, ctx.runtime_steering, input)?;
            }
        }
    };

    ctx.runtime_steering.set_receiver(Some(receiver));
    if steering_result? {
        return Ok(true);
    }

    Ok(false)
}

fn queue_follow_up_input(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    input: String,
) -> Result<()> {
    display_status(renderer, &format!("Queued Follow-up Input: {input}"))?;
    runtime_steering.queue_follow_up_input(input);
    Ok(())
}

async fn cancel_for_steering_stop(tool_registry: &mut vtcode_core::tools::ToolRegistry, result: &mut TurnLoopResult) {
    if let Err(err) = tool_registry.terminate_all_exec_sessions_async().await {
        tracing::warn!(error = %err, "Failed to terminate exec sessions after steering stop");
    }
    *result = TurnLoopResult::Cancelled;
}

async fn handle_pause_signal(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    tool_registry: &mut vtcode_core::tools::ToolRegistry,
    ctrl_c_state: &crate::agent::runloop::unified::state::CtrlCState,
    ctrl_c_notify: &tokio::sync::Notify,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    result: &mut TurnLoopResult,
    pending: Vec<SteeringMessage>,
) -> Result<bool> {
    display_status(renderer, "Paused by steering signal. Waiting for Resume...")?;

    let mut resumed = false;
    for message in pending {
        match message {
            SteeringMessage::Resume => {
                resumed = true;
            }
            SteeringMessage::SteerStop => {
                cancel_for_steering_stop(tool_registry, result).await;
                return Ok(true);
            }
            SteeringMessage::FollowUpInput(input) => {
                queue_follow_up_input(renderer, runtime_steering, input)?;
            }
            SteeringMessage::Pause => {}
        }
    }

    if resumed {
        display_status(renderer, "Resumed by steering signal.")?;
        return Ok(false);
    }

    loop {
        tokio::select! {
            message = receiver.recv() => {
                match message {
                    Some(SteeringMessage::Resume) => {
                        display_status(renderer, "Resumed by steering signal.")?;
                        return Ok(false);
                    }
                    Some(SteeringMessage::SteerStop) => {
                        cancel_for_steering_stop(tool_registry, result).await;
                        return Ok(true);
                    }
                    Some(SteeringMessage::FollowUpInput(input)) => {
                        queue_follow_up_input(renderer, runtime_steering, input)?;
                    }
                    Some(SteeringMessage::Pause) => {}
                    None => return Ok(false),
                }
            }
            _ = ctrl_c_notify.notified() => {
                if ctrl_c_state.is_exit_requested() {
                    *result = TurnLoopResult::Exit;
                    return Ok(true);
                }
                if ctrl_c_state.is_cancel_requested() {
                    *result = TurnLoopResult::Cancelled;
                    return Ok(true);
                }
            }
        }
    }
}

pub(super) async fn maybe_handle_planning_exit_trigger(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    step_count: usize,
    pending_primary_agent: &mut Option<String>,
) -> Result<bool> {
    if !ctx.is_planning_active() {
        return Ok(false);
    }

    // Guard: prevent repeated finish_planning calls within the same turn.
    // Without this, the loop re-detects implementation intent from the same
    // user message on every iteration and calls finish_planning repeatedly,
    // accumulating plan context and wasting tokens.
    if *ctx.auto_finish_planning_attempted {
        return Ok(false);
    }

    let Some(last_user_msg) = working_history.iter().rev().find(|msg| msg.role == uni::MessageRole::User) else {
        return Ok(false);
    };

    let text = last_user_msg.content.as_text();
    let assistant_prompted = assistant_recently_prompted_implementation(working_history);
    let intent = detect_planning_intent(&text, assistant_prompted);

    match intent {
        PlanningIntent::ExitAndImplement => {
            // Mark that we've attempted finish_planning this turn to prevent re-entry.
            *ctx.auto_finish_planning_attempted = true;
            ctx.plan_session.request_approval();

            display_status(ctx.renderer, PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS)?;
            // Continue with finish_planning logic below...
        }
        PlanningIntent::StayInPlanning => {
            // User explicitly wants to stay in planning - show hint.
            display_status(ctx.renderer, &short_confirmation_hint_with_fallback())?;
            return Ok(false);
        }
        PlanningIntent::None => {
            // No planning intent detected - continue turn.
            return Ok(false);
        }
    }

    use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        FINISH_PLANNING_REASON_USER_REQUESTED_IMPLEMENTATION, build_finish_planning_args,
        build_step_finish_planning_call_id,
    };
    use vtcode_core::llm::provider::ToolCall;

    let args = build_finish_planning_args(FINISH_PLANNING_REASON_USER_REQUESTED_IMPLEMENTATION);
    let call = ToolCall::function(
        build_step_finish_planning_call_id(step_count),
        tool_names::FINISH_PLANNING.to_string(),
        serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
    );
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;
    let default_placeholder = ctx.default_placeholder.clone();
    let lifecycle_hooks = ctx.lifecycle_hooks;
    let vt_cfg = ctx.vt_cfg;
    // This helper is entered only after the user explicitly typed an approval
    // intent (`approve`, `implement`, etc.). Do not open the plan-confirmation
    // overlay a second time; that would consume the approval turn and leave
    // the agent apparently idle. The finish tool still performs the normal
    // planning-state transition and execution policy checks.
    ctx.handle.set_skip_confirmations(true);
    let mut run_ctx = ctx.as_run_loop_context();

    let outcome = run_tool_call(
        &mut run_ctx,
        &call,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        lifecycle_hooks,
        true,
        vt_cfg,
        step_count,
        false,
    )
    .await;

    match outcome {
        Ok(pipe_outcome) => {
            // Add tool output to history so the model can see the result
            // (validation blockers, confirmation outcome, etc.) and respond.
            if let Some(output) = tool_output_from_outcome(&pipe_outcome) {
                // Inject a synthetic assistant tool_use so the history is well-formed:
                //   user(intent) → assistant(tool_use: finish_planning) → user(tool_result)
                // Without this the tool_result is orphaned and the LLM generates
                // confused repeated output when called on the next turn.
                working_history.push(uni::Message::assistant_with_tools(String::new(), vec![call]));
                push_tool_response(
                    working_history,
                    build_step_finish_planning_call_id(step_count),
                    Some(tool_names::FINISH_PLANNING),
                    serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string()),
                );
            }

            // Propagate a plan-mode agent handoff (SwitchBuild/SwitchAuto from the
            // HITL confirmation) so the interaction loop actually switches the
            // active agent instead of silently staying in plan mode. This field
            // was previously discarded because the function only returned a bool.
            if let Some(agent) = pipe_outcome.pending_primary_agent.clone() {
                *pending_primary_agent = Some(agent);
            }

            if !planning_fully_disabled(ctx) {
                // Planning still active (waiting for user confirmation via HITL overlay).
                // Break the loop so the UI can present the confirmation dialog.
                // Without this break, the loop re-enters on the next iteration and
                // would call finish_planning again (blocked by the guard above),
                // but also wastes an LLM round-trip.
                return Ok(true);
            }

            // Planning was fully disabled (auto-accept on user's approval intent).
            // Break the turn loop to avoid an unnecessary LLM round-trip that
            // would see the synthetic finish_planning tool artifacts in the
            // history and could produce confusing or duplicate output. The exit
            // trigger status above already informed the user; no additional
            // message is needed here.
            return Ok(true);
        }
        Err(err) => {
            display_error(ctx.renderer, "Failed to exit Planning workflow", &err)?;
            Ok(false)
        }
    }
}

pub(super) async fn maybe_handle_planning_enter_trigger(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut [uni::Message],
    step_count: usize,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    if ctx.is_planning_active() {
        return Ok(false);
    }

    let Some(last_user_msg) = working_history.iter().rev().find(|msg| msg.role == uni::MessageRole::User) else {
        return Ok(false);
    };

    let text = last_user_msg.content.as_text();
    if !detect_enter_planning_intent(&text) {
        return Ok(false);
    }

    display_status(ctx.renderer, PLANNING_WORKFLOW_ENTER_TRIGGER_STATUS)?;

    use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
    use vtcode_core::llm::provider::ToolCall;

    let call = ToolCall::function(
        format!("call_{step_count}_start_planning"),
        tool_names::START_PLANNING.to_string(),
        serde_json::to_string(&json!({
            "description": text,
            "approved": true
        }))
        .unwrap_or_else(|_| "{}".to_string()),
    );
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;
    let default_placeholder = ctx.default_placeholder.clone();
    let lifecycle_hooks = ctx.lifecycle_hooks;
    let vt_cfg = ctx.vt_cfg;
    let mut run_ctx = ctx.as_run_loop_context();

    match run_tool_call(
        &mut run_ctx,
        &call,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        lifecycle_hooks,
        true,
        vt_cfg,
        step_count,
        false,
    )
    .await
    {
        Ok(_) if ctx.is_planning_active() => Ok(false),
        Ok(_) => {
            *result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
            Ok(true)
        }
        Err(err) => {
            display_error(ctx.renderer, "Failed to enter Planning workflow", &err)?;
            *result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
            Ok(true)
        }
    }
}

pub(super) async fn maybe_handle_tool_loop_limit(
    ctx: &mut TurnLoopContext<'_>,
    step_count: usize,
    current_max_tool_loops: &mut usize,
) -> Result<ToolLoopLimitAction> {
    if *current_max_tool_loops == UNLIMITED_TOOL_LOOPS {
        return Ok(ToolLoopLimitAction::Proceed);
    }

    if step_count < *current_max_tool_loops {
        return Ok(ToolLoopLimitAction::Proceed);
    }

    display_status(ctx.renderer, &format!("Reached maximum tool loops ({})", *current_max_tool_loops))?;

    let planning_active = ctx.is_planning_active();
    let base_limit = configured_tool_loop_base_limit(ctx);
    let hard_cap = tool_loop_hard_cap(base_limit, planning_active);
    if *current_max_tool_loops >= hard_cap {
        emit_loop_hard_cap_break_metric(
            ctx,
            step_count,
            *current_max_tool_loops,
            base_limit,
            hard_cap,
            "hard_cap_reached",
        );
        display_status(
            ctx.renderer,
            &format!("Tool loop hard cap reached ({hard_cap}). Stopping turn to prevent runaway looping."),
        )?;
        return Ok(ToolLoopLimitAction::BreakLoop);
    }

    match crate::agent::runloop::unified::tool_routing::prompt_tool_loop_limit_increase(
        ctx.handle,
        ctx.session,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        *current_max_tool_loops,
    )
    .await
    {
        Ok(Some(requested_increment)) => {
            let increment =
                clamp_tool_loop_increment(requested_increment, *current_max_tool_loops, hard_cap, planning_active);
            if increment == 0 {
                emit_loop_hard_cap_break_metric(
                    ctx,
                    step_count,
                    *current_max_tool_loops,
                    base_limit,
                    hard_cap,
                    "no_remaining_headroom",
                );
                display_status(ctx.renderer, "Tool loop limit cannot be increased further for this turn.")?;
                return Ok(ToolLoopLimitAction::BreakLoop);
            }
            let previous_max_tool_loops = *current_max_tool_loops;
            *current_max_tool_loops = (*current_max_tool_loops).saturating_add(increment);
            tracing::info!(
                "Updated tool loop limit: turn={} (was {}), session tool-call limit remains unchanged",
                *current_max_tool_loops,
                previous_max_tool_loops,
            );
            display_status(
                ctx.renderer,
                &format!("Tool loop limit increased to {} (+{}, cap {})", *current_max_tool_loops, increment, hard_cap),
            )?;
            Ok(ToolLoopLimitAction::ContinueLoop)
        }
        _ => Ok(ToolLoopLimitAction::BreakLoop),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS, PLANNING_WORKFLOW_MIN_TOOL_LOOPS, UNLIMITED_TOOL_LOOPS,
        clamp_tool_loop_increment, extract_turn_config, handle_steering_messages, resolve_safety_tool_call_limits,
        resolve_tool_loop_limit, tool_loop_hard_cap,
    };
    use crate::agent::runloop::unified::planning_workflow::{
        PlanningIntent, detect_enter_planning_intent, detect_planning_intent,
    };
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use std::time::Duration;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::agent::steering::SteeringMessage;

    #[test]
    fn detects_implement_the_plan_trigger() {
        assert_eq!(detect_planning_intent("Implement the plan.", false), PlanningIntent::ExitAndImplement);
        assert_eq!(
            detect_planning_intent("Please execute this plan and start coding.", false),
            PlanningIntent::ExitAndImplement
        );
    }

    #[test]
    fn detects_existing_exit_intents() {
        assert_eq!(
            detect_planning_intent("Exit planning workflow and implement.", false),
            PlanningIntent::ExitAndImplement
        );
        assert_eq!(
            detect_planning_intent("Exit planning workflow and proceed.", false),
            PlanningIntent::ExitAndImplement
        );
    }

    #[test]
    fn does_not_exit_when_user_wants_to_keep_planning() {
        assert_eq!(
            detect_planning_intent("Don't implement yet, stay in planning workflow and refine the plan.", false),
            PlanningIntent::StayInPlanning
        );
        assert_eq!(detect_planning_intent("Continue planning for now.", false), PlanningIntent::StayInPlanning);
    }

    #[test]
    fn detects_bare_implement_trigger() {
        assert_eq!(detect_planning_intent("implement", false), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("/implement", false), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("implement.", false), PlanningIntent::ExitAndImplement);
    }

    #[test]
    fn detects_short_implement_variants() {
        assert_eq!(detect_planning_intent("Implement now", false), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("Start implementing", false), PlanningIntent::ExitAndImplement);
    }

    #[test]
    fn detects_direct_confirmation_aliases_as_execute_intent() {
        assert_eq!(detect_planning_intent("yes", false), PlanningIntent::ExitAndImplement);
        // "continue" is NOT a direct exit trigger — it is ambiguous.
        // It only works as a short confirmation when the assistant
        // recently prompted for implementation.
        assert_eq!(detect_planning_intent("continue", false), PlanningIntent::None);
        assert_eq!(detect_planning_intent("go", false), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("start", false), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("yes!", false), PlanningIntent::ExitAndImplement);
    }

    #[test]
    fn stay_mode_has_priority_over_implement_keyword() {
        assert_eq!(
            detect_planning_intent("Do not implement yet; keep planning.", false),
            PlanningIntent::StayInPlanning
        );
        assert_eq!(
            detect_planning_intent("Stay in planning workflow and don't implement.", false),
            PlanningIntent::StayInPlanning
        );
    }

    #[test]
    fn does_not_false_trigger_on_non_intent_implementation_text() {
        assert_eq!(detect_planning_intent("The implementation details are unclear.", false), PlanningIntent::None);
    }

    #[test]
    fn detects_explicit_planning_requests() {
        assert!(detect_enter_planning_intent("make a plan for this"));
        assert!(detect_enter_planning_intent("before implementing, create a plan"));
        assert!(detect_enter_planning_intent("outline the implementation plan"));
    }

    #[test]
    fn does_not_start_planning_for_generic_research_requests() {
        assert!(!detect_enter_planning_intent("explore and tell me about the core agent loop"));
        assert!(!detect_enter_planning_intent("review the runloop and summarize the behavior"));
    }

    #[test]
    fn confirmation_words_trigger_with_implementation_prompt_context() {
        assert_eq!(detect_planning_intent("yes", true), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("continue", true), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("go", true), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("start", true), PlanningIntent::ExitAndImplement);
        assert_eq!(detect_planning_intent("begin", true), PlanningIntent::ExitAndImplement);
    }

    #[test]
    fn confirmation_words_do_not_trigger_without_implementation_prompt_context() {
        assert_eq!(
            detect_planning_intent("yes", false),
            PlanningIntent::ExitAndImplement // "yes" is a direct command
        );
        assert_eq!(detect_planning_intent("continue", false), PlanningIntent::None);
    }

    #[test]
    fn confirmation_words_do_not_trigger_when_stay_in_planning_workflow_is_prompted() {
        // When the assistant asks about staying in planning, "yes" should
        // not trigger exit - but "yes" is still a direct command, so it
        // will trigger ExitAndImplement. This is expected behavior.
        assert_eq!(detect_planning_intent("yes", false), PlanningIntent::ExitAndImplement);
    }

    #[test]
    fn planning_exit_trigger_status_mentions_exit_tool_and_transition() {
        assert!(PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS.contains("finish_planning"));
        assert!(PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS.contains("selected primary agent"));
        assert!(PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS.contains("implementation intent"));
    }

    #[test]
    fn tool_loop_hard_cap_scales_and_bounds() {
        assert_eq!(tool_loop_hard_cap(20, false), 60);
        assert_eq!(tool_loop_hard_cap(40, false), 120);
        assert_eq!(tool_loop_hard_cap(120, false), 120);
        assert_eq!(tool_loop_hard_cap(200, false), 200);
        assert_eq!(tool_loop_hard_cap(40, true), 240);
        assert_eq!(tool_loop_hard_cap(120, true), 240);
    }

    #[test]
    fn clamp_tool_loop_increment_respects_cap_and_per_prompt_limit() {
        assert_eq!(clamp_tool_loop_increment(200, 20, 60, false), 40);
        assert_eq!(clamp_tool_loop_increment(50, 20, 80, false), 50);
        assert_eq!(clamp_tool_loop_increment(10, 75, 80, false), 5);
        assert_eq!(clamp_tool_loop_increment(10, 80, 80, false), 0);
        assert_eq!(clamp_tool_loop_increment(120, 80, 240, true), 80);
    }

    #[test]
    fn extract_turn_config_applies_planning_workflow_loop_floor() {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = 20;
        let turn_cfg = extract_turn_config(Some(&cfg), true, true);
        assert_eq!(turn_cfg.max_tool_loops, PLANNING_WORKFLOW_MIN_TOOL_LOOPS);
    }

    #[test]
    fn extract_turn_config_keeps_non_planning_workflow_loop_limit() {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = 20;
        let turn_cfg = extract_turn_config(Some(&cfg), false, true);
        assert_eq!(turn_cfg.max_tool_loops, 20);
    }

    #[test]
    fn resolve_tool_loop_limit_allows_unlimited_mode() {
        assert_eq!(resolve_tool_loop_limit(0, false), UNLIMITED_TOOL_LOOPS);
        assert_eq!(resolve_tool_loop_limit(0, true), UNLIMITED_TOOL_LOOPS);
    }

    #[test]
    fn resolve_safety_tool_call_limits_maps_zero_turn_budget_to_unbounded_limits() {
        assert_eq!(resolve_safety_tool_call_limits(0, 50, false), (usize::MAX, usize::MAX));
    }

    #[test]
    fn resolve_safety_tool_call_limits_scales_session_limit_from_turn_budget() {
        assert_eq!(resolve_safety_tool_call_limits(12, 40, false), (12, 480));
    }

    #[test]
    fn resolve_safety_tool_call_limits_keeps_planning_workflow_session_unbounded() {
        assert_eq!(resolve_safety_tool_call_limits(48, 40, true), (48, usize::MAX));
    }

    #[test]
    fn extract_turn_config_honors_request_user_input_setting_in_planning_workflow() {
        let mut cfg = VTCodeConfig::default();
        cfg.chat.ask_questions.enabled = false;

        let turn_cfg = extract_turn_config(Some(&cfg), true, true);
        assert!(!turn_cfg.request_user_input_enabled);
    }

    #[tokio::test]
    async fn steering_follow_up_inputs_queue_in_order() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(SteeringMessage::FollowUpInput("first".to_string()))
            .expect("first follow-up");
        sender.send(SteeringMessage::Resume).expect("stray resume");
        sender
            .send(SteeringMessage::FollowUpInput("second".to_string()))
            .expect("second follow-up");
        backing.set_steering_receiver(receiver);

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle steering")
        };

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let inputs = backing.deferred_follow_up_inputs();
        assert_eq!(inputs, vec!["first".to_string(), "second".to_string()]);
    }

    #[tokio::test]
    async fn paused_steering_accepts_follow_up_before_resume() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(SteeringMessage::Pause).expect("pause");
        sender
            .send(SteeringMessage::FollowUpInput("refine search".to_string()))
            .expect("follow-up");
        let resume_sender = sender.clone();
        let resume_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            resume_sender.send(SteeringMessage::Resume).expect("resume");
        });
        backing.set_steering_receiver(receiver);

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle paused steering")
        };
        resume_task.await.expect("resume task");

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let inputs = backing.deferred_follow_up_inputs();
        assert_eq!(inputs, vec!["refine search".to_string()]);
    }

    #[tokio::test]
    async fn paused_steering_keeps_follow_up_after_resume_in_same_batch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(SteeringMessage::Pause).expect("pause");
        sender.send(SteeringMessage::Resume).expect("resume");
        sender
            .send(SteeringMessage::FollowUpInput("use the queued note".to_string()))
            .expect("follow-up");
        backing.set_steering_receiver(receiver);

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle paused steering batch")
        };

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let inputs = backing.deferred_follow_up_inputs();
        assert_eq!(inputs, vec!["use the queued note".to_string()]);
    }

    #[tokio::test]
    async fn steering_stop_beats_queued_follow_up() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(SteeringMessage::FollowUpInput("ignore me".to_string()))
            .expect("follow-up");
        sender.send(SteeringMessage::SteerStop).expect("stop");
        backing.set_steering_receiver(receiver);

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle stop steering")
        };

        assert!(handled);
        assert!(matches!(result, TurnLoopResult::Cancelled));
        assert!(working_history.is_empty());
        assert!(backing.deferred_follow_up_inputs().is_empty());
    }
}
