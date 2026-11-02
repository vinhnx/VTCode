//! Turn-loop usage accounting and cost estimation helpers.

use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::usage_cost;
use vtcode_core::utils::ansi::MessageStyle;

use super::post_tool_recovery::{PlanRecoveryEventContext, complete_turn_after_failed_tool_free_recovery_with_events};
use crate::agent::runloop::unified::turn::context::{TurnLoopResult, TurnProcessingContext};

pub(super) fn accumulate_turn_usage(provider: &str, total: &mut HarnessUsage, usage: Option<&uni::Usage>) {
    let Some(usage) = usage else {
        return;
    };

    total.add(&usage_cost::normalized_turn_usage(provider, usage));
}

pub(super) fn has_turn_usage(usage: &HarnessUsage) -> bool {
    usage.input_tokens > 0
        || usage.cached_input_tokens > 0
        || usage.cache_creation_tokens > 0
        || usage.output_tokens > 0
}

pub(super) fn stop_reason_from_finish_reason(finish_reason: &uni::FinishReason) -> String {
    match finish_reason {
        uni::FinishReason::Stop => "end_turn".to_string(),
        uni::FinishReason::Length => "max_tokens".to_string(),
        uni::FinishReason::ToolCalls => "tool_calls".to_string(),
        uni::FinishReason::ContentFilter => "content_filter".to_string(),
        uni::FinishReason::Pause => "pause_turn".to_string(),
        uni::FinishReason::Refusal => "refusal".to_string(),
        uni::FinishReason::Error(message) => message.clone(),
    }
}

pub(super) fn estimate_session_costs(
    provider: &str,
    model: &str,
    usage: &HarnessUsage,
) -> Option<usage_cost::SessionCostEstimate> {
    usage_cost::estimate_session_costs(provider, model, usage)
}

/// Enforce the session USD budget after a response: record cost, warn once,
/// and return `Some` break result when the budget is exhausted or
/// unenforceable.
pub(super) async fn enforce_response_budget(
    turn_processing_ctx: &mut TurnProcessingContext<'_>,
    provider_name: &str,
    active_model: &str,
    response_usage: Option<&uni::Usage>,
) -> Option<TurnLoopResult> {
    let max_budget_usd = turn_processing_ctx.vt_cfg.and_then(|cfg| cfg.agent.harness.max_budget_usd);
    let turn_cost = response_usage.and_then(|usage| {
        let normalized = usage_cost::normalized_turn_usage(provider_name, usage);
        estimate_session_costs(provider_name, active_model, &normalized)
    });
    match turn_processing_ctx.session_stats.record_cost(turn_cost) {
        Some(estimate) => {
            // Keep the conservative raw figure for enforcement while the
            // cache-aware effective figure is shown to users.
            let threshold = turn_processing_ctx
                .vt_cfg
                .map(|cfg| cfg.agent.harness.budget_warning_threshold)
                .unwrap_or(usage_cost::DEFAULT_BUDGET_WARNING_RATIO);
            match usage_cost::BudgetStatus::classify(estimate.raw_usd, max_budget_usd, threshold) {
                usage_cost::BudgetStatus::Exceeded { max, .. } => {
                    turn_processing_ctx
                        .session_stats
                        .mark_budget_limit_reached(max, estimate.raw_usd);
                    turn_processing_ctx.context_manager.update_token_usage(response_usage);
                    #[cfg(debug_assertions)]
                    turn_processing_ctx.context_manager.validate_token_tracking(response_usage);
                    // In planning mode, preserve the session and use the same
                    // user-facing recovery fallback as post-tool failures.
                    // Returning a model-only directive here would leave no
                    // synthesis call to consume it and would dead-end the turn.
                    if turn_processing_ctx.is_planning_active() {
                        turn_processing_ctx.plan_session.mark_budget_exhausted();
                        let plan_state = turn_processing_ctx.tool_registry.planning_workflow_state();
                        let event_thread_id = turn_processing_ctx.harness_state.run_id.0.clone();
                        let event_turn_id = turn_processing_ctx.harness_state.turn_id.0.clone();
                        let event_context = PlanRecoveryEventContext {
                            emitter: turn_processing_ctx.harness_emitter,
                            thread_id: &event_thread_id,
                            turn_id: &event_turn_id,
                        };
                        let break_result = complete_turn_after_failed_tool_free_recovery_with_events(
                            turn_processing_ctx.working_history,
                            "turn_loop.budget_limit_planning",
                            None,
                            None,
                            Some(turn_processing_ctx.plan_session),
                            Some(&plan_state),
                            Some(event_context),
                            Some(&mut *turn_processing_ctx.harness_state),
                        )
                        .await;
                        let _ = turn_processing_ctx.renderer.line(
                        MessageStyle::Warning,
                        "Budget exhausted during planning workflow. The plan draft is preserved for approval or revision.",
                    );
                        return Some(break_result);
                    } else {
                        return Some(TurnLoopResult::Blocked {
                            reason: Some(format!(
                                "Stopped after reaching budget limit (max: ${max:.4}, spent: ${:.4}, cache-adjusted: ${:.4}).",
                                estimate.raw_usd, estimate.effective_usd
                            )),
                        });
                    }
                }
                usage_cost::BudgetStatus::Warning { max, .. }
                    if !turn_processing_ctx.session_stats.budget_warning_emitted() =>
                {
                    turn_processing_ctx.session_stats.mark_budget_warning_emitted();
                    let _ = turn_processing_ctx.renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Session cost ${:.4} has reached {:.0}% of the ${max:.2} budget. {}",
                            estimate.raw_usd,
                            threshold * 100.0,
                            turn_processing_ctx.session_stats.total_usage().cache_summary(),
                        ),
                    );
                }
                _ => {}
            }
        }
        None => {
            if max_budget_usd.is_some() {
                let reason = format!(
                    "Cannot enforce session USD budget for `{provider_name}/{active_model}`: complete valid pricing metadata is unavailable"
                );
                return Some(TurnLoopResult::Blocked { reason: Some(reason) });
            }
        }
    }

    None
}

/// Record per-response usage: turn totals, session stats, prompt pressure,
/// cache health, and the stop reason.
pub(super) fn record_response_usage(
    turn_processing_ctx: &mut TurnProcessingContext<'_>,
    provider_name: &str,
    turn_usage: &mut HarnessUsage,
    response_usage: Option<&uni::Usage>,
    finish_reason: &uni::FinishReason,
) {
    accumulate_turn_usage(provider_name, &mut *turn_usage, response_usage);
    turn_processing_ctx.session_stats.record_usage(provider_name, response_usage);
    // Record prompt pressure immediately so mid-turn readers (e.g. the
    // plan-approval modal's `Context: N% used` subtitle) see the current
    // response's prompt tokens instead of the previous turn's stale value.
    // The later `update_token_usage` before continue/break is idempotent.
    turn_processing_ctx.context_manager.update_token_usage(response_usage);
    // SEV-style prompt-cache health: a sustained hit-rate collapse warns
    // once per session instead of silently re-paying full input cost.
    if let Some(message) = turn_processing_ctx
        .session_stats
        .record_cache_turn_health(provider_name, response_usage)
    {
        tracing::warn!("{message}");
        let _ = turn_processing_ctx.renderer.line(MessageStyle::Warning, &message);
    }
    turn_processing_ctx
        .session_stats
        .set_stop_reason(Some(stop_reason_from_finish_reason(finish_reason)));
}
