use anyhow::Result;
use serde_json::json;

use crate::agent::runloop::unified::plan_mode_state::{
    plan_mode_still_active_hint_with_fallback, short_confirmation_hint_with_fallback,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, display_status};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use tokio::sync::mpsc::error::TryRecvError;
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
    plan_mode_active: bool,
) -> PrecomputedTurnConfig {
    let features = FeatureSet::from_config(vt_cfg);
    vt_cfg
        .map(|cfg| PrecomputedTurnConfig {
            max_tool_loops: resolve_tool_loop_limit(cfg.tools.max_tool_loops, plan_mode_active),
            tool_repeat_limit: if cfg.tools.max_repeated_tool_calls > 0 {
                cfg.tools.max_repeated_tool_calls
            } else {
                DEFAULT_MAX_REPEATED_TOOL_CALLS
            },
            max_session_turns: cfg.agent.max_conversation_turns,
            request_user_input_enabled: features.request_user_input_enabled(plan_mode_active, true),
        })
        .unwrap_or(PrecomputedTurnConfig {
            max_tool_loops: resolve_tool_loop_limit(DEFAULT_MAX_TOOL_LOOPS, plan_mode_active),
            tool_repeat_limit: DEFAULT_MAX_REPEATED_TOOL_CALLS,
            max_session_turns: DEFAULT_MAX_CONVERSATION_TURNS,
            request_user_input_enabled: features.request_user_input_enabled(plan_mode_active, true),
        })
}

pub(super) enum ToolLoopLimitAction {
    Proceed,
    ContinueLoop,
    BreakLoop,
}

const MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 120;
const MAX_TOOL_LOOP_CAP_MULTIPLIER: usize = 3;
const MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 50;
const PLAN_MODE_MIN_TOOL_LOOPS: usize = 40;
const PLAN_MODE_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 240;
const PLAN_MODE_TOOL_LOOP_CAP_MULTIPLIER: usize = 6;
const PLAN_MODE_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 80;
const PLAN_MODE_ENTER_TRIGGER_STATUS: &str = "Plan Mode: explicit planning request detected. Entering read-only planning before continuing this turn.";
const PLAN_MODE_EXIT_TRIGGER_STATUS: &str = "Plan Mode: implementation intent detected from your message. Running `exit_plan_mode` for plan confirmation; once approved, VT Code will switch to Edit Mode and execute.";
const PLAN_MODE_EXIT_SWITCHED_CONTINUE_STATUS: &str =
    "Plan Mode disabled. Continuing this turn in Edit Mode to execute your implementation request.";

fn resolve_tool_loop_limit(configured_limit: usize, plan_mode_active: bool) -> usize {
    if configured_limit == 0 {
        UNLIMITED_TOOL_LOOPS
    } else if plan_mode_active {
        configured_limit.max(PLAN_MODE_MIN_TOOL_LOOPS)
    } else {
        configured_limit
    }
}

fn plan_mode_fully_disabled(ctx: &TurnLoopContext<'_>) -> bool {
    !ctx.session_stats.is_plan_mode()
        && !ctx.tool_registry.is_plan_mode()
        && !ctx.tool_registry.plan_mode_state().is_active()
}

fn configured_tool_loop_base_limit(ctx: &TurnLoopContext<'_>) -> usize {
    let configured = ctx
        .vt_cfg
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_MAX_TOOL_LOOPS);
    if ctx.session_stats.is_plan_mode() {
        configured.max(PLAN_MODE_MIN_TOOL_LOOPS)
    } else {
        configured
    }
}

