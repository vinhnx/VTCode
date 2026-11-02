//! Request-boundary compaction orchestration with bounded recovery.
//!
//! The loop enforces pricing and decides whether to stop. This helper owns the
//! scoped progress row, engine dispatch, history-index rebasing and diagnostics.

use std::time::Instant;

use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use super::post_tool_recovery::{POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON, ensure_post_tool_resume_directive};
use super::recovery_compaction::{RecoveryCompactionRequest, compact_before_tool_enabled_retry};
use super::{TurnLoopContext, effective_vt_cfg};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;

pub(super) async fn compact_before_request(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    turn_history_start_len: &mut usize,
    repeated_tool_attempts: &mut LoopTracker,
    active_model: &str,
    context_budget: usize,
) -> Option<TurnLoopResult> {
    let vt_cfg = effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg);
    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
    let steering_update = vtcode_core::compaction::memory_envelope::SessionMemoryEnvelopeUpdate {
        pending_intents: Some(ctx.runtime_steering.pending_follow_up_intents_snapshot()),
        applied_intent_ids: ctx.runtime_steering.applied_follow_up_intent_ids().iter().cloned().collect(),
        ..Default::default()
    };
    let recovery_compaction_requested = ctx.harness_state.take_post_tool_compaction_pending();
    if recovery_compaction_requested {
        let context_capacity_failure = ctx.harness_state.post_tool_context_capacity_failure();
        // The transcript row stays owned while recovery compacts the older prefix.
        let progress = crate::agent::runloop::unified::turn::compaction::CompactionProgressGuard::start(
            ctx.handle,
            crate::agent::runloop::unified::turn::compaction::estimate_history_tokens(working_history),
        );
        let recovery_result = compact_before_tool_enabled_retry(RecoveryCompactionRequest {
            history: working_history,
            turn_history_start_len,
            compaction_context: crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                ctx.provider_client.as_ref(),
                active_model,
                &harness_snapshot.session_id,
                &ctx.harness_state.run_id.0,
                &ctx.config.workspace,
                vt_cfg,
                ctx.lifecycle_hooks,
                ctx.harness_emitter,
            ),
            session_stats: ctx.session_stats,
            context_manager: ctx.context_manager,
            steering_update,
        })
        .await;
        let recovery_elapsed = progress.finish();
        match recovery_result {
            Ok(Some(outcome)) => {
                repeated_tool_attempts.clear_navigation_evidence();
                let label = crate::agent::runloop::unified::turn::compaction::format_compacted_summary(
                    outcome.estimated_tokens_before,
                    outcome.estimated_tokens_after,
                    outcome.mode.as_str(),
                    recovery_elapsed,
                );
                let _ = ctx.renderer.line(MessageStyle::Info, &label);
            }
            Ok(None) if context_capacity_failure => {
                let _ = ctx.renderer.line(
                    MessageStyle::Warning,
                    "Recovery compaction found no reducible history; the current history was preserved.",
                );
                ctx.harness_state.mark_post_tool_context_compaction_failed();
                ensure_post_tool_resume_directive(working_history);
                return Some(TurnLoopResult::Blocked {
                    reason: Some(POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()),
                });
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    error = %crate::agent::runloop::unified::turn::compaction::format_compaction_error_details(&err),
                    "Post-tool recovery compaction failed; preserving the existing history for the bounded retry"
                );
                let prefix =
                    crate::agent::runloop::unified::turn::compaction::format_compaction_failed(recovery_elapsed, &err);
                if context_capacity_failure {
                    let _ = ctx.renderer.line(MessageStyle::Error, &prefix);
                    ctx.harness_state.mark_post_tool_context_compaction_failed();
                    ensure_post_tool_resume_directive(working_history);
                    return Some(TurnLoopResult::Blocked {
                        reason: Some(POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()),
                    });
                }
                let _ = ctx.renderer.line(
                    MessageStyle::Warning,
                    &format!("{prefix} Continuing with full history for the bounded retry."),
                );
            }
        }
    } else {
        tracing::info!(
            model = %active_model,
            context_budget,
            prompt_tokens = ctx.context_manager.current_token_usage(),
            "Resolved per-turn context budget denominator"
        );
        // Only show the progress row when compaction is likely to run.
        // `maybe_auto_compact_history` returns `None` fast below threshold,
        // when disabled, or when suppressed; starting a progress row
        // unconditionally would flicker on every turn.
        let auto_start = Instant::now();
        let auto_compaction_allowed = vt_cfg.is_some_and(|cfg| cfg.agent.harness.auto_compaction_enabled)
            && ctx.session_stats.auto_compact_suppressed == vtcode_core::compaction::SUPPRESS_NONE;
        let auto_threshold = crate::agent::runloop::unified::turn::compaction::effective_compaction_threshold(
            vt_cfg,
            ctx.provider_client.as_ref(),
            active_model,
        );
        let auto_likely = auto_compaction_allowed
            && (ctx.context_manager.compaction_pending()
                || auto_threshold.is_some_and(|threshold| ctx.context_manager.current_token_usage() >= threshold));
        let auto_progress = auto_likely.then(|| {
            crate::agent::runloop::unified::turn::compaction::CompactionProgressGuard::start(
                ctx.handle,
                crate::agent::runloop::unified::turn::compaction::estimate_history_tokens(working_history),
            )
        });
        let auto_result = crate::agent::runloop::unified::turn::compaction::maybe_auto_compact_history(
            crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                ctx.provider_client.as_ref(),
                active_model,
                &harness_snapshot.session_id,
                &ctx.harness_state.run_id.0,
                &ctx.config.workspace,
                vt_cfg,
                ctx.lifecycle_hooks,
                ctx.harness_emitter,
            ),
            crate::agent::runloop::unified::turn::compaction::CompactionState::new(
                working_history,
                ctx.session_stats,
                ctx.context_manager,
            )
            .with_steering_update(steering_update),
        )
        .await;
        let auto_elapsed = auto_progress
            .map(|guard| guard.finish())
            .unwrap_or_else(|| auto_start.elapsed());
        match auto_result {
            Ok(Some(outcome)) => {
                repeated_tool_attempts.clear_navigation_evidence();
                *turn_history_start_len = outcome.compacted_len;
                tracing::info!(
                    original_len = outcome.original_len,
                    compacted_len = outcome.compacted_len,
                    turn_history_start_len = *turn_history_start_len,
                    "Applied local fallback compaction before the next turn request"
                );
                let label = crate::agent::runloop::unified::turn::compaction::format_compacted_summary(
                    outcome.estimated_tokens_before,
                    outcome.estimated_tokens_after,
                    outcome.mode.as_str(),
                    auto_elapsed,
                );
                let _ = ctx.renderer.line(MessageStyle::Info, &label);
                // Continuation: do not break. The loop falls through to the
                // next LLM request using the compacted handoff (summary +
                // envelope + continuity tail) so the task resumes.
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    error = %crate::agent::runloop::unified::turn::compaction::format_compaction_error_details(&err),
                    "Automatic context compaction failed; continuing with full history"
                );
                let label =
                    crate::agent::runloop::unified::turn::compaction::format_compaction_failed(auto_elapsed, &err);
                let _ = ctx
                    .renderer
                    .line(MessageStyle::Warning, &format!("{label} Continuing with full history."));
            }
        }
    }

    None
}
