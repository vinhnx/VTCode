use anyhow::Result;
use serde_json::json;

use crate::agent::runloop::unified::planning_workflow::detect_enter_planning_intent;
use crate::agent::runloop::unified::turn::context::{TurnLoopResult, last_user_message};
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, display_status};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::core::agent::steering::{FollowUpProvenance, FollowUpSteer, SteerRejection, SteeringMessage};
use vtcode_core::llm::provider as uni;

#[derive(Debug, Clone)]
pub(super) struct PrecomputedTurnConfig {
    pub(super) tool_repeat_limit: usize,
    pub(super) request_user_input_enabled: bool,
}

#[inline]
pub(super) fn extract_turn_config(
    vt_cfg: Option<&VTCodeConfig>,
    planning_active: bool,
    interactive_session: bool,
) -> PrecomputedTurnConfig {
    let features = FeatureSet::from_config(vt_cfg);
    vt_cfg
        .map(|cfg| PrecomputedTurnConfig {
            tool_repeat_limit: if cfg.tools.max_repeated_tool_calls > 0 {
                cfg.tools.max_repeated_tool_calls
            } else {
                DEFAULT_MAX_REPEATED_TOOL_CALLS
            },
            request_user_input_enabled: features.request_user_input_enabled(planning_active, interactive_session),
        })
        .unwrap_or(PrecomputedTurnConfig {
            tool_repeat_limit: DEFAULT_MAX_REPEATED_TOOL_CALLS,
            request_user_input_enabled: features.request_user_input_enabled(planning_active, interactive_session),
        })
}

/// Detects a stale recovery status response that incorrectly carries the
/// planning turn's tool-disabled state into the fresh approved-plan execution
/// turn. This is intentionally narrow: ordinary blocker explanations remain
/// valid build responses, while the exact pause language is retried with the
/// write-capable execution context.
pub(super) fn is_stale_approved_plan_pause_response(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let pause_marker = [
        "implementation is paused",
        "implementation paused",
        "wait for the next turn",
        "pending step",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    let unavailable_marker = [
        "tool use is disabled",
        "tools are disabled",
        "normal tool availability is restored",
        "no edits, builds, or tests were run",
    ]
    .iter()
    .any(|marker| lower.contains(marker));

    pause_marker && unavailable_marker
}

/// Detects an explicit declaration that the edit is being applied now
/// ("now applying …", "now I'll apply …"). Intentionally narrow:
/// reading-framed ("now reading … to make the edits precise"), past-tense
/// ("applied the fix"), and conditional ("will apply after review") text
/// must not arm declared-edit enforcement. Returns the first matching
/// line, trimmed and truncated, for the enforcement message.
pub(super) fn declares_imminent_edit(text: &str) -> Option<String> {
    /// Snippet budget for the enforcement message.
    const MAX_DECLARATION_CHARS: usize = 160;
    /// "now" must follow "applying" within this many characters; keeps the
    /// reversed order ("applying … now") tight enough to exclude research
    /// framing ("applying careful review before the next step").
    const APPLY_NOW_SPAN_CHARS: usize = 64;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let normalized: String = line
            .to_ascii_lowercase()
            .chars()
            .filter(|c| *c != '\'' && *c != '’' && *c != '‘')
            .collect();
        let direct = ["now applying", "now ill apply", "will now apply", "making the edit"]
            .iter()
            .any(|marker| normalized.contains(marker));
        let spanned = normalized.match_indices("applying").any(|(start, _)| {
            normalized[start + "applying".len()..]
                .chars()
                .take(APPLY_NOW_SPAN_CHARS)
                .collect::<String>()
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word == "now")
        });
        if direct || spanned {
            return Some(line.chars().take(MAX_DECLARATION_CHARS).collect());
        }
    }
    None
}

const PLANNING_WORKFLOW_ENTER_TRIGGER_STATUS: &str =
    "Planning workflow: explicit planning request detected. Entering read-only planning before continuing this turn.";