fn tool_loop_hard_cap(base_limit: usize, plan_mode_active: bool) -> usize {
    if plan_mode_active {
        if base_limit >= PLAN_MODE_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
            return base_limit;
        }
        return base_limit
            .saturating_mul(PLAN_MODE_TOOL_LOOP_CAP_MULTIPLIER)
            .min(PLAN_MODE_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP);
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
    plan_mode_active: bool,
) -> usize {
    let remaining = hard_cap.saturating_sub(current_limit);
    let per_prompt_limit = if plan_mode_active {
        PLAN_MODE_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT
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
        plan_mode = ctx.session_stats.is_plan_mode(),
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
    working_history: &mut Vec<uni::Message>,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    let renderer = &mut *ctx.renderer;
    let tool_registry = &mut *ctx.tool_registry;
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;

    if let Some(receiver) = ctx.steering_receiver.as_mut() {
        loop {
            let mut pending = Vec::new();
            while let Ok(message) = receiver.try_recv() {
                pending.push(message);
            }

            if pending.is_empty() {
                break;
            }

            if pending
                .iter()
                .any(|message| matches!(message, SteeringMessage::SteerStop))
            {
                cancel_for_steering_stop(tool_registry, result).await;
                display_status(renderer, "Stop requested by steering signal.")?;
                return Ok(true);
            }

            if let Some(pause_index) = pending
                .iter()
                .position(|message| matches!(message, SteeringMessage::Pause))
            {
                for message in pending.drain(..pause_index) {
                    if let SteeringMessage::FollowUpInput(input) = message {
                        append_follow_up_input(renderer, working_history, input)?;
                    }
                }
                pending.remove(0);
                if handle_pause_signal(
                    renderer,
                    tool_registry,
                    ctrl_c_state,
                    ctrl_c_notify,
                    receiver,
                    working_history,
                    result,
                    pending,
                )
                .await?
                {
                    return Ok(true);
                }
                continue;
            }

            for message in pending {
                if let SteeringMessage::FollowUpInput(input) = message {
                    append_follow_up_input(renderer, working_history, input)?;
                }
            }
        }
    }

    Ok(false)
}

fn append_follow_up_input(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    working_history: &mut Vec<uni::Message>,
    input: String,
) -> Result<()> {
    display_status(renderer, &format!("Follow-up Input: {}", input))?;
    working_history.push(uni::Message::user(input));
    Ok(())
}

async fn cancel_for_steering_stop(
    tool_registry: &mut vtcode_core::tools::ToolRegistry,
    result: &mut TurnLoopResult,
) {
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
    working_history: &mut Vec<uni::Message>,
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
                append_follow_up_input(renderer, working_history, input)?;
            }
            SteeringMessage::Pause => {}
        }
    }

    if resumed {
        display_status(renderer, "Resumed by steering signal.")?;
        return Ok(false);
    }

    loop {
        match receiver.try_recv() {
            Ok(SteeringMessage::Resume) => {
                display_status(renderer, "Resumed by steering signal.")?;
                return Ok(false);
            }
            Ok(SteeringMessage::SteerStop) => {
                cancel_for_steering_stop(tool_registry, result).await;
                return Ok(true);
            }
            Ok(SteeringMessage::FollowUpInput(input)) => {
                append_follow_up_input(renderer, working_history, input)?;
            }
            Ok(SteeringMessage::Pause) => {}
            Err(TryRecvError::Disconnected) => return Ok(false),
            Err(TryRecvError::Empty) => {}
        }

        tokio::select! {
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(30)) => {}
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

pub(super) async fn maybe_handle_plan_mode_exit_trigger(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut [uni::Message],
    step_count: usize,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    if !ctx.session_stats.is_plan_mode() {
        return Ok(false);
    }

    let Some(last_user_msg) = working_history
        .iter()
        .rev()
        .find(|msg| msg.role == uni::MessageRole::User)
    else {
        return Ok(false);
    };

    let text = last_user_msg.content.as_text();
    let should_exit_plan = should_exit_plan_mode_from_user_text(&text)
        || should_exit_plan_mode_from_confirmation(&text, working_history);

    if !should_exit_plan {
        if is_short_confirmation_intent(&text) {
            display_status(ctx.renderer, &short_confirmation_hint_with_fallback())?;
        }
        return Ok(false);
    }

    display_status(ctx.renderer, PLAN_MODE_EXIT_TRIGGER_STATUS)?;

    use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        EXIT_PLAN_MODE_REASON_USER_REQUESTED_IMPLEMENTATION, build_exit_plan_mode_args,
        build_step_exit_plan_mode_call_id,
    };
    use vtcode_core::llm::provider::ToolCall;

    let args = build_exit_plan_mode_args(EXIT_PLAN_MODE_REASON_USER_REQUESTED_IMPLEMENTATION);
    let call = ToolCall::function(
        build_step_exit_plan_mode_call_id(step_count),
        tool_names::EXIT_PLAN_MODE.to_string(),
        serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
    );
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;
    let default_placeholder = ctx.default_placeholder.clone();
    let lifecycle_hooks = ctx.lifecycle_hooks;
    let vt_cfg = ctx.vt_cfg;
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
        Ok(_pipe_outcome) => {
            if !plan_mode_fully_disabled(ctx) {
                display_status(ctx.renderer, &plan_mode_still_active_hint_with_fallback())?;
                *result = TurnLoopResult::Completed;
                return Ok(true);
            }

            display_status(ctx.renderer, PLAN_MODE_EXIT_SWITCHED_CONTINUE_STATUS)?;
            Ok(false)
        }
        Err(err) => {
            display_error(ctx.renderer, "Failed to exit Plan Mode", &err)?;
            Ok(false)
        }
    }
}

