//! Agent Legibility:
//! - Entrypoint: `run_turn_loop` coordinates the per-turn request, recovery, tool execution, and completion flow.
//! - Common changes:
//!   - Main loop policy and break/continue rules stay in this root.
//!   - Post-tool recovery, usage accounting, and completion notification helpers live in `turn_loop/` support modules.
//! - Constraints: Preserve turn-phase transitions and recovery semantics when moving helpers out of the root.
//! - Verify: `cargo nextest run --locked -p vtcode -E 'test(turn_loop)'`

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::runloop::unified::session_settings::SessionSettingsControl;
use crate::agent::runloop::welcome::SessionBootstrap;
use anyhow::Result;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedReceiver;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::runtime::RuntimeSteering;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHeaderContext;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::unified::inline_events::harness::{HarnessEventEmitter, turn_started_event};
use crate::agent::runloop::unified::planning_workflow::maybe_handle_planning_exit_trigger;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, RunLoopContext, TurnPhase};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    ToolLoopLimitAction, extract_turn_config, handle_steering_messages, initial_tool_loop_limit,
    maybe_handle_planning_enter_trigger, maybe_handle_tool_loop_limit, resolve_safety_tool_call_limits,
};

#[path = "turn_loop/compaction.rs"]
mod compaction;
#[path = "turn_loop/context.rs"]
mod context;
#[path = "turn_loop/error_arms.rs"]
mod error_arms;
#[path = "turn_loop/finalization.rs"]
mod finalization;
#[path = "turn_loop/loop_guards.rs"]
mod loop_guards;
pub(crate) use context::{ActiveSettingsContext, TurnLoopContext, TurnLoopOutcome, effective_vt_cfg};
use error_arms::{handle_llm_request_error, handle_response_parse_error, handle_result_handler_error};
use finalization::{convert_fallback_completion, finalize_turn, normalize_recovery_break};
use loop_guards::handle_stale_approved_plan_pause;
use settings::{apply_mid_turn_mode_switch, apply_mid_turn_settings};
#[path = "turn_loop/notifications.rs"]
mod notifications;
#[path = "turn_loop/post_tool_recovery/mod.rs"]
mod post_tool_recovery;
#[path = "turn_loop/recovery_compaction.rs"]
mod recovery_compaction;
#[path = "turn_loop/responses.rs"]
mod responses;

// Response-text surface extracted to `turn_loop/responses.rs`; re-exported so
// `result_handler`, timeline views, orchestration, `post_tool_recovery`, and
// `turn_loop/tests` keep their existing paths.
pub(crate) use responses::{
    ASSISTANT_TEXT_RESPONSE_CAP_REASON, PENDING_VERIFICATION_BLOCK_REASON, POST_TOOL_RECOVERY_REASON,
    POST_TOOL_RECOVERY_REASON_PLAN_MODE, POST_TOOL_RESUME_DIRECTIVE, POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE,
    UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT, budget_recovery_final_response, is_plan_entry_handoff,
};
/// Test-only response surface for `turn_loop/tests` (unused in normal builds).
#[cfg(test)]
pub(crate) use responses::{
    COMPLETED_TURN_FALLBACK_RESPONSE, PLANNING_COMPLETED_FALLBACK_RESPONSE,
    RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE, blocked_turn_final_response, completed_fallback_reason,
    format_blocked_turn_final_response,
};
use responses::{
    RECOVERY_TOOL_CALL_RETRY_DIRECTIVE, completed_turn_requires_final_response, ensure_blocked_turn_response,
    ensure_completed_turn_response, is_primary_agent_handoff,
};
#[path = "turn_loop/settings.rs"]
mod settings;
#[path = "turn_loop/text_cap.rs"]
mod text_cap;
use text_cap::enforce_text_response_cap;
#[path = "turn_loop/usage_accounting.rs"]
mod usage_accounting;

// Using `tool_output_handler::handle_pipeline_output_from_turn_ctx` adapter where needed

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{ANTI_BLIND_EDITING_DIRECTIVE, LoopTracker};
#[cfg(test)]
use post_tool_recovery::POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON;
#[cfg(test)]
use post_tool_recovery::PostToolFailureRecovery;
#[cfg(test)]
use post_tool_recovery::maybe_recover_after_post_tool_llm_failure;
#[cfg(test)]
use post_tool_recovery::normalize_tool_free_recovery_break_outcome;
#[cfg(test)]
use recovery_compaction::current_turn_preserve_index;
#[cfg(test)]
use usage_accounting::accumulate_turn_usage;
#[cfg(test)]
use usage_accounting::has_turn_usage;
use usage_accounting::{enforce_response_budget, record_response_usage};
use vtcode_core::config::types::AgentConfig;
use vtcode_core::primary_agent::ActivePrimaryAgentState;