pub(super) async fn handle_steering_messages(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    let renderer = &mut *ctx.renderer;
    let tool_registry = &mut *ctx.tool_registry;
    let ctrl_c_state = ctx.ctrl_c_state;
    let ctrl_c_notify = ctx.ctrl_c_notify;

    let Some(mut receiver) = ctx.runtime_steering.take_receiver() else {
        let user_input = apply_pending_follow_ups_mid_turn(renderer, ctx.runtime_steering, working_history)?;
        record_planning_steering_revision(ctx, user_input).await?;
        return Ok(false);
    };

    // Inputs accepted during this call are acknowledged once their outcome is
    // known: delivered mid-turn ("Steered into active turn") or deferred to
    // the next turn after an interrupt ("Queued Follow-up Input").
    let mut accepted_this_call: Vec<String> = Vec::new();
    let steering_result: Result<bool> = loop {
        let mut pending = Vec::new();
        while let Ok(message) = receiver.try_recv() {
            pending.push(message);
        }

        if pending.is_empty() {
            break Ok(false);
        }

        if pending.iter().any(|message| matches!(message, SteeringMessage::SteerStop)) {
            // Cancellation outranks steering: reject every steer in this batch
            // with an explicit reason so senders fall back exactly once.
            ctx.runtime_steering.set_cancellation_precedence(true);
            for message in pending {
                if let SteeringMessage::FollowUpInput(steer) = message {
                    steer.reject(SteerRejection::CancellationPrecedence);
                }
            }
            cancel_for_steering_stop(tool_registry, result).await;
            display_status(renderer, "Stop requested by steering signal.")?;
            break Ok(true);
        }

        if let Some(pause_index) = pending.iter().position(|message| matches!(message, SteeringMessage::Pause)) {
            for message in pending.drain(..pause_index) {
                if let SteeringMessage::FollowUpInput(steer) = message {
                    queue_follow_up_input(renderer, ctx.runtime_steering, steer, &mut accepted_this_call)?;
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
                &mut accepted_this_call,
            )
            .await?
            {
                break Ok(true);
            }
            continue;
        }

        for message in pending {
            if let SteeringMessage::FollowUpInput(steer) = message {
                queue_follow_up_input(renderer, ctx.runtime_steering, steer, &mut accepted_this_call)?;
            }
        }
    };

    ctx.runtime_steering.set_receiver(Some(receiver));
    let steering_interrupted = steering_result?;
    if !steering_interrupted {
        // Deliver queued steering to the live history so the next LLM request
        // in this turn already sees it. Intents move to the in-flight queue
        // and are acknowledged by the post-turn history checkpoint, exactly
        // like turn-boundary delivery.
        let user_input = apply_pending_follow_ups_mid_turn(renderer, ctx.runtime_steering, working_history)?;
        record_planning_steering_revision(ctx, user_input).await?;
    }
    if !ctx.runtime_steering.pending_follow_up_intents_snapshot().is_empty() {
        let session_id = ctx.tool_registry.harness_context_snapshot().session_id;
        let steering_update = vtcode_core::compaction::memory_envelope::SessionMemoryEnvelopeUpdate {
            pending_intents: Some(ctx.runtime_steering.pending_follow_up_intents_snapshot()),
            applied_intent_ids: ctx.runtime_steering.applied_follow_up_intent_ids().iter().cloned().collect(),
            ..Default::default()
        };
        if let Err(error) = crate::agent::runloop::unified::turn::compaction::refresh_session_memory_envelope_async(
            ctx.config.workspace.as_path(),
            &session_id,
            super::turn_loop::effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            working_history,
            ctx.session_stats,
            Some(&steering_update),
        )
        .await
        {
            tracing::warn!(%error, session_id = %session_id, "Failed to persist queued steering intent");
        }
    }
    if steering_interrupted {
        for input in &accepted_this_call {
            display_status(ctx.renderer, &format!("Queued Follow-up Input: {input}"))?;
        }
        return Ok(true);
    }

    Ok(false)
}

fn queue_follow_up_input(
    _renderer: &mut vtcode_tui_runtime::utils::ansi::AnsiRenderer,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    steer: FollowUpSteer,
    accepted: &mut Vec<String>,
) -> Result<()> {
    let text = steer.text.clone();
    if runtime_steering.admit_follow_up_steer(steer).is_some() {
        accepted.push(text);
    }
    Ok(())
}

/// Apply every queued follow-up intent to the live turn history so the next
/// LLM request in this turn sees it (mid-turn steering). Each intent moves to
/// the in-flight queue and stays in the pending snapshot until the post-turn
/// history checkpoint acknowledges it, preserving the crash-recovery
/// contract.
///
/// Harness-generated continuations (tracker/plan auto-continue, background
/// completion, verification recovery) stay quiet: the full internal prompt is
/// model-facing, so echoing it to the transcript is TUI noise. Only genuine
/// user steering gets a `Steered into active turn` status line.
pub(crate) fn apply_pending_follow_ups_mid_turn(
    renderer: &mut vtcode_tui_runtime::utils::ansi::AnsiRenderer,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    working_history: &mut Vec<uni::Message>,
) -> Result<bool> {
    let mut user_input = false;
    for intent in runtime_steering.drain_follow_up_intents_to_in_flight() {
        let (intent_id, input, provenance) = intent.into_parts_with_provenance();
        push_steered_user_message(working_history, &intent_id, &input, provenance);
        if provenance == FollowUpProvenance::Synthetic {
            tracing::debug!("Applied synthetic follow-up mid-turn without TUI echo");
            continue;
        }
        user_input = true;
        display_status(renderer, &format!("Steered into active turn: {input}"))?;
    }
    Ok(user_input)
}

async fn record_planning_steering_revision(ctx: &mut TurnLoopContext<'_>, user_input: bool) -> Result<()> {
    if user_input
        && ctx.is_planning_active()
        && crate::agent::runloop::unified::planning_workflow::recovery_state::begin_user_revision(
            &mut ctx.plan_session.recovery,
        )
        && let Some(emitter) = ctx.harness_emitter
    {
        emitter
            .persist_checkpoint(&crate::agent::runloop::unified::planning_workflow::recovery_state::checkpoint_event(
                &ctx.plan_session.recovery,
                &ctx.harness_state.run_id.0,
                &ctx.harness_state.turn_id.0,
            ))
            .await?;
    }
    Ok(())
}

/// True for machine-generated continuation prompts that must not echo to the
/// TUI. Matches the stable openings of every harness-queued follow-up so a
/// reworded tail cannot reintroduce transcript noise.
pub(crate) fn is_internal_harness_follow_up(input: &str) -> bool {
    use crate::agent::runloop::unified::turn::session::slash_commands::GOAL_AUTO_CONTINUE_PREFIX;
    use crate::agent::runloop::unified::turn::session_loop::{
        BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, MONITOR_CONTINUATION_PROMPT_PREFIX,
        VERIFICATION_AUTO_RECOVERY_PREFIX,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        PLAN_MODE_AUTO_CONTINUE_MARKER, RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX,
        TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
    };

    input.starts_with(crate::agent::runloop::unified::state::PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX)
        || input.starts_with(TRACKER_CONTINUE_FOLLOW_UP_PREFIX)
        || input.starts_with(PLAN_MODE_AUTO_CONTINUE_MARKER)
        || input.starts_with(RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX)
        || input.starts_with(BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX)
        || input.starts_with(MONITOR_CONTINUATION_PROMPT_PREFIX)
        || input.starts_with(VERIFICATION_AUTO_RECOVERY_PREFIX)
        || input.starts_with(GOAL_AUTO_CONTINUE_PREFIX)
}

const FRESH_TURN_TOOL_GUIDANCE: &str = "Fresh turn: the previous turn's preview budget and tool-free recovery restrictions have expired. Tools are available subject to this turn's catalog, planning mode, safety, verification, and permission checks. Older recovery messages do not disable tools in this turn. If earlier file contents were cleared, recover the needed context with a targeted read or small spool range before editing; do not repeat broad inspections.";

pub(crate) fn is_turn_scoped_tool_restriction(text: &str) -> bool {
    // Inspect the runtime-owned instruction, not bounded evidence that a
    // planning synthesis directive may append beneath it.
    let instruction = text.lines().next().unwrap_or_default().trim().to_ascii_lowercase();
    if instruction.starts_with("tool preview budget exhausted;")
        || instruction.starts_with("planning recovery: the proposed plan was rejected.")
    {
        return true;
    }
    let recovery_family = [
        "recovery:",
        "planning recovery:",
        "planning recovery synthesis:",
        "planning navigation produced",
        "planning tool preview budget exhausted",
        "planning research completed",
        "navigation loop detected",
        "repeated low-signal navigation calls",
        "diverse low-signal navigation reached",
        "turn balancer detected repeated low-signal tool churn",
        "tool loop budget exhausted",
        "tool wall-clock budget exhausted for this turn",
        "tool follow-up failed.",
        "model returned no answer after tool activity.",
        "model follow-up failed after tool activity.",
    ]
    .iter()
    .any(|prefix| instruction.starts_with(prefix));
    recovery_family
        && (instruction.contains("tools are disabled")
            || instruction.contains("tools disabled")
            || instruction.contains("do not emit tool calls"))
}

/// Supersede expired recovery guidance without rewriting replayed history.
/// Only call at a fresh turn boundary, never during an active recovery pass.
pub(super) fn restore_fresh_turn_tool_guidance(history: &mut Vec<uni::Message>, recovery_active: bool) {
    if recovery_active {
        return;
    }
    // A newer restoration supersedes every older restriction. Search only
    // back to that boundary so later turns neither duplicate the instruction
    // nor scan all of the session's old recovery history.
    let has_expired_restriction = history
        .iter()
        .rev()
        .filter(|message| message.role == uni::MessageRole::System)
        .find_map(|message| {
            let text = message.content.as_text();
            if text == FRESH_TURN_TOOL_GUIDANCE {
                Some(false)
            } else if is_turn_scoped_tool_restriction(&text) {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(false);
    if has_expired_restriction {
        history.push(uni::Message::system(FRESH_TURN_TOOL_GUIDANCE.to_owned()));
    }
}

/// Append a steered user message tagged with its intent id and provenance so
/// restart recovery can dedupe it (mirrors the corresponding
/// `AgentSessionState` insertion method).
pub(crate) fn push_steered_user_message(
    working_history: &mut Vec<uni::Message>,
    intent_id: &str,
    input: &str,
    provenance: FollowUpProvenance,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let tokens = vtcode_commons::tokens::heuristic_tokens_from_byte_len(input.len());
    let metadata = match provenance {
        FollowUpProvenance::UserInput => vtcode_commons::message_metadata::MessageMetadata::user_input(now, tokens),
        FollowUpProvenance::Synthetic => vtcode_commons::message_metadata::MessageMetadata::harness_notice(now, tokens),
    };
    working_history.push(uni::Message::user(input.to_string()).with_metadata(metadata.with_intent_id(intent_id)));
}

async fn cancel_for_steering_stop(tool_registry: &mut vtcode_core::tools::ToolRegistry, result: &mut TurnLoopResult) {
    if let Err(err) = tool_registry.terminate_active_exec_sessions_async().await {
        tracing::warn!(error = %err, "Failed to terminate exec sessions after steering stop");
    }
    *result = TurnLoopResult::Cancelled;
}

async fn handle_pause_signal(
    renderer: &mut vtcode_tui_runtime::utils::ansi::AnsiRenderer,
    tool_registry: &mut vtcode_core::tools::ToolRegistry,
    ctrl_c_state: &crate::agent::runloop::unified::state::CtrlCState,
    ctrl_c_notify: &tokio::sync::Notify,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    result: &mut TurnLoopResult,
    pending: Vec<SteeringMessage>,
    accepted: &mut Vec<String>,
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
            SteeringMessage::FollowUpInput(steer) => {
                queue_follow_up_input(renderer, runtime_steering, steer, accepted)?;
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
                    Some(SteeringMessage::FollowUpInput(steer)) => {
                        queue_follow_up_input(renderer, runtime_steering, steer, accepted)?;
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

pub(super) async fn maybe_handle_planning_enter_trigger(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut [uni::Message],
    step_count: usize,
    result: &mut TurnLoopResult,
) -> Result<bool> {
    let Some(last_user_msg) = last_user_message(working_history) else {
        return Ok(false);
    };

    let text = last_user_msg.content.as_text();
    if is_internal_harness_follow_up(&text) || !detect_enter_planning_intent(&text) {
        return Ok(false);
    }
    *ctx.auto_finish_planning_attempted = true;
    if ctx.is_planning_active() {
        ctx.plan_session.mark_explicit_plan_request();
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
    let effective_cfg = super::turn_loop::effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg).cloned();
    let mut run_ctx = ctx.as_run_loop_context();

    match run_tool_call(
        &mut run_ctx,
        &call,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        lifecycle_hooks,
        true,
        effective_cfg.as_ref(),
        step_count,
        false,
    )
    .await
    {
        Ok(_) if ctx.is_planning_active() => {
            ctx.plan_session.mark_explicit_plan_request();
            Ok(false)
        }
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
#[cfg(test)]
mod tests;
