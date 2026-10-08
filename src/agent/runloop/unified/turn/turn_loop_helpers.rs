use anyhow::Result;
use serde_json::json;

use crate::agent::runloop::unified::inline_events::harness::harness_event;
use crate::agent::runloop::unified::planning_workflow::detect_enter_planning_intent;
use crate::agent::runloop::unified::run_loop_context::{
    BudgetExhaustedMetrics, HarnessTurnState, budget_kind, emit_budget_exhausted_metric, full_auto_loop_grants_enabled,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, display_status};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS;
use vtcode_core::config::constants::tool_limits::{
    APPROVED_PLAN_MIN_TOOL_CALLS_PER_TURN, APPROVED_PLAN_TOOL_LOOP_INCREMENT, DEFAULT_MAX_CONVERSATION_TURNS,
    DEFAULT_MAX_TOOL_LOOPS, MAX_TOOL_LOOP_INCREMENT_PER_PROMPT, PLANNING_WORKFLOW_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT,
    PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN, PLANNING_WORKFLOW_MIN_TOOL_LOOPS, tool_loop_hard_cap,
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
fn tool_loop_limit_recovery_reason(limit: usize) -> String {
    format!(
        "Tool loop budget exhausted before a final response ({limit}/{limit}). Tools are disabled for one bounded synthesis pass; answer from the completed tool outputs and state any incomplete work explicitly."
    )
}

/// Initialize the loop allowance for a turn that is executing an approved
/// plan. The allowance is applied at turn initialization only; later
/// extensions continue through the existing grant path (automatic in
/// full-auto runs with auto-grant enabled, otherwise prompt-driven).
pub(super) fn initial_tool_loop_limit(configured_limit: usize, approved_plan_execution: bool) -> usize {
    if configured_limit == 0 || configured_limit == UNLIMITED_TOOL_LOOPS {
        return UNLIMITED_TOOL_LOOPS;
    }
    if !approved_plan_execution {
        return configured_limit;
    }

    configured_limit
        .saturating_add(APPROVED_PLAN_TOOL_LOOP_INCREMENT)
        .min(tool_loop_hard_cap(configured_limit, false))
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

/// Minimum per-turn tool-call budget while the planning workflow is active.
/// Plan-mode research legitimately needs far more read-only calls than a
/// build-mode turn; a lower configured `max_tool_calls_per_turn` must not
/// starve planning (checkpoint turn_804: research died at the build-mode cap).
/// Planning-aware per-turn tool-call budget. `0` stays `0` (unlimited);
/// planning raises the configured limit to the planning research floor.
pub(in crate::agent::runloop::unified::turn) fn effective_max_tool_calls_for_turn(
    configured_limit: usize,
    planning_active: bool,
) -> usize {
    if configured_limit == 0 {
        0
    } else if planning_active {
        configured_limit.max(PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN)
    } else {
        configured_limit
    }
}

/// Approved plans need enough room for implementation and verification after
/// planning research has already consumed a turn. Keep this floor separate
/// from ordinary build turns so unrelated requests retain their configured
/// safety budget.
pub(super) fn effective_max_tool_calls_for_approved_plan_execution(configured_limit: usize) -> usize {
    if configured_limit == 0 {
        0
    } else {
        configured_limit.max(APPROVED_PLAN_MIN_TOOL_CALLS_PER_TURN)
    }
}

/// Remaining tool-call / tool-loop floor after a mid-turn Build↔Plan switch.
///
/// Mode floors (`max(limit, 120)`) only help when the turn is still empty. If
/// Build already consumed most of the cap and the workflow auto-switches to
/// Plan (or back to implementation), the new mode must get a full floor of
/// *remaining* headroom from now — not a cap that is already nearly spent.
pub(super) fn apply_mode_switch_remaining_tool_call_floor(
    max_tool_calls: &mut usize,
    used_tool_calls: usize,
    planning_active: bool,
) {
    if *max_tool_calls == 0 {
        return;
    }
    let floor = if planning_active {
        PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN
    } else {
        APPROVED_PLAN_MIN_TOOL_CALLS_PER_TURN
    };
    *max_tool_calls = (*max_tool_calls).max(used_tool_calls.saturating_add(floor));
}

/// Same remaining-headroom rule for tool-loop iterations (`step_count` is
/// loops already taken this turn).
pub(super) fn apply_mode_switch_remaining_tool_loop_floor(
    current_max_tool_loops: &mut usize,
    step_count: usize,
    planning_active: bool,
) {
    if *current_max_tool_loops == UNLIMITED_TOOL_LOOPS {
        return;
    }
    let floor = if planning_active {
        PLANNING_WORKFLOW_MIN_TOOL_LOOPS
    } else {
        // Implementation after a plan still needs a full research-sized
        // runway for edits + verification in the same turn.
        PLANNING_WORKFLOW_MIN_TOOL_LOOPS.max(DEFAULT_MAX_TOOL_LOOPS)
    };
    *current_max_tool_loops = (*current_max_tool_loops).max(step_count.saturating_add(floor));
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

const PLANNING_WORKFLOW_ENTER_TRIGGER_STATUS: &str =
    "Planning workflow: explicit planning request detected. Entering read-only planning before continuing this turn.";

fn resolve_tool_loop_limit(configured_limit: usize, planning_active: bool) -> usize {
    if configured_limit == 0 {
        UNLIMITED_TOOL_LOOPS
    } else if planning_active {
        configured_limit.max(PLANNING_WORKFLOW_MIN_TOOL_LOOPS)
    } else {
        configured_limit
    }
}

fn configured_tool_loop_base_limit(ctx: &TurnLoopContext<'_>) -> usize {
    let configured = super::turn_loop::effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg)
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_MAX_TOOL_LOOPS);
    resolve_tool_loop_limit(configured, ctx.is_planning_active())
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

/// Increment a full-auto run (or a session-preauthorized interactive run)
/// grants itself when it hits the tool-loop limit:
/// the same maximum one manual approval may add, clamped to the remaining
/// headroom below the hard cap. Pure so the grant arithmetic stays unit
/// tested without standing up an interactive session.
///
/// The planning branch is currently unreachable in the unified runloop:
/// planning loop exhaustion early-returns to tool-free synthesis above and
/// planning session limits are unbounded. It is retained for arithmetic
/// symmetry with `tool_loop_hard_cap` and its tests, not as a live path.
fn auto_tool_loop_grant_increment(current_limit: usize, hard_cap: usize, planning_active: bool) -> usize {
    let per_prompt_limit = if planning_active {
        PLANNING_WORKFLOW_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT
    } else {
        MAX_TOOL_LOOP_INCREMENT_PER_PROMPT
    };
    clamp_tool_loop_increment(per_prompt_limit, current_limit, hard_cap, planning_active)
}

/// How a tool-loop increase is authorized at a limit hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolLoopGrantSource {
    /// Full-auto policy with `auto_grant_tool_limits` on.
    FullAuto,
    /// Interactive session where the user already granted once.
    SessionPreauthorized,
    /// Manual HITL approval (or denial handled by the caller).
    Manual,
}

impl ToolLoopGrantSource {
    /// Whether this source skips the HITL modal and grants the max increment.
    fn is_automatic(self) -> bool {
        matches!(self, Self::FullAuto | Self::SessionPreauthorized)
    }
}

/// Pure grant-policy decision at a tool-loop limit hit.
///
/// Full-auto always auto-grants. Otherwise the first interactive hit prompts;
/// once the session latch is set by a successful grant, later hits auto-grant
/// the maximum increment without a modal.
fn tool_loop_grant_source(full_auto_grants_enabled: bool, session_preauthorized: bool) -> ToolLoopGrantSource {
    if full_auto_grants_enabled {
        ToolLoopGrantSource::FullAuto
    } else if session_preauthorized {
        ToolLoopGrantSource::SessionPreauthorized
    } else {
        ToolLoopGrantSource::Manual
    }
}

/// Apply one tool-loop limit increase shared by the full-auto grant path,
/// the session-preauthorized path, and the manual prompt path. Only the
/// user-facing wording records who authorized the increase; the event kind
/// and continuation semantics are identical.
///
/// A successful manual grant latches session-preauthorized auto-grants for
/// later limit hits in this process session. Denial never reaches this
/// function, so it cannot latch.
fn apply_tool_loop_grant(
    ctx: &mut TurnLoopContext<'_>,
    current_max_tool_loops: &mut usize,
    increment: usize,
    requested_increment: usize,
    hard_cap: usize,
    grant_source: ToolLoopGrantSource,
) -> Result<ToolLoopLimitAction> {
    if grant_source == ToolLoopGrantSource::Manual {
        ctx.session_stats.mark_tool_loop_grant_preauthorized();
    }
    let previous_max_tool_loops = *current_max_tool_loops;
    *current_max_tool_loops = (*current_max_tool_loops).saturating_add(increment);
    let agent_name = ctx.active_primary_agent.active().name();
    let event_message = match grant_source {
        ToolLoopGrantSource::FullAuto => {
            format!(
                "Full-auto auto-granted +{} tool loops to current agent {} (limit {}); continuing this turn and reusing existing tool outputs.",
                increment, agent_name, *current_max_tool_loops,
            )
        }
        ToolLoopGrantSource::SessionPreauthorized => {
            format!(
                "Auto-granted +{} tool loops to current agent {} (limit {}); earlier grant this session preauthorized further increases. Continuing this turn and reusing existing tool outputs.",
                increment, agent_name, *current_max_tool_loops,
            )
        }
        ToolLoopGrantSource::Manual => {
            format!(
                "Current agent {} granted +{} tool loops (limit {}); continuing this turn and reusing existing tool outputs.",
                agent_name, increment, *current_max_tool_loops,
            )
        }
    };
    if let Some(emitter) = ctx.harness_emitter
        && let Err(error) = emitter.emit(harness_event(
            vtcode_core::exec::events::HarnessEventKind::ToolLoopLimitIncreased,
            Some(event_message),
            None,
            None,
            None,
        ))
    {
        tracing::debug!(error = %error, "Failed to emit tool-loop grant event");
    }
    tracing::info!(
        grant_source = ?grant_source,
        "Updated tool loop limit: turn={} (was {}), session tool-call limit remains unchanged",
        *current_max_tool_loops,
        previous_max_tool_loops,
    );
    let status_message = match grant_source {
        ToolLoopGrantSource::FullAuto => {
            format!(
                "Full-auto auto-granted +{} tool loops (limit {}, cap {}); per-turn tool-call budget unchanged",
                increment, *current_max_tool_loops, hard_cap,
            )
        }
        ToolLoopGrantSource::SessionPreauthorized => {
            format!(
                "Auto-granted +{} tool loops (limit {}, cap {}); earlier grant this session preauthorized further increases; per-turn tool-call budget unchanged",
                increment, *current_max_tool_loops, hard_cap,
            )
        }
        ToolLoopGrantSource::Manual if requested_increment != increment => {
            format!(
                "Tool loop limit increased to {} (+{}, requested +{}, cap {}); per-turn tool-call budget unchanged",
                *current_max_tool_loops, increment, requested_increment, hard_cap,
            )
        }
        ToolLoopGrantSource::Manual => {
            format!(
                "Tool loop limit increased to {} (+{}, cap {}); per-turn tool-call budget unchanged",
                *current_max_tool_loops, increment, hard_cap,
            )
        }
    };
    display_status(ctx.renderer, &status_message)?;
    Ok(ToolLoopLimitAction::ContinueLoop)
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
    // Trajectory twin of the metric above: post-hoc reports of an exhausted
    // turn ("tool budget exhausted") are otherwise undebuggable because no
    // trajectory kind records which ceiling fired.
    emit_budget_exhausted_metric(
        ctx.traj,
        BudgetExhaustedMetrics {
            budget: budget_kind::TOOL_LOOP,
            used: current_limit,
            max: hard_cap,
            step_count: Some(step_count),
            planning_active: ctx.is_planning_active(),
            tool_calls: ctx.harness_state.tool_calls,
        },
    );
}

/// Arm the single tool-free synthesis pass used when a turn reaches its loop
/// allowance. The loop allowance is made unlimited only for the control loop
/// itself; the recovery request disables tools at the provider boundary, so
/// this does not weaken the ordinary hard cap or permit another tool batch.
fn arm_tool_loop_synthesis_recovery(harness_state: &mut HarnessTurnState, current_max_tool_loops: &mut usize) -> bool {
    // `activate_recovery` only arms from the `Inactive` phase and reports
    // whether it did, so a pending/in-flight pass never widens the loop
    // allowance here.
    if !harness_state.activate_recovery(tool_loop_limit_recovery_reason(*current_max_tool_loops)) {
        return false;
    }

    *current_max_tool_loops = UNLIMITED_TOOL_LOOPS;
    true
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

    let Some(mut receiver) = ctx.runtime_steering.take_receiver() else {
        apply_pending_follow_ups_mid_turn(renderer, ctx.runtime_steering, working_history)?;
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
            cancel_for_steering_stop(tool_registry, result).await;
            display_status(renderer, "Stop requested by steering signal.")?;
            break Ok(true);
        }

        if let Some(pause_index) = pending.iter().position(|message| matches!(message, SteeringMessage::Pause)) {
            for message in pending.drain(..pause_index) {
                if let SteeringMessage::FollowUpInput(input) = message {
                    queue_follow_up_input(renderer, ctx.runtime_steering, input, &mut accepted_this_call)?;
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
            if let SteeringMessage::FollowUpInput(input) = message {
                queue_follow_up_input(renderer, ctx.runtime_steering, input, &mut accepted_this_call)?;
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
        apply_pending_follow_ups_mid_turn(renderer, ctx.runtime_steering, working_history)?;
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
            display_status(renderer, &format!("Queued Follow-up Input: {input}"))?;
        }
        return Ok(true);
    }

    Ok(false)
}

fn queue_follow_up_input(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    input: String,
    accepted: &mut Vec<String>,
) -> Result<()> {
    match runtime_steering.try_queue_follow_up_input(input.clone()) {
        Ok(()) => accepted.push(input),
        Err(error) => {
            tracing::warn!(%error, "Rejected follow-up steering input");
            display_status(renderer, &format!("Follow-up Input Rejected: {error}"))?;
        }
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
fn apply_pending_follow_ups_mid_turn(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    runtime_steering: &mut vtcode_core::core::agent::runtime::RuntimeSteering,
    working_history: &mut Vec<uni::Message>,
) -> Result<()> {
    for intent in runtime_steering.drain_follow_up_intents_to_in_flight() {
        let (intent_id, input) = intent.into_parts();
        push_steered_user_message(working_history, &intent_id, &input);
        if is_internal_harness_follow_up(&input) {
            tracing::debug!("Applied internal harness follow-up mid-turn without TUI echo");
            continue;
        }
        display_status(renderer, &format!("Steered into active turn: {input}"))?;
    }
    Ok(())
}

/// True for machine-generated continuation prompts that must not echo to the
/// TUI. Matches the stable openings of every harness-queued follow-up so a
/// reworded tail cannot reintroduce transcript noise.
pub(crate) fn is_internal_harness_follow_up(input: &str) -> bool {
    use crate::agent::runloop::unified::turn::session_loop::{
        BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, VERIFICATION_AUTO_RECOVERY_PREFIX,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        PLAN_MODE_AUTO_CONTINUE_MARKER, RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX,
        TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
    };

    input.starts_with(TRACKER_CONTINUE_FOLLOW_UP_PREFIX)
        || input.starts_with(PLAN_MODE_AUTO_CONTINUE_MARKER)
        || input.starts_with(RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX)
        || input.starts_with(BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX)
        || input.starts_with(VERIFICATION_AUTO_RECOVERY_PREFIX)
}

const FRESH_TURN_TOOL_GUIDANCE: &str = "Fresh turn: the previous turn's preview budget and tool-free recovery restrictions have expired. Tools are available subject to this turn's catalog, planning mode, safety, verification, and permission checks. Older recovery messages do not disable tools in this turn. If earlier file contents were cleared, recover the needed context with a targeted read or small spool range before editing; do not repeat broad inspections.";

fn is_turn_scoped_tool_restriction(text: &str) -> bool {
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
        "tool-call budget exhausted for this turn",
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

/// Append a steered user message tagged with its intent id so restart
/// recovery can dedupe it (mirrors
/// `AgentSessionState::add_user_message_with_intent`).
fn push_steered_user_message(working_history: &mut Vec<uni::Message>, intent_id: &str, input: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let tokens = input.len().saturating_div(4);
    let metadata = vtcode_commons::message_metadata::MessageMetadata::user_input(now, tokens).with_intent_id(intent_id);
    working_history.push(uni::Message::user(input.to_string()).with_metadata(metadata));
}

async fn cancel_for_steering_stop(tool_registry: &mut vtcode_core::tools::ToolRegistry, result: &mut TurnLoopResult) {
    if let Err(err) = tool_registry.terminate_active_exec_sessions_async().await {
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
            SteeringMessage::FollowUpInput(input) => {
                queue_follow_up_input(renderer, runtime_steering, input, accepted)?;
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
                        queue_follow_up_input(renderer, runtime_steering, input, accepted)?;
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

    let planning_active = ctx.is_planning_active();
    if planning_active {
        ctx.plan_session.mark_budget_exhausted();
        ctx.harness_state
            .activate_recovery(tool_loop_limit_recovery_reason(*current_max_tool_loops));
        ctx.harness_state.switch_to_tool_free_recovery();
        *current_max_tool_loops = UNLIMITED_TOOL_LOOPS;
        display_status(
            ctx.renderer,
            "Planning research has reached its safe limit. I’m stopping research and synthesizing a plan from the evidence already collected.",
        )?;
        return Ok(ToolLoopLimitAction::ContinueLoop);
    }

    display_status(ctx.renderer, &format!("Reached maximum tool loops ({})", *current_max_tool_loops))?;

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

    let grant_source = tool_loop_grant_source(
        full_auto_loop_grants_enabled(ctx.full_auto, super::turn_loop::effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg)),
        ctx.session_stats.tool_loop_grant_preauthorized(),
    );
    let prompt_result = if grant_source.is_automatic() {
        let increment = auto_tool_loop_grant_increment(*current_max_tool_loops, hard_cap, planning_active);
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
                &format!("Tool loop limit cannot be increased further for this turn (already at cap {hard_cap})."),
            )?;
            return Ok(ToolLoopLimitAction::BreakLoop);
        }
        return apply_tool_loop_grant(ctx, current_max_tool_loops, increment, increment, hard_cap, grant_source);
    } else {
        crate::agent::runloop::unified::tool_routing::prompt_tool_loop_limit_increase(
            ctx.handle,
            ctx.session,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            *current_max_tool_loops,
            hard_cap,
            Some(ctx.active_primary_agent.active().name()),
        )
        .await
    };
    match prompt_result {
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
                display_status(
                    ctx.renderer,
                    &format!("Tool loop limit cannot be increased further for this turn (already at cap {hard_cap})."),
                )?;
                return Ok(ToolLoopLimitAction::BreakLoop);
            }
            apply_tool_loop_grant(
                ctx,
                current_max_tool_loops,
                increment,
                requested_increment,
                hard_cap,
                ToolLoopGrantSource::Manual,
            )
        }
        _ => {
            display_status(
                ctx.renderer,
                "Tool loop limit was not increased. Synthesizing from the tool results already collected.",
            )?;
            if arm_tool_loop_synthesis_recovery(ctx.harness_state, current_max_tool_loops) {
                Ok(ToolLoopLimitAction::ContinueLoop)
            } else {
                Ok(ToolLoopLimitAction::BreakLoop)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ToolLoopGrantSource, UNLIMITED_TOOL_LOOPS, apply_mode_switch_remaining_tool_call_floor,
        apply_mode_switch_remaining_tool_loop_floor, arm_tool_loop_synthesis_recovery, auto_tool_loop_grant_increment,
        clamp_tool_loop_increment, effective_max_tool_calls_for_approved_plan_execution,
        effective_max_tool_calls_for_turn, extract_turn_config, handle_steering_messages, initial_tool_loop_limit,
        is_internal_harness_follow_up, is_stale_approved_plan_pause_response, resolve_safety_tool_call_limits,
        resolve_tool_loop_limit, tool_loop_grant_source, tool_loop_hard_cap, tool_loop_limit_recovery_reason,
    };
    use crate::agent::runloop::unified::planning_workflow::{
        PlanningIntent, detect_enter_planning_intent, detect_planning_intent,
    };
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use std::time::Duration;
    use vtcode_core::config::constants::tool_limits::PLANNING_WORKFLOW_MIN_TOOL_LOOPS;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::agent::steering::SteeringMessage;
    use vtcode_core::llm::provider::MessageRole;

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
    fn auto_grant_uses_max_prompt_step_clamped_to_remaining_headroom() {
        // Ordinary turns grant the +50 manual maximum: 20 -> 60 needs one
        // grant, 40 -> 120 needs two (40 + 50, then 90 + 30).
        assert_eq!(auto_tool_loop_grant_increment(20, 60, false), 40);
        assert_eq!(auto_tool_loop_grant_increment(40, 120, false), 50);
        assert_eq!(auto_tool_loop_grant_increment(90, 120, false), 30);
        // Planning turns grant the +80 manual maximum instead.
        assert_eq!(auto_tool_loop_grant_increment(60, 240, true), 80);
        assert_eq!(auto_tool_loop_grant_increment(200, 240, true), 40);
        // No headroom left means no grant: the caller breaks the loop.
        assert_eq!(auto_tool_loop_grant_increment(60, 60, false), 0);
        assert_eq!(auto_tool_loop_grant_increment(120, 120, false), 0);
        assert_eq!(auto_tool_loop_grant_increment(240, 240, true), 0);
    }

    #[test]
    fn tool_loop_grant_source_prefers_full_auto_then_session_latch() {
        assert_eq!(
            tool_loop_grant_source(true, false),
            ToolLoopGrantSource::FullAuto,
            "full-auto wins even before any interactive grant"
        );
        assert_eq!(
            tool_loop_grant_source(true, true),
            ToolLoopGrantSource::FullAuto,
            "full-auto wording stays full-auto after an interactive grant"
        );
        assert_eq!(
            tool_loop_grant_source(false, true),
            ToolLoopGrantSource::SessionPreauthorized,
            "after the first successful grant later hits auto-grant without a prompt"
        );
        assert_eq!(
            tool_loop_grant_source(false, false),
            ToolLoopGrantSource::Manual,
            "first interactive hit (and any denial retry) still prompts"
        );

        // Only Manual opens the HITL modal; the other two auto-grant max +N
        // via `auto_tool_loop_grant_increment` (already covered above).
        assert!(!tool_loop_grant_source(false, false).is_automatic());
        assert!(tool_loop_grant_source(false, true).is_automatic());
        assert!(tool_loop_grant_source(true, false).is_automatic());
        assert!(tool_loop_grant_source(true, true).is_automatic());
    }

    #[test]
    fn extract_turn_config_applies_planning_workflow_loop_floor() {
        for (configured_limit, expected_limit) in [(20, 60), (40, 60), (60, 60), (80, 80)] {
            let mut cfg = VTCodeConfig::default();
            cfg.tools.max_tool_loops = configured_limit;
            let turn_cfg = extract_turn_config(Some(&cfg), true, true);
            assert_eq!(turn_cfg.max_tool_loops, expected_limit);
        }
    }

    #[test]
    fn extract_turn_config_keeps_non_planning_workflow_loop_limit() {
        for configured_limit in [20, 40, 60, 80] {
            let mut cfg = VTCodeConfig::default();
            cfg.tools.max_tool_loops = configured_limit;
            let turn_cfg = extract_turn_config(Some(&cfg), false, true);
            assert_eq!(turn_cfg.max_tool_loops, configured_limit);
        }
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
    fn planning_workflow_applies_tool_call_floor() {
        assert_eq!(effective_max_tool_calls_for_turn(32, true), 120);
        assert_eq!(effective_max_tool_calls_for_turn(64, true), 120);
        assert_eq!(effective_max_tool_calls_for_turn(120, true), 120);
        assert_eq!(effective_max_tool_calls_for_turn(200, true), 200);
    }

    #[test]
    fn zero_tool_call_limit_stays_unlimited_in_all_modes() {
        assert_eq!(effective_max_tool_calls_for_turn(0, true), 0);
        assert_eq!(effective_max_tool_calls_for_turn(0, false), 0);
    }

    #[test]
    fn edit_mode_keeps_configured_tool_call_limit() {
        assert_eq!(effective_max_tool_calls_for_turn(32, false), 32);
    }

    #[test]
    fn mode_switch_grants_remaining_tool_call_floor() {
        // Build already spent most of a 120-cap; Plan entry must not inherit
        // the leftover 20-call budget.
        let mut max = 120usize;
        apply_mode_switch_remaining_tool_call_floor(&mut max, 110, true);
        assert_eq!(max, 110 + 120);
        // Cap already larger than used+floor is preserved.
        let mut max = 400usize;
        apply_mode_switch_remaining_tool_call_floor(&mut max, 10, true);
        assert_eq!(max, 400);
        // Unlimited stays unlimited.
        let mut max = 0usize;
        apply_mode_switch_remaining_tool_call_floor(&mut max, 10, true);
        assert_eq!(max, 0);
    }

    #[test]
    fn mode_switch_grants_remaining_tool_loop_floor() {
        let mut loops = 60usize;
        apply_mode_switch_remaining_tool_loop_floor(&mut loops, 55, true);
        assert_eq!(loops, 55 + PLANNING_WORKFLOW_MIN_TOOL_LOOPS);
        let mut loops = 200usize;
        apply_mode_switch_remaining_tool_loop_floor(&mut loops, 10, true);
        assert_eq!(loops, 200);
        let mut loops = usize::MAX;
        apply_mode_switch_remaining_tool_loop_floor(&mut loops, 10, true);
        assert_eq!(loops, usize::MAX);
    }

    #[test]
    fn approved_plan_execution_gets_a_fresh_implementation_budget() {
        assert_eq!(effective_max_tool_calls_for_approved_plan_execution(32), 120);
        assert_eq!(effective_max_tool_calls_for_approved_plan_execution(160), 160);
        assert_eq!(effective_max_tool_calls_for_approved_plan_execution(0), 0);
    }

    #[test]
    fn approved_plan_execution_gets_one_bounded_loop_allowance() {
        assert_eq!(initial_tool_loop_limit(40, true), 90);
        assert_eq!(initial_tool_loop_limit(100, true), 120);
        assert_eq!(initial_tool_loop_limit(0, true), UNLIMITED_TOOL_LOOPS);
        assert_eq!(initial_tool_loop_limit(40, false), 40);
        // The helper is an initialization transform; applying the allowance
        // to the already-initialized value is not part of the turn loop.
        assert_eq!(initial_tool_loop_limit(initial_tool_loop_limit(40, true), false), 90);
    }

    #[test]
    fn exhausted_loop_limit_arms_one_tool_free_synthesis_pass() {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 40, 0, 1);
        let mut loop_limit = 40;

        assert!(arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
        assert_eq!(loop_limit, UNLIMITED_TOOL_LOOPS);
        assert!(state.is_recovery_active());
        assert!(state.recovery_is_tool_free());
        assert_eq!(state.recovery_reason(), Some(tool_loop_limit_recovery_reason(40).as_str()));

        // The recovery request is the only pass added by this transition.
        assert!(!arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
        assert!(state.consume_recovery_pass());
        assert!(state.finish_recovery_pass());
        assert!(!arm_tool_loop_synthesis_recovery(&mut state, &mut loop_limit));
    }

    #[tokio::test]
    async fn planning_loop_exhaustion_retains_limit_and_hard_cap_stays_terminal() {
        let mut planning = TestTurnProcessingBacking::new(4).await;
        planning.activate_planning_for_test();
        let mut limit = 7;
        let mut ctx = planning.turn_loop_context();
        assert!(matches!(
            super::maybe_handle_tool_loop_limit(&mut ctx, 7, &mut limit).await.unwrap(),
            super::ToolLoopLimitAction::ContinueLoop
        ));
        assert_eq!(limit, UNLIMITED_TOOL_LOOPS);
        assert_eq!(
            ctx.harness_state.budget_recovery_reason(),
            Some("Tool loop budget exhausted before a final response (7/7)")
        );
        assert!(ctx.harness_state.recovery_is_tool_free());

        let mut build = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.tools.max_tool_loops = 5;
        build.set_vt_cfg_for_test(cfg);
        let cap = tool_loop_hard_cap(5, false);
        let mut limit = cap;
        let mut ctx = build.turn_loop_context();
        assert!(matches!(
            super::maybe_handle_tool_loop_limit(&mut ctx, cap, &mut limit).await.unwrap(),
            super::ToolLoopLimitAction::BreakLoop
        ));
        assert_eq!(limit, cap);
        assert!(!ctx.harness_state.is_recovery_active());
    }

    #[test]
    fn stale_approved_plan_pause_response_requires_both_pause_and_unavailable_markers() {
        assert!(is_stale_approved_plan_pause_response(
            "Implementation is paused because tool use is disabled. Wait for the next turn."
        ));
        assert!(!is_stale_approved_plan_pause_response(
            "The implementation is blocked by a missing Docker daemon; no source edits are safe yet."
        ));
        assert!(!is_stale_approved_plan_pause_response(
            "Implementation is paused while I wait for the user to clarify the API contract."
        ));
    }

    #[test]
    fn fresh_turn_supersedes_expired_recovery_without_changing_history() {
        use vtcode_core::llm::provider::Message;

        let restriction = "Recovery: two assistant batches attempted inspection after the tool preview budget was exhausted. Tools are disabled for this pass.";
        let mut history = vec![
            Message::system(restriction.to_owned()),
            Message::assistant("All six steps remain blocked".to_owned()),
            Message::user("Continue the approved plan".to_owned()),
        ];
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].content.as_text(), restriction);
        assert_eq!(history[3].role, MessageRole::System);
        let fresh = history[3].content.as_text();
        assert!(fresh.contains("restrictions have expired"));
        assert!(fresh.contains("permission checks"));
        assert!(fresh.contains("targeted read or small spool range"));

        history.push(Message::user("Continue the next step".to_owned()));
        let restored_len = history.len();
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), restored_len, "already superseded restrictions must not duplicate guidance");
        history.push(Message::system(tool_loop_limit_recovery_reason(60)));
        history.push(Message::user("Retry after the new recovery".to_owned()));
        super::restore_fresh_turn_tool_guidance(&mut history, true);
        assert_eq!(history.len(), restored_len + 2, "active recovery still takes precedence");
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), restored_len + 3, "a newer restriction needs one new restoration");
        super::restore_fresh_turn_tool_guidance(&mut history, false);
        assert_eq!(history.len(), restored_len + 3);

        let mut active = vec![Message::system(restriction.to_owned())];
        super::restore_fresh_turn_tool_guidance(&mut active, true);
        assert_eq!(active.len(), 1, "current recovery must still disable tools");

        let mut policy = vec![Message::system(
            "exec_command is denied by permission policy".to_owned(),
        )];
        super::restore_fresh_turn_tool_guidance(&mut policy, false);
        assert_eq!(policy.len(), 1, "permission restrictions must not be superseded");

        let mut user_text = vec![Message::user(restriction.to_owned())];
        super::restore_fresh_turn_tool_guidance(&mut user_text, false);
        assert_eq!(user_text.len(), 1, "user text is not a harness recovery event");

        let mut quoted_evidence = vec![Message::system(
            "Planning recovery synthesis: gather evidence.\n<bounded_recovery_evidence>tools are disabled</bounded_recovery_evidence>".to_owned(),
        )];
        super::restore_fresh_turn_tool_guidance(&mut quoted_evidence, false);
        assert_eq!(quoted_evidence.len(), 1, "quoted evidence must not become a runtime restriction");
    }

    #[test]
    fn fresh_turn_restores_generated_budget_and_post_tool_recovery_directives() {
        use crate::agent::runloop::unified::run_loop_context::{ToolBudgetExhaustion, ToolWallClockExhaustion};
        use crate::agent::runloop::unified::turn::turn_loop::{
            POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE,
            RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
        };
        use vtcode_core::llm::provider::Message;

        for directive in [
            tool_loop_limit_recovery_reason(60),
            ToolBudgetExhaustion { used: 4, max: 4, remaining: 0 }.synthesis_directive_message(),
            ToolWallClockExhaustion { max_secs: 600 }.synthesis_directive_message(),
            POST_TOOL_RECOVERY_REASON.to_owned(),
            POST_TOOL_RECOVERY_REASON_PLAN_MODE.to_owned(),
            RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE.to_owned(),
            crate::agent::runloop::unified::planning_workflow::build_plan_repair_directive("Invalid plan"),
        ] {
            let mut history = vec![Message::system(directive.clone())];
            super::restore_fresh_turn_tool_guidance(&mut history, false);
            assert_eq!(history.len(), 2, "must restore production directive: {directive}");
            assert_eq!(history[0].content.as_text(), directive);
            super::restore_fresh_turn_tool_guidance(&mut history, false);
            assert_eq!(history.len(), 2);
        }
    }

    #[test]
    fn internal_harness_follow_ups_stay_quiet_while_user_steering_echoes() {
        use crate::agent::runloop::unified::turn::session_loop::{
            BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, VERIFICATION_AUTO_RECOVERY_PREFIX,
        };
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
            PLAN_MODE_AUTO_CONTINUE_MARKER, RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX,
            TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
        };

        assert!(is_internal_harness_follow_up(
            format!("{TRACKER_CONTINUE_FOLLOW_UP_PREFIX} #1 do X. This follow-up is the harness resuming the work.")
                .as_str()
        ));
        assert!(is_internal_harness_follow_up(
            format!("{PLAN_MODE_AUTO_CONTINUE_MARKER} no validated persisted plan is ready for approval yet.").as_str()
        ));
        assert!(is_internal_harness_follow_up(
            format!("{RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX} Turn ended with a recovery fallback.").as_str()
        ));
        assert!(is_internal_harness_follow_up(
            format!("{BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX} and continue.").as_str()
        ));
        assert!(is_internal_harness_follow_up(
            format!("{VERIFICATION_AUTO_RECOVERY_PREFIX} Verification is still pending.").as_str()
        ));
        assert!(!is_internal_harness_follow_up("leftover"));
        assert!(!is_internal_harness_follow_up("please keep going with the build"));
    }

    #[test]
    fn extract_turn_config_honors_request_user_input_setting_in_planning_workflow() {
        let mut cfg = VTCodeConfig::default();
        cfg.chat.ask_questions.enabled = false;

        let turn_cfg = extract_turn_config(Some(&cfg), true, true);
        assert!(!turn_cfg.request_user_input_enabled);
    }

    #[tokio::test]
    async fn steering_follow_up_inputs_apply_mid_turn_in_order() {
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
        let steered: Vec<String> = working_history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| message.content.as_text().to_string())
            .collect();
        assert_eq!(steered, vec!["first".to_string(), "second".to_string()]);
        // Every steered message is tagged with its intent id so restart
        // recovery can dedupe it.
        assert!(working_history.iter().any(|message| {
            message.role == MessageRole::User
                && message
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.intent_id())
                    .is_some_and(|intent_id| !intent_id.is_empty())
        }));
        // Applied intents move to the in-flight queue and stay in the pending
        // snapshot until the post-turn checkpoint acknowledges them.
        let snapshot: Vec<String> = backing
            .pending_follow_up_intents_snapshot()
            .iter()
            .map(|intent| intent.text().to_string())
            .collect();
        assert_eq!(snapshot, vec!["first".to_string(), "second".to_string()]);
        assert_eq!(backing.deferred_follow_up_inputs(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn leftover_steering_intent_applies_without_channel_messages() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        backing.set_steering_receiver(receiver);
        backing.queue_follow_up_input_for_test("leftover");

        let mut working_history = Vec::new();
        let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
        let handled = {
            let mut ctx = backing.turn_loop_context();
            handle_steering_messages(&mut ctx, &mut working_history, &mut result)
                .await
                .expect("handle steering")
        };

        assert!(!handled);
        let steered: Vec<String> = working_history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| message.content.as_text().to_string())
            .collect();
        assert_eq!(steered, vec!["leftover".to_string()]);
        assert_eq!(backing.deferred_follow_up_inputs(), Vec::<String>::new());
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
        let steered: Vec<String> = working_history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| message.content.as_text().to_string())
            .collect();
        assert_eq!(steered, vec!["refine search".to_string()]);
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
        let steered: Vec<String> = working_history
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .map(|message| message.content.as_text().to_string())
            .collect();
        assert_eq!(steered, vec!["use the queued note".to_string()]);
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