/// Max completion tokens for the tool-free recovery synthesis pass.
///
/// Raised from 1024 → 4096: recovery synthesis must summarize the entire
/// turn's tool outputs (often dozens of file reads and searches). 1024 tokens
/// truncated substantive answers — observed in checkpoint turn_621 where a
/// launch-time analysis over ~60 messages could not fit and the pass
/// destabilized into emitting tool-call markup instead of prose.
const RECOVERY_SYNTHESIS_MAX_TOKENS: u32 = 4096;
/// Maximum number of times the recovery pass is retried when the model
/// returns tool calls (discarded) instead of text during tool-free recovery.
///
/// Raised from 2 → 3: a single transient post-tool follow-up failure (e.g. an
/// LLM stream timeout on a large context) no longer terminates the turn and
/// forces the user to nudge "continue". The extra pass only fires when the
/// model keeps emitting tool calls during a tool-free recovery window.
pub(super) const MAX_RECOVERY_RETRIES: u8 = 3;
/// Hard cap on consecutive assistant text-only responses. Without this, the
/// recovery / continuation logic can loop forever when the model has already
/// produced a substantive final answer but the system keeps re-prompting it
/// (e.g. tool-free recovery activates on a transient LLM stream timeout, the
/// model re-summarizes the same outline 4+ times, wasting context and tokens).
/// Admitted tool execution resets the streak. Blocked and malformed attempts
/// retain it because they are not productive progress; their dedicated
/// safeguards still bound those failure paths.
///
/// Observed in checkpoint turn_594: a simple "what functions/structs are
// Stays in the root (not `responses.rs`): turn-wide `pub(super)` visibility
// required by `result_handler` and `post_tool_recovery` cannot survive a move.
/// Reason set on `TurnLoopResult::Blocked` when the model emits tool calls or
/// textual tool-call markup during a tool-free recovery pass.  Shared between
/// `result_handler` (producer) and `post_tool_recovery` (consumer).
pub(super) const RECOVERY_CONTRACT_VIOLATION_REASON: &str =
    "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls.";

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

