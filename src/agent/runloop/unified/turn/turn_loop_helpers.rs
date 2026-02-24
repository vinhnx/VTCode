use anyhow::Result;

use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, display_status};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use crate::hooks::lifecycle::SessionEndReason;
use vtcode_core::config::constants::defaults::{
    DEFAULT_MAX_CONVERSATION_TURNS, DEFAULT_MAX_REPEATED_TOOL_CALLS, DEFAULT_MAX_TOOL_LOOPS,
};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::llm::provider as uni;

#[derive(Debug, Clone)]
pub(super) struct PrecomputedTurnConfig {
    pub(super) max_tool_loops: usize,
    pub(super) tool_repeat_limit: usize,
    pub(super) max_session_turns: usize,
    pub(super) ask_questions_enabled: bool,
}

#[inline]
pub(super) fn extract_turn_config(
    vt_cfg: Option<&VTCodeConfig>,
    plan_mode_active: bool,
) -> PrecomputedTurnConfig {
    vt_cfg
        .map(|cfg| PrecomputedTurnConfig {
            max_tool_loops: if plan_mode_active {
                if cfg.tools.max_tool_loops > 0 {
                    cfg.tools.max_tool_loops.max(PLAN_MODE_MIN_TOOL_LOOPS)
                } else {
                    DEFAULT_MAX_TOOL_LOOPS.max(PLAN_MODE_MIN_TOOL_LOOPS)
                }
            } else if cfg.tools.max_tool_loops > 0 {
                cfg.tools.max_tool_loops
            } else {
                DEFAULT_MAX_TOOL_LOOPS
            },
            tool_repeat_limit: if cfg.tools.max_repeated_tool_calls > 0 {
                cfg.tools.max_repeated_tool_calls
            } else {
                DEFAULT_MAX_REPEATED_TOOL_CALLS
            },
            max_session_turns: cfg.agent.max_conversation_turns,
            ask_questions_enabled: cfg.chat.ask_questions.enabled,
        })
        .unwrap_or(PrecomputedTurnConfig {
            max_tool_loops: if plan_mode_active {
                DEFAULT_MAX_TOOL_LOOPS.max(PLAN_MODE_MIN_TOOL_LOOPS)
            } else {
                DEFAULT_MAX_TOOL_LOOPS
            },
            tool_repeat_limit: DEFAULT_MAX_REPEATED_TOOL_CALLS,
            max_session_turns: DEFAULT_MAX_CONVERSATION_TURNS,
            ask_questions_enabled: true,
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
    requested_increment
        .min(per_prompt_limit)
        .min(remaining)
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
    if let Some(receiver) = ctx.steering_receiver {
        match receiver.try_recv() {
            Ok(SteeringMessage::SteerStop) => {
                display_status(ctx.renderer, "Stopped by steering signal.")?;
                *result = TurnLoopResult::Cancelled;
                return Ok(true);
            }
            Ok(SteeringMessage::Pause) => {
                display_status(
                    ctx.renderer,
                    "Paused by steering signal. Waiting for Resume...",
                )?;
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                        _ = ctx.ctrl_c_notify.notified() => {
                            if ctx.ctrl_c_state.is_exit_requested() {
                                *result = TurnLoopResult::Exit;
                                break;
                            }
                            if ctx.ctrl_c_state.is_cancel_requested() {
                                *result = TurnLoopResult::Cancelled;
                                break;
                            }
                            continue;
                        }
                    }
                    match receiver.try_recv() {
                        Ok(SteeringMessage::Resume) => {
                            display_status(ctx.renderer, "Resumed by steering signal.")?;
                            break;
                        }
                        Ok(SteeringMessage::SteerStop) => {
                            *result = TurnLoopResult::Cancelled;
                            break;
                        }
                        _ => {}
                    }
                }
                if matches!(*result, TurnLoopResult::Cancelled | TurnLoopResult::Exit) {
                    return Ok(true);
                }
            }
            Ok(SteeringMessage::Resume) => {}
            Ok(SteeringMessage::FollowUpInput(input)) => {
                display_status(ctx.renderer, &format!("Follow-up Input: {}", input))?;
                working_history.push(uni::Message::user(input));
            }
            Err(_) => {}
        }
    }

    Ok(false)
}

pub(super) async fn handle_pre_request_action(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    session_end_reason: &mut SessionEndReason,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    use crate::agent::runloop::unified::context_manager::PreRequestAction;

    let context_window_size = ctx
        .provider_client
        .effective_context_size(&ctx.config.model);
    match ctx
        .context_manager
        .pre_request_check(working_history, context_window_size)
    {
        PreRequestAction::Stop(msg) => {
            display_error(
                ctx.renderer,
                "Session Limit Reached",
                &anyhow::anyhow!("{}", msg),
            )?;
            *result = TurnLoopResult::Aborted;
            *session_end_reason = SessionEndReason::Error;
            Ok(true)
        }
        PreRequestAction::Warn(msg) => {
            display_status(ctx.renderer, &format!("Warning: {}", msg))?;
            let alert = format!("SYSTEM ALERT: {}", msg);
            let duplicate_alert = working_history.last().is_some_and(|last| {
                last.role == uni::MessageRole::System
                    && last.content.as_text_borrowed() == Some(alert.as_str())
            });
            if !duplicate_alert {
                working_history.push(uni::Message::system(alert));
            }
            Ok(false)
        }
        PreRequestAction::Compact(msg) => {
            display_status(ctx.renderer, &msg)?;
            let compacted = ctx
                .context_manager
                .compact_history_if_needed(
                    working_history,
                    ctx.provider_client.as_ref(),
                    &ctx.config.model,
                )
                .await?;
            *working_history = compacted;

            if let Some(team) = ctx.session_stats.team_state.as_ref() {
                let snapshot = team.prompt_snapshot();
                if !snapshot.is_empty() {
                    working_history.retain(|msg| {
                        !(msg.role == uni::MessageRole::System
                            && msg
                                .content
                                .as_text_borrowed()
                                .is_some_and(|t| t.starts_with("[vtcode:team_state]")))
                    });
                    working_history.push(uni::Message::system(format!(
                        "[vtcode:team_state]\n{}",
                        snapshot
                    )));
                }
            }
            Ok(false)
        }
        PreRequestAction::Proceed => Ok(false),
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
            display_status(
                ctx.renderer,
                "Plan Mode: type `implement` (or `yes`/`continue`/`go`/`start`) to execute, or say `stay in plan mode` to revise.",
            )?;
        }
        return Ok(false);
    }

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
            *result = TurnLoopResult::Completed;
            Ok(true)
        }
        Err(err) => {
            display_error(ctx.renderer, "Failed to exit Plan Mode", &err)?;
            Ok(false)
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
            let increment =
                clamp_tool_loop_increment(
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
        clamp_tool_loop_increment, extract_turn_config, should_exit_plan_mode_from_confirmation,
        should_exit_plan_mode_from_user_text, tool_loop_hard_cap, PLAN_MODE_MIN_TOOL_LOOPS,
    };
    use vtcode_core::config::loader::VTCodeConfig;
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
}