pub(super) async fn maybe_handle_plan_mode_enter_trigger(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut [uni::Message],
    step_count: usize,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    if ctx.session_stats.is_plan_mode() {
        return Ok(false);
    }

    let Some(last_user_msg) = working_history
        .iter()
        .rev()
        .find(|msg| msg.role == uni::MessageRole::User)
    else {
        return Ok(false);
    };

    let text = last_user_msg.content.as_text();
    if !should_enter_plan_mode_from_user_text(&text) {
        return Ok(false);
    }

    display_status(ctx.renderer, PLAN_MODE_ENTER_TRIGGER_STATUS)?;

    use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
    use vtcode_core::llm::provider::ToolCall;

    let call = ToolCall::function(
        format!("call_{step_count}_enter_plan_mode"),
        tool_names::ENTER_PLAN_MODE.to_string(),
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
        Ok(_) if ctx.session_stats.is_plan_mode() => Ok(false),
        Ok(_) => {
            *result = TurnLoopResult::Completed;
            Ok(true)
        }
        Err(err) => {
            display_error(ctx.renderer, "Failed to enter Plan Mode", &err)?;
            *result = TurnLoopResult::Completed;
            Ok(true)
        }
    }
}

fn should_exit_plan_mode_from_user_text(text: &str) -> bool {
    let normalized = normalize_user_intent_text(text);
    let normalized_trimmed = normalized.trim();

    // Prefer explicit "stay in plan mode" / "don't implement" instructions over
    // generic implementation words that might appear in the same sentence.
    let stay_phrases = [
        "stay in plan mode",
        "keep in plan mode",
        "continue planning",
        "keep planning",
        "do not implement",
        "don t implement",
        "not ready to implement",
        "don t exit plan mode",
        "do not exit plan mode",
    ];
    if stay_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
    {
        return false;
    }

    // Handle short imperative commands like "implement" (with punctuation/slash variants).
    // These are common in TUI flows and should reliably trigger Plan Mode exit.
    let direct_commands = [
        "implement",
        "yes",
        "continue",
        "go",
        "start",
        "implement now",
        "start implementing",
        "start implementation",
        "execute plan",
        "execute the plan",
        "execute this plan",
        "switch to edit mode",
        "go to edit mode",
        "switch to agent mode",
        "exit plan mode",
        "exit plan mode and implement",
    ];
    if direct_commands.contains(&normalized_trimmed) {
        return true;
    }

    let trigger_phrases = [
        "start implement",
        "start implementation",
        "start implementing",
        "implement now",
        "implement the plan",
        "implement this plan",
        "begin implement",
        "begin implementation",
        "begin coding",
        "proceed to implement",
        "proceed with implementation",
        "proceed to coding",
        "proceed with coding",
        "execute the plan",
        "execute this plan",
        "let s implement",
        "lets implement",
        "go ahead and implement",
        "go ahead and code",
        "ready to implement",
        "start coding",
        "start building",
        "switch to agent mode",
        "switch to edit mode",
        "go to edit mode",
        "exit plan mode",
        "exit plan mode and implement",
    ];
    trigger_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

fn should_exit_plan_mode_from_confirmation(text: &str, working_history: &[uni::Message]) -> bool {
    is_short_confirmation_intent(text)
        && assistant_recently_prompted_implementation(working_history)
}

fn is_short_confirmation_intent(text: &str) -> bool {
    let normalized = normalize_user_intent_text(text);
    let normalized_trimmed = normalized.trim();
    let confirmation_tokens = [
        "yes",
        "y",
        "ok",
        "okay",
        "continue",
        "go",
        "go ahead",
        "proceed",
        "start",
        "start now",
        "begin",
        "begin now",
        "let s start",
        "lets start",
        "sounds good",
        "do it",
    ];
    confirmation_tokens.contains(&normalized_trimmed)
}

fn assistant_recently_prompted_implementation(working_history: &[uni::Message]) -> bool {
    let Some(last_user_index) = working_history
        .iter()
        .rposition(|msg| msg.role == uni::MessageRole::User)
    else {
        return false;
    };

    let Some(last_assistant_msg) = working_history[..last_user_index]
        .iter()
        .rev()
        .find(|msg| msg.role == uni::MessageRole::Assistant)
    else {
        return false;
    };

    let assistant_text = normalize_user_intent_text(&last_assistant_msg.content.as_text());
    let cues = [
        "implement this plan",
        "implement the plan",
        "ready to implement",
        "exit plan mode",
        "execute the plan",
        "switch out of plan mode",
        "start implementation",
        "start implementing",
        "start coding",
    ];
    cues.iter().any(|cue| assistant_text.contains(cue))
}

fn should_enter_plan_mode_from_user_text(text: &str) -> bool {
    let normalized = normalize_user_intent_text(text);
    let normalized_trimmed = normalized.trim();

    if normalized_trimmed == "/plan" || normalized_trimmed.starts_with("/plan ") {
        return true;
    }

    let explicit_phrases = [
        "make a plan",
        "create a plan",
        "write a plan",
        "come up with a plan",
        "plan this",
        "stay in plan mode",
        "keep planning",
        "continue planning",
        "before you implement make a plan",
        "before implementing make a plan",
        "outline the implementation plan",
    ];

    explicit_phrases
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

fn normalize_user_intent_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
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

    display_status(
        ctx.renderer,
        &format!("Reached maximum tool loops ({})", *current_max_tool_loops),
    )?;

    let plan_mode_active = ctx.session_stats.is_plan_mode();
    let base_limit = configured_tool_loop_base_limit(ctx);
    let hard_cap = tool_loop_hard_cap(base_limit, plan_mode_active);
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
            &format!(
                "Tool loop hard cap reached ({hard_cap}). Stopping turn to prevent runaway looping."
            ),
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
            let increment = clamp_tool_loop_increment(
                requested_increment,
                *current_max_tool_loops,
                hard_cap,
                plan_mode_active,
            );
            if increment == 0 {
                emit_loop_hard_cap_break_metric(
                    ctx,
                    step_count,
                    *current_max_tool_loops,
                    base_limit,
                    hard_cap,
                    "no_remaining_headroom",
                );
                display_status(
                    ctx.renderer,
                    "Tool loop limit cannot be increased further for this turn.",
                )?;
                return Ok(ToolLoopLimitAction::BreakLoop);
            }
            let previous_max_tool_loops = *current_max_tool_loops;
            *current_max_tool_loops = (*current_max_tool_loops).saturating_add(increment);
            {
                let mut validator = ctx.safety_validator.write().await;
                let current_session_limit = validator.get_session_limit();
                validator.set_limits(*current_max_tool_loops, current_session_limit);
                tracing::info!(
                    "Updated safety validator limits: turn={} (was {}), session={}",
                    *current_max_tool_loops,
                    previous_max_tool_loops,
                    current_session_limit
                );
            }
            display_status(
                ctx.renderer,
                &format!(
                    "Tool loop limit increased to {} (+{}, cap {})",
                    *current_max_tool_loops, increment, hard_cap
                ),
            )?;
            Ok(ToolLoopLimitAction::ContinueLoop)
        }
        _ => Ok(ToolLoopLimitAction::BreakLoop),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PLAN_MODE_EXIT_SWITCHED_CONTINUE_STATUS, PLAN_MODE_EXIT_TRIGGER_STATUS,
        PLAN_MODE_MIN_TOOL_LOOPS, UNLIMITED_TOOL_LOOPS, clamp_tool_loop_increment,
        extract_turn_config, handle_steering_messages, resolve_tool_loop_limit,
        should_enter_plan_mode_from_user_text, should_exit_plan_mode_from_confirmation,
        should_exit_plan_mode_from_user_text, tool_loop_hard_cap,
    };
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use std::time::Duration;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::agent::steering::SteeringMessage;
    use vtcode_core::llm::provider as uni;

    #[test]
    fn detects_implement_the_plan_trigger() {
        assert!(should_exit_plan_mode_from_user_text("Implement the plan."));
        assert!(should_exit_plan_mode_from_user_text(
            "Please execute this plan and start coding."
        ));
    }

    #[test]
    fn detects_existing_exit_intents() {
        assert!(should_exit_plan_mode_from_user_text(
            "Exit plan mode and implement."
        ));
        assert!(should_exit_plan_mode_from_user_text(
            "Switch to edit mode and proceed."
        ));
    }

    #[test]
    fn does_not_exit_when_user_wants_to_keep_planning() {
        assert!(!should_exit_plan_mode_from_user_text(
            "Don't implement yet, stay in plan mode and refine the plan."
        ));
        assert!(!should_exit_plan_mode_from_user_text(
            "Continue planning for now."
        ));
    }

    #[test]
    fn detects_bare_implement_trigger() {
        assert!(should_exit_plan_mode_from_user_text("implement"));
        assert!(should_exit_plan_mode_from_user_text("/implement"));
        assert!(should_exit_plan_mode_from_user_text("implement."));
    }

    #[test]
    fn detects_short_implement_variants() {
        assert!(should_exit_plan_mode_from_user_text("Implement now"));
        assert!(should_exit_plan_mode_from_user_text("Start implementing"));
    }

    #[test]
    fn detects_direct_confirmation_aliases_as_execute_intent() {
        assert!(should_exit_plan_mode_from_user_text("yes"));
        assert!(should_exit_plan_mode_from_user_text("continue"));
        assert!(should_exit_plan_mode_from_user_text("go"));
        assert!(should_exit_plan_mode_from_user_text("start"));
        assert!(should_exit_plan_mode_from_user_text("yes!"));
    }

    #[test]
    fn stay_mode_has_priority_over_implement_keyword() {
        assert!(!should_exit_plan_mode_from_user_text(
            "Do not implement yet; keep planning."
        ));
        assert!(!should_exit_plan_mode_from_user_text(
            "Stay in plan mode and don't implement."
        ));
    }

    #[test]
    fn does_not_false_trigger_on_non_intent_implementation_text() {
        assert!(!should_exit_plan_mode_from_user_text(
            "The implementation details are unclear."
        ));
    }

    #[test]
    fn detects_explicit_plan_mode_requests() {
        assert!(should_enter_plan_mode_from_user_text(
            "make a plan for this"
        ));
        assert!(should_enter_plan_mode_from_user_text(
            "before implementing, create a plan"
        ));
        assert!(should_enter_plan_mode_from_user_text(
            "outline the implementation plan"
        ));
    }

    #[test]
    fn does_not_enter_plan_mode_for_generic_research_requests() {
        assert!(!should_enter_plan_mode_from_user_text(
            "explore and tell me about the core agent loop"
        ));
        assert!(!should_enter_plan_mode_from_user_text(
            "review the runloop and summarize the behavior"
        ));
    }

    #[test]
    fn confirmation_words_trigger_with_implementation_prompt_context() {
        let history = vec![
            uni::Message::assistant("Implement this plan?".to_string()),
            uni::Message::user("yes".to_string()),
        ];
        assert!(should_exit_plan_mode_from_confirmation("yes", &history));
        assert!(should_exit_plan_mode_from_confirmation(
            "continue", &history
        ));
        assert!(should_exit_plan_mode_from_confirmation("go", &history));
        assert!(should_exit_plan_mode_from_confirmation("start", &history));
        assert!(should_exit_plan_mode_from_confirmation("begin", &history));
    }

    #[test]
    fn confirmation_words_do_not_trigger_without_implementation_prompt_context() {
        let history = vec![
            uni::Message::assistant("Continue planning and expand the risks section.".to_string()),
            uni::Message::user("yes".to_string()),
        ];
        assert!(!should_exit_plan_mode_from_confirmation("yes", &history));
        assert!(!should_exit_plan_mode_from_confirmation(
            "continue", &history
        ));
    }

    #[test]
    fn confirmation_words_do_not_trigger_when_stay_in_plan_mode_is_prompted() {
        let history = vec![
            uni::Message::assistant(
                "Do you want to stay in plan mode and revise the plan?".to_string(),
            ),
            uni::Message::user("yes".to_string()),
        ];
        assert!(!should_exit_plan_mode_from_confirmation("yes", &history));
        assert!(!should_exit_plan_mode_from_confirmation("start", &history));
    }

    #[test]
    fn plan_mode_exit_trigger_status_mentions_exit_tool_and_transition() {
        assert!(PLAN_MODE_EXIT_TRIGGER_STATUS.contains("exit_plan_mode"));
        assert!(PLAN_MODE_EXIT_TRIGGER_STATUS.contains("Edit Mode"));
        assert!(PLAN_MODE_EXIT_TRIGGER_STATUS.contains("implementation intent"));
    }

    #[test]
    fn plan_mode_exit_switched_continue_status_mentions_edit_mode_and_turn_continuation() {
        assert!(PLAN_MODE_EXIT_SWITCHED_CONTINUE_STATUS.contains("Edit Mode"));
        assert!(PLAN_MODE_EXIT_SWITCHED_CONTINUE_STATUS.contains("Continuing this turn"));
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
    fn extract_turn_config_applies_plan_mode_loop_floor() {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = 20;
        let turn_cfg = extract_turn_config(Some(&cfg), true);
        assert_eq!(turn_cfg.max_tool_loops, PLAN_MODE_MIN_TOOL_LOOPS);
    }

    #[test]
    fn extract_turn_config_keeps_non_plan_mode_loop_limit() {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = 20;
        let turn_cfg = extract_turn_config(Some(&cfg), false);
        assert_eq!(turn_cfg.max_tool_loops, 20);
    }

    #[test]
    fn resolve_tool_loop_limit_allows_unlimited_mode() {
        assert_eq!(resolve_tool_loop_limit(0, false), UNLIMITED_TOOL_LOOPS);
        assert_eq!(resolve_tool_loop_limit(0, true), UNLIMITED_TOOL_LOOPS);
    }

    #[test]
    fn extract_turn_config_forces_request_user_input_in_plan_mode() {
        let mut cfg = VTCodeConfig::default();
        cfg.chat.ask_questions.enabled = false;

        let turn_cfg = extract_turn_config(Some(&cfg), true);
        assert!(turn_cfg.request_user_input_enabled);
    }

    #[tokio::test]
    async fn steering_follow_up_inputs_append_in_order() {
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
        let mut result = TurnLoopResult::Completed;
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle steering")
        };

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed));
        let inputs: Vec<String> = working_history
            .iter()
            .map(|message| message.content.as_text().to_string())
            .collect();
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
        let mut result = TurnLoopResult::Completed;
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle paused steering")
        };
        resume_task.await.expect("resume task");

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed));
        let inputs: Vec<String> = working_history
            .iter()
            .map(|message| message.content.as_text().to_string())
            .collect();
        assert_eq!(inputs, vec!["refine search".to_string()]);
    }

    #[tokio::test]
    async fn paused_steering_keeps_follow_up_after_resume_in_same_batch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(SteeringMessage::Pause).expect("pause");
        sender.send(SteeringMessage::Resume).expect("resume");
        sender
            .send(SteeringMessage::FollowUpInput(
                "use the queued note".to_string(),
            ))
            .expect("follow-up");
        backing.set_steering_receiver(receiver);

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed;
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle paused steering batch")
        };

        assert!(!handled);
        assert!(matches!(result, TurnLoopResult::Completed));
        let inputs: Vec<String> = working_history
            .iter()
            .map(|message| message.content.as_text().to_string())
            .collect();
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
        let mut result = TurnLoopResult::Completed;
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle stop steering")
        };

        assert!(handled);
        assert!(matches!(result, TurnLoopResult::Cancelled));
        assert!(working_history.is_empty());
    }
}