pub(crate) async fn run_turn_loop(
    working_history: &mut Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingResult};
    use crate::agent::runloop::unified::turn::guards::run_proactive_guards;
    use crate::agent::runloop::unified::turn::turn_processing::{
        HandleTurnProcessingResultParams, execute_llm_request, handle_turn_processing_result,
        maybe_force_planning_workflow_interview, process_llm_response, resolve_effective_request_model,
    };

    // Approval starts implementation with usable validation state, including
    // current-context handoffs. Keep the consumed continuation budgets intact.
    if ctx.harness_state.is_approved_plan_execution() {
        ctx.session_stats.preflight_recovery = Default::default();
    }

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let mut turn_modified_files = BTreeSet::new();
    // Snapshot the bounded session touched window so the end-of-turn
    // checkpoint records only files touched during this turn (plus every
    // modified file below), not stale entries from previous turns.
    let touched_at_turn_start: std::collections::HashSet<String> =
        ctx.session_stats.recent_touched_files().into_iter().collect();
    let mut pending_primary_agent: Option<String> = None;
    let mut pending_plan_execution_target = None;
    *ctx.auto_finish_planning_attempted = ctx.plan_session.take_recovery_input_pending()
        || ctx.plan_session.recovery.outcome == vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;

    // Compact command rows are a presentation-only active tail. A new turn
    // must start a fresh contiguous group even when the renderer is reused by
    // the session loop.
    ctx.renderer.flush_compact_command_group();
    ctx.set_phase(TurnPhase::Preparing);
    super::turn_loop_helpers::restore_fresh_turn_tool_guidance(
        working_history,
        ctx.harness_state.is_recovery_active()
            || (ctx.is_planning_active() && ctx.plan_session.recovery.research_closed),
    );
    if let Some(Err(e)) = ctx.harness_emitter.map(|e| e.emit(turn_started_event())) {
        tracing::debug!(error = %e, "harness turn_started event emission failed");
    }

    // Optimization: Extract all frequently accessed config values once
    let mut turn_config = extract_turn_config(
        effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
        ctx.is_planning_active(),
        ctx.renderer.supports_inline_ui(),
    );
    if ctx.is_planning_active() {
        ctx.plan_session.start_turn();
        // Planning work starts now: the user request exists, so promote to
        // the Planning stage and show the researching indicator once per
        // turn. Mode entry alone stays Idle to avoid a premature "Planning..."
        // footer before any request.
        crate::agent::runloop::unified::planning_workflow_state::mark_planning_turn_started(ctx.renderer, ctx.handle);
    }
    // After a permanent `request_user_input` denial, suppress the tool for the
    // rest of the session so the model stops retrying it across turns.
    if ctx.plan_session.is_interview_denied() {
        turn_config.request_user_input_enabled = false;
    }

    let mut step_count = 0;
    let mut current_max_tool_loops =
        initial_tool_loop_limit(turn_config.max_tool_loops, ctx.harness_state.is_approved_plan_execution());
    let mut turn_history_start_len = working_history.len();
    // Bounded condense-retry counter for truncated planning syntheses (see
    // the re-prompt block after generation). Prevents an oversized plan from
    // looping the turn.
    let mut plan_condense_attempts: u8 = 0;
    let mut turn_usage = HarnessUsage::default();
    // Optimization: Interned signatures with exponential backoff for loop detection
    let mut repeated_tool_attempts = LoopTracker::with_verification_snapshot(ctx.session_stats.verification_snapshot());
    if ctx.session_stats.verification_snapshot().0 {
        ctx.tool_registry.restore_completion_verification_debt();
    }
    if repeated_tool_attempts.verification_is_pending() {
        repeated_tool_attempts.verification_warning_emitted = true;
        let has_recent_verification_directive = working_history.iter().rev().take(8).any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains(ANTI_BLIND_EDITING_DIRECTIVE)
        });
        if !has_recent_verification_directive {
            working_history.push(uni::Message::system(ANTI_BLIND_EDITING_DIRECTIVE.to_string()));
        }
    }

    // Reset per-turn counters while preserving only the explicit session
    // headroom granted by the user. The validator reapplies that fuse inside
    // `set_limits`, while ordinary runtime config changes (including a lower
    // configured limit) remain authoritative when no grant exists.
    {
        let (max_per_turn, max_per_session) = resolve_safety_tool_call_limits(
            ctx.harness_state.max_tool_calls,
            turn_config.max_session_turns,
            ctx.is_planning_active(),
        );
        ctx.safety_validator.set_limits(max_per_turn, max_per_session);
        ctx.safety_validator.start_turn();
    }
    // Tracks the planning flag from the previous loop iteration so Build↔Plan
    // auto-switches mid-turn re-apply budgets in *both* directions. Raising
    // only `max(limit, floor)` is not enough: the prior mode may have already
    // consumed that cap, so the new mode needs a full floor of remaining
    // headroom from now (user report: explicit Plan works, auto-switch hits
    // the tool-call limit).
    let mut last_planning_active = ctx.is_planning_active();

    loop {
        if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state) {
            result = stop;
            break;
        }
        let settings_applied =
            apply_mid_turn_settings(&mut ctx, working_history, &mut turn_config, &mut turn_history_start_len).await?;
        if handle_steering_messages(&mut ctx, working_history, &mut result).await? {
            break;
        }
        if ctx.session_stats.navigation_recovery.blocked() {
            result = TurnLoopResult::Blocked {
                reason: Some(crate::agent::runloop::unified::state::NAVIGATION_RECOVERY_BLOCK_REASON.to_string()),
            };
            break;
        }
        if settings_applied {
            // Double-check the new model receives the updated prompt verbatim
            // before the next LLM request is built: fresh steered input must
            // sit at the tail after compaction plus resume, not summarized away.
            verify_mid_turn_prompt_received(working_history, turn_history_start_len);
        }

        // A permanent interview denial can happen after this turn's initial
        // config snapshot (for example when the model's plan response
        // triggers the inline interview). Keep the parser and request builder
        // on the same capability set immediately; otherwise this turn can
        // still inject another `request_user_input` call even though the
        // catalog has already removed it.
        if ctx.plan_session.is_interview_denied() {
            turn_config.request_user_input_enabled = false;
        }

        step_count += 1;
        ctx.telemetry.record_turn();

        if maybe_handle_planning_enter_trigger(&mut ctx, working_history, step_count, &mut result).await? {
            break;
        }

        // Build<->Plan auto-switch mid-turn (see `settings` for the starvation report).
        apply_mid_turn_mode_switch(
            &mut ctx,
            &mut last_planning_active,
            &mut turn_config,
            &mut current_max_tool_loops,
            step_count,
        );

        // The settings application and Build↔Plan mode switch above are the
        // only points that can change the request identity mid-iteration:
        // nothing after this line mutates `config.model`, the active primary
        // agent, or the live config, so derive the effective model, context
        // budget, and provider identity once and reuse them for the planning
        // exit, budget check, compaction, and telemetry below. `vt_cfg` is
        // re-read where needed: it borrows `ctx`, which is taken `&mut` by
        // the tool-loop-limit handler and request-context construction.
        let active_model = resolve_effective_request_model(&ctx.config.model, ctx.active_primary_agent.active());
        let context_budget = vtcode_core::compaction::effective_context_budget(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            ctx.provider_client.as_ref(),
            &active_model,
        );
        let provider_name = ctx.provider_client.name().to_string();

        let transition = maybe_handle_planning_exit_trigger(
            ctx.renderer,
            ctx.tool_registry,
            ctx.plan_session,
            ctx.handle,
            working_history,
            ctx.auto_finish_planning_attempted,
            crate::agent::runloop::unified::planning_workflow::PlanningExitContext {
                session: ctx.session,
                ctrl_c_state: ctx.ctrl_c_state,
                ctrl_c_notify: ctx.ctrl_c_notify,
                vt_cfg: effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
                skip_confirmations: ctx.skip_confirmations,
                full_auto: ctx.full_auto,
                context_usage_percent: ctx.context_manager.context_usage_percent(context_budget),
                telemetry: crate::agent::runloop::unified::planning_workflow::PlanApprovalTelemetryContext {
                    emitter: ctx.harness_emitter,
                    thread_id: &ctx.harness_state.run_id.0,
                    turn_id: &ctx.harness_state.turn_id.0,
                },
            },
        )
        .await?;

        if transition.should_break() {
            let (loop_result, target) = transition.into_result_and_target();
            result = loop_result;
            pending_plan_execution_target = target;
            break;
        }

        match maybe_handle_tool_loop_limit(&mut ctx, step_count, &mut current_max_tool_loops).await? {
            ToolLoopLimitAction::Proceed => {}
            ToolLoopLimitAction::ContinueLoop => continue,
            ToolLoopLimitAction::BreakLoop => {
                result = TurnLoopResult::Blocked {
                    reason: Some(format!(
                        "Tool loop budget exhausted ({step_count}/{current_max_tool_loops}); no additional synthesis pass is available. The current plan and task state were retained."
                    )),
                };
                break;
            }
        }

        // Shared config view for the budget check and compaction decisions
        // below; the borrow ends before the request context is constructed.
        let vt_cfg = effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg);
        // A configured monetary budget is an enforcement contract. Validate
        // pricing before any compaction path because native compaction can
        // itself dispatch a provider request.
        if let Some(max_budget_usd) = vt_cfg.and_then(|cfg| cfg.agent.harness.max_budget_usd)
            && let Err(error) = vtcode_core::llm::usage_cost::require_budget_pricing(
                ctx.provider_client.name(),
                &active_model,
                Some(max_budget_usd),
            )
        {
            result = TurnLoopResult::Blocked { reason: Some(error.to_string()) };
            break;
        }
        if let Some(blocked) = compaction::compact_before_request(
            &mut ctx,
            working_history,
            &mut turn_history_start_len,
            &mut repeated_tool_attempts,
            &active_model,
            context_budget,
        )
        .await
        {
            result = blocked;
            break;
        }

        // Clone validation cache arc to avoid borrow conflict
        let validation_cache = ctx.session_stats.validation_cache.clone();

        // Anti-runaway guard: cap consecutive text-only responses without
        // admitted tool progress (see `text_cap` for the observed failure mode).
        if let Some(capped) = enforce_text_response_cap(&mut ctx, &mut *working_history, turn_history_start_len) {
            result = capped;
            break;
        }

        // Prepare turn processing context
        // Capture input status state before constructing the context, which
        // mutably borrows the turn loop context for the request lifetime.
        let restore_status_left = ctx.input_status_state.left.clone();
        let restore_status_right = ctx.input_status_state.right.clone();
        let mut turn_processing_ctx = ctx.as_turn_processing_context(working_history);

        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        run_proactive_guards(&mut turn_processing_ctx, step_count).await?;

        match turn_processing_ctx.admit_planning_recovery_request(&active_model).await {
            Ok(true) => {}
            Ok(false) => {
                match turn_processing_ctx
                    .await_planning_recovery_input("The planning recovery allowance is exhausted.")
                    .await?
                {
                    TurnHandlerOutcome::Continue => continue,
                    TurnHandlerOutcome::Break(outcome) => {
                        result = outcome;
                        break;
                    }
                    _ => {
                        result = TurnLoopResult::Blocked { reason: Some("planning awaiting input".into()) };
                        break;
                    }
                }
            }
            Err(error) => {
                turn_processing_ctx.plan_session.recovery.outcome =
                    vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
                turn_processing_ctx.renderer.line(
                    MessageStyle::Error,
                    &format!(
                        "Planning could not save recovery state. Retry after fixing persistence. Details: {error}"
                    ),
                )?;
                result = TurnLoopResult::Blocked {
                    reason: Some("planning persistence failed; awaiting input".into()),
                };
                break;
            }
        }

        // Execute the LLM request
        turn_processing_ctx.set_phase(TurnPhase::Requesting);
        // `active_model` is the per-iteration resolution from above; the
        // proactive guards between there and here cannot change the model.
        let recovery_pass = turn_processing_ctx.consume_recovery_pass();

        let tool_free_recovery = recovery_pass && turn_processing_ctx.recovery_is_tool_free();
        // Capture input status state for potential restoration after a
        // pre-request block or provider failure.
        // Cache-gap advisory (Phase E1): warn once per gap when the user
        // paused long enough for the provider prompt cache to have expired,
        // so this request may unexpectedly re-pay full input cost.
        if let Some(threshold) = turn_processing_ctx.vt_cfg.and_then(|cfg| {
            cfg.prompt_cache.gap_threshold_secs_for_route(
                &provider_name,
                &active_model,
                turn_processing_ctx.provider_client.requested_prompt_cache_ttl(&active_model),
            )
        }) {
            let threshold = Duration::from_secs(threshold);
            if turn_processing_ctx.session_stats.total_usage().cached_input_tokens > 0
                && let Some(elapsed) = turn_processing_ctx.session_stats.cache_gap_exceeds(threshold)
            {
                let _ = turn_processing_ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "~{} since the last request; the provider prompt cache may have expired, so this request may rebuild cached input.",
                        vtcode_core::llm::request_gap::format_gap(elapsed)
                    ),
                );
            }
        }
        turn_processing_ctx.session_stats.note_request_sent();

        let suppress_unverified_output = turn_processing_ctx.tool_registry.completion_verification_pending();
        let request_result = if suppress_unverified_output
            || turn_processing_ctx
                .config
                .provider
                .eq_ignore_ascii_case(vtcode_core::copilot::COPILOT_PROVIDER_KEY)
        {
            turn_processing_ctx
                .execute_llm_request_with_options(
                    step_count,
                    &active_model,
                    tool_free_recovery.then_some(RECOVERY_SYNTHESIS_MAX_TOKENS),
                    tool_free_recovery,
                    None, // parallel_cfg_opt
                    suppress_unverified_output,
                    Some(&mut repeated_tool_attempts),
                )
                .await
        } else {
            execute_llm_request(
                &mut turn_processing_ctx,
                step_count,
                &active_model,
                tool_free_recovery.then_some(RECOVERY_SYNTHESIS_MAX_TOKENS),
                tool_free_recovery,
                None, // parallel_cfg_opt
            )
            .await
        };
        let (response, response_streamed) = match request_result {
            Ok(val) => val,
            Err(err) => {
                match handle_llm_request_error(
                    &mut turn_processing_ctx,
                    &err,
                    step_count,
                    turn_history_start_len,
                    restore_status_left.clone(),
                    restore_status_right.clone(),
                    tool_free_recovery,
                )
                .await?
                {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(r) => {
                        result = r;
                        break;
                    }
                }
            }
        };

        if let Some(stop) =
            crate::agent::runloop::unified::stop_requests::stop_outcome(turn_processing_ctx.ctrl_c_state)
        {
            result = stop;
            break;
        }
        turn_processing_ctx.record_planning_response(&response).await?;

        // Track turn usage and context pressure before later processing borrows `response`.
        let response_usage = response.usage.clone();
        // Usage normalization and pricing must follow the provider instance
        // that will serve the request. Configuration may contain an alias or
        // an inferred route, while the provider trait is authoritative. The
        // per-iteration `provider_name` binding is that provider's identity.
        record_response_usage(
            &mut turn_processing_ctx,
            &provider_name,
            &mut turn_usage,
            response_usage.as_ref(),
            &response.finish_reason,
        );
        if let Some(blocked) =
            enforce_response_budget(&mut turn_processing_ctx, &provider_name, &active_model, response_usage.as_ref())
                .await
        {
            result = blocked;
            break;
        }
        if !response.tool_references.is_empty() {
            turn_processing_ctx
                .tool_catalog
                .note_tool_references(turn_processing_ctx.tools, &response.tool_references)
                .await;
        }

        {
            if turn_processing_ctx.is_planning_active() {
                turn_processing_ctx.plan_session.increment_turns();
            }
        }

        // Plan-mode robustness: if the planning synthesis was truncated at the
        // model's output token limit, the emitted plan is incomplete ("cut off
        // mid-flight"). Rather than accept a partial plan or re-enter the
        // recovery path (which previously looped forever), ask for a tighter
        // spec and retry once. Bounded so a genuinely oversized plan cannot
        // loop. The policy lives in the planning-workflow facade so this loop
        // stays free of plan-mode specifics.
        let planning_active = turn_processing_ctx.is_planning_active();
        if crate::agent::runloop::unified::planning_workflow::maybe_condense_truncated_plan(
            &mut *turn_processing_ctx.working_history,
            &mut *turn_processing_ctx.renderer,
            planning_active,
            tool_free_recovery,
            &mut plan_condense_attempts,
            &response,
        ) {
            continue;
        }

        // A successful provider response may race the user's stop. Never admit
        // its tools or planning continuation after that stop is accepted.
        if let Some(stop) =
            crate::agent::runloop::unified::stop_requests::stop_outcome(turn_processing_ctx.ctrl_c_state)
        {
            result = stop;
            break;
        }
        // Process the LLM response
        let processing_result_outcome = {
            let allow_plan_interview = turn_processing_ctx.is_planning_active()
                && turn_config.request_user_input_enabled
                && crate::agent::runloop::unified::turn::turn_processing::planning_workflow_interview_ready(
                    turn_processing_ctx.session_stats,
                    turn_processing_ctx.plan_session,
                );
            process_llm_response(
                &response,
                turn_processing_ctx.renderer,
                turn_processing_ctx.working_history.len(),
                turn_processing_ctx.is_planning_active(),
                allow_plan_interview,
                turn_config.request_user_input_enabled,
                !tool_free_recovery,
                Some(&validation_cache),
                Some(turn_processing_ctx.tool_registry),
            )
        };
        let mut processing_result = match processing_result_outcome {
            Ok(result) => result,
            Err(err) => {
                match handle_response_parse_error(
                    &mut turn_processing_ctx,
                    err,
                    step_count,
                    turn_history_start_len,
                    restore_status_left.clone(),
                    restore_status_right.clone(),
                )
                .await?
                {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(r) => {
                        result = r;
                        break;
                    }
                }
            }
        };
        // When in tool-free recovery and the model returns no text (e.g. producing
        // tool calls that get discarded), retry with a more explicit directive
        // rather than immediately falling back to the deterministic final answer.
        if tool_free_recovery
            && !turn_processing_ctx.is_planning_active()
            && matches!(processing_result, TurnProcessingResult::Empty { .. })
            && response.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty())
            && turn_processing_ctx.recovery_retry_count() < MAX_RECOVERY_RETRIES
        {
            // The guard above requires `!is_planning_active()`, so the base
            // directive always applies here (no plan-mode variant).
            turn_processing_ctx
                .working_history
                .push(uni::Message::system(RECOVERY_TOOL_CALL_RETRY_DIRECTIVE.to_string()));
            turn_processing_ctx.retry_recovery_pass();
            continue;
        }
        // During the tool-free recovery pass, tools are disabled at the API
        // level, so an injected `request_user_input` interview call can never
        // be executed — it only trips the recovery contract guard and collapses
        // the turn to a dead-end fallback. Skip interview synthesis/forcing
        // here; the recovery synthesis can still produce a valid text answer.
        if turn_config.request_user_input_enabled && !tool_free_recovery && turn_processing_ctx.is_planning_active() {
            processing_result = maybe_force_planning_workflow_interview(
                processing_result,
                response.content.as_deref(),
                turn_processing_ctx.session_stats,
                turn_processing_ctx.plan_session,
                turn_processing_ctx.working_history.len(),
            );
        }

        if let Some(flow) = handle_stale_approved_plan_pause(
            &mut turn_processing_ctx,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
            &processing_result,
            step_count,
            current_max_tool_loops,
            turn_config.tool_repeat_limit,
        )
        .await?
        {
            match flow {
                ControlFlow::Continue(()) => continue,
                ControlFlow::Break(result_) => {
                    result = result_;
                    break;
                }
            }
        }

        // Restore input status if there are no tool calls (turn is completing)
        // This handles the case where defer_restore was set but no tool spinners will take over
        let has_tool_calls = matches!(processing_result, TurnProcessingResult::ToolCalls { .. });
        if !has_tool_calls {
            turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);
        }

        if has_tool_calls {
            turn_processing_ctx.set_phase(TurnPhase::ExecutingTools);
        } else {
            turn_processing_ctx.set_phase(TurnPhase::Finalizing);
        }

        // Handle the turn processing result (dispatch tool calls or finish turn)
        let turn_outcome_result = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut turn_processing_ctx,
            processing_result,
            response_streamed,
            step_count,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: current_max_tool_loops,
            tool_repeat_limit: turn_config.tool_repeat_limit,
        })
        .await;
        let turn_outcome = match turn_outcome_result {
            Ok(outcome) => outcome,
            Err(err) => {
                match handle_result_handler_error(&mut ctx, working_history, err, step_count, turn_history_start_len)
                    .await?
                {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(r) => {
                        result = r;
                        break;
                    }
                }
            }
        };
        // Record token usage before continuing or breaking
        ctx.context_manager.update_token_usage(response_usage.as_ref());
        #[cfg(debug_assertions)]
        ctx.context_manager.validate_token_tracking(response_usage.as_ref());

        match turn_outcome {
            TurnHandlerOutcome::Continue => continue,
            TurnHandlerOutcome::SwitchPrimaryAgent(agent) => {
                // Primary-agent handoff after the turn. Plan *entry* selects the
                // plan agent without claiming an approved-plan execution turn;
                // plan→build/auto still uses the approved-plan path below.
                let plan_entry = is_plan_entry_handoff(&agent);
                pending_primary_agent = Some(agent);
                result = TurnLoopResult::Completed { plan_approved_execution_pending: !plan_entry };
                break;
            }
            TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target } => {
                pending_plan_execution_target = Some(target);
                result = TurnLoopResult::Completed { plan_approved_execution_pending: true };
                break;
            }
            TurnHandlerOutcome::BreakWithPolicy { result: outcome_result, target } => {
                pending_plan_execution_target = Some(target);
                result = outcome_result;
                break;
            }
            TurnHandlerOutcome::Break(outcome_result) => {
                match normalize_recovery_break(&mut ctx, working_history, outcome_result, tool_free_recovery).await {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(r) => {
                        result = r;
                        break;
                    }
                }
            }
        }
    }

    if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state) {
        result = stop;
        pending_primary_agent = None;
        pending_plan_execution_target = None;
    }
    if !ctx.is_planning_active()
        && matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: false })
        && ctx.tool_registry.completion_verification_pending()
    {
        repeated_tool_attempts.mark_verification_pending();
        ctx.session_stats
            .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());
        result = TurnLoopResult::Blocked {
            reason: Some(ctx.tool_registry.unverified_completion_reason()),
        };
    }
    // A terminal or failed synthesis cannot leave a stale resume grant for an internal follow-up.
    ctx.session_stats.navigation_recovery.consume();
    ctx.session_stats.preflight_recovery.finish_corrective_turn();
    if ctx.session_stats.preflight_recovery.exhausted()
        && !ctx.harness_state.turn_refused()
        && matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: false })
        && pending_primary_agent.is_none()
        && pending_plan_execution_target.is_none()
    {
        result = TurnLoopResult::Blocked {
            reason: Some("Automatic preflight validation recovery stopped after one corrective turn without a successful correction. Type continue to resume with corrected arguments.".to_string()),
        };
    }
    if let TurnLoopResult::Blocked { reason } = &result {
        ensure_blocked_turn_response(
            &mut ctx,
            working_history,
            turn_history_start_len,
            reason.as_deref().unwrap_or("blocked"),
        )?;
    }

    // An approved-plan handoff or plan-entry handoff is a completed control-
    // flow turn, not a user-visible assistant turn. Its follow-up request is
    // constructed by the outer session loop, so requiring a final assistant
    // response here would convert the handoff into `Blocked` before the
    // implementation/planning turn can start.
    //
    // Consume the deferred plan-entry switch *before* the guard: mid-turn
    // `start_planning` queues it without breaking the turn, and the handoff
    // must be exempt like an explicit `SwitchPrimaryAgent("plan")`.
    let deferred_plan_entry_switch = ctx.plan_session.take_plan_entry_agent_switch()
        && !matches!(result, TurnLoopResult::Cancelled | TurnLoopResult::Exit);
    let primary_agent_handoff = is_primary_agent_handoff(
        pending_primary_agent.as_deref(),
        pending_plan_execution_target.as_ref(),
        deferred_plan_entry_switch,
    );
    let final_response_was_fallback = if completed_turn_requires_final_response(&result, primary_agent_handoff) {
        ensure_completed_turn_response(&mut ctx, working_history, turn_history_start_len)?
    } else {
        ctx.harness_state.final_response_was_fallback()
    };
    convert_fallback_completion(
        &ctx,
        &mut result,
        primary_agent_handoff,
        final_response_was_fallback,
        working_history,
        turn_history_start_len,
    );

    // Apply the deferred plan-entry agent switch at this turn boundary unless
    // a stronger handoff (approved-plan policy target / explicit switch) owns
    // the boundary. Apply on Blocked too: plan mode often blocks tools in the
    // entry turn, and discarding here would leave the build agent selected
    // forever while planning stays active.
    if deferred_plan_entry_switch {
        if pending_primary_agent.is_none() && pending_plan_execution_target.is_none() {
            pending_primary_agent =
                Some(crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME.to_string());
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                agent = %crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME,
                "Applying deferred plan primary-agent switch after turn boundary"
            );
        } else {
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                has_primary_agent = pending_primary_agent.is_some(),
                has_plan_execution_target = pending_plan_execution_target.is_some(),
                "Discarding deferred plan primary-agent switch; stronger handoff owns the boundary"
            );
        }
    }

    ctx.renderer.flush_compact_command_group();
    ctx.set_phase(TurnPhase::Finalizing);
    finalize_turn(&mut ctx, working_history, &result, &turn_usage).await;

    // Final outcome with the correct result status
    ctx.session_stats.record_turn_completed();
    let plan_approved_execution_pending =
        matches!(&result, TurnLoopResult::Completed { plan_approved_execution_pending: true });
    ctx.session_stats
        .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());
    let in_progress_sessions = if matches!(result, TurnLoopResult::Cancelled | TurnLoopResult::Exit) {
        Vec::new()
    } else {
        crate::agent::runloop::unified::stop_requests::await_with_stop(
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            ctx.tool_registry.in_progress_exec_sessions(4),
        )
        .await
        .unwrap_or_default()
    };
    let turn_diagnostics = ctx
        .harness_state
        .snapshot_turn_diagnostics(turn_usage.clone(), repeated_tool_attempts.low_signal_tool_calls)
        .with_in_progress_exec_sessions(in_progress_sessions);
    // Touched files include reads/searches recorded during this turn plus
    // every modified file, so checkpoint replay shows evidence even for
    // read-only turns without snapshotting file contents. `recent_touched_files`
    // is a bounded session window (last 5), so turns touching more than 5 new
    // files keep only the most recent; that is an intentional replay-context
    // bound, not a per-turn isolation guarantee.
    let mut turn_touched_files: BTreeSet<PathBuf> = turn_modified_files.clone();
    for touched in ctx.session_stats.recent_touched_files() {
        let trimmed = touched.trim();
        if !trimmed.is_empty() && !touched_at_turn_start.contains(trimmed) {
            turn_touched_files.insert(PathBuf::from(trimmed));
        }
    }
    Ok(TurnLoopOutcome {
        result,
        turn_modified_files,
        turn_touched_files,
        turn_diagnostics,
        pending_primary_agent,
        pending_plan_execution_target,
        plan_approved_execution_pending,
        final_response_was_fallback,
        refused: ctx.harness_state.turn_refused(),
    })
}

/// Verify the new model actually receives the updated mid-turn prompt verbatim.
///
/// Called right after a mid-turn settings/model switch plus steering delivery,
/// before the next LLM request is built. Fresh steered user input must sit at
/// the history tail after compaction plus the resume note — never summarized
/// away. Logs the outcome for trajectory diagnostics; the TUI echo for the
/// steered prompt already happened in `handle_steering_messages`, so this stays
/// log-only to avoid duplicate transcript noise.
fn verify_mid_turn_prompt_received(working_history: &[uni::Message], turn_history_start_len: usize) {
    let start = turn_history_start_len.min(working_history.len());
    let tail = working_history.get(start..).unwrap_or(&[]);
    if let Some(latest_user) = tail.iter().rev().find(|m| m.role == uni::MessageRole::User) {
        let preview: String = latest_user.content.as_text().chars().take(120).collect();
        tracing::info!(
            prompt_preview = %preview,
            tail_len = tail.len(),
            history_len = working_history.len(),
            "Verified mid-turn prompt reaches new model verbatim"
        );
    } else {
        tracing::info!(
            history_len = working_history.len(),
            "Model switched mid-turn with no fresh user prompt in current turn; compacted context plus resume note retained"
        );
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hypothesis_revision_tests;
