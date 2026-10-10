use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use vtcode_core::core::agent::harness_kernel::{PreparedToolBatch, PreparedToolBatchKind};

use super::{
    PreparedToolCall, ToolOutcomeContext, ValidationTransition, block_mutation_until_verification,
    finalize_validation_result, flush_budget_synthesis_directives, validate_tool_call,
};
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::{
    emit_tool_completion_for_status, emit_tool_outcome_observation, emit_tool_start_if_needed,
    exec_settlement_mode_for_tool_call, execute_prevalidated_read_only_with_cache, resolve_harness_item_identity,
    run_tool_call_with_args,
};
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnProcessingContext,
};
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::{
    handle_tool_execution_result, handle_tool_execution_result_with_spinner,
};
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    PIPED_VERIFICATION_DIRECTIVE, PIPED_VERIFICATION_WARNING, VERIFICATION_RESULT_LOST_DIRECTIVE,
    VERIFICATION_RESULT_LOST_WARNING, resolve_max_tool_retries, update_repetition_tracker,
};

const DEFAULT_MAX_PARALLEL_TOOL_CALLS: usize = 4;

/// Surface queued loop directives after the tool response lands.
/// `update_repetition_tracker` queues them when a pending gate's verifier
/// result was lost (verifier-level Failure/Timeout, or a dead exec session)
/// or when a completed checker used an unverified shell form. The latter is
/// one diagnostic per turn even when no verification gate is pending.
fn flush_loop_notices(ctx: &mut TurnProcessingContext<'_>, loop_tracker: &mut super::super::helpers::LoopTracker) {
    if loop_tracker.take_focused_search_notice() {
        ctx.push_system_message("Several searches have concentrated on one file this turn. Reuse the gathered context and take the next concrete edit, verification, or synthesis step when it is sufficient. Search again only for a specific missing fact. This is coaching, not a read cap; distinct queries and tools remain available under existing limits.");
    }
    if loop_tracker.take_redundant_navigation_notice() {
        const DIRECTIVE: &str = "This inspection returned only text evidence already gathered this turn. Reuse it to edit, verify, or synthesize; read only missing or changed ranges. Do not copy a capped file to another path to evade the cap. Tools remain available under the existing limits; this notice grants no permissions, verification credit, or repair edits.";
        ctx.renderer
            .line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Inspection repeated retained evidence; continue from the existing context.",
            )
            .unwrap_or(());
        ctx.push_system_message(DIRECTIVE);
    }
    if loop_tracker.take_verification_result_lost_notice() {
        ctx.renderer
            .line(vtcode_core::utils::ansi::MessageStyle::Warning, VERIFICATION_RESULT_LOST_WARNING)
            .unwrap_or(());
        ctx.push_system_message(VERIFICATION_RESULT_LOST_DIRECTIVE);
    }
    if loop_tracker.take_piped_verification_notice() {
        ctx.renderer
            .line(vtcode_core::utils::ansi::MessageStyle::Warning, PIPED_VERIFICATION_WARNING)
            .unwrap_or(());
        ctx.push_system_message(PIPED_VERIFICATION_DIRECTIVE);
    }
}

struct ValidatedToolCall<'a> {
    tool_call: &'a PreparedAssistantToolCall,
    prepared: PreparedToolCall,
}

fn snapshot_circuit_diagnostics(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    tool_name: &str,
) -> Option<vtcode_core::tools::circuit_breaker::ToolCircuitDiagnostics> {
    registry
        .shared_circuit_breaker()
        .map(|breaker| breaker.get_diagnostics(tool_name))
}

async fn record_circuit_transition(
    ctx: &TurnProcessingContext<'_>,
    tool_name: &str,
    before: Option<vtcode_core::tools::circuit_breaker::ToolCircuitDiagnostics>,
) {
    let Some(before) = before else {
        return;
    };
    let Some(breaker) = ctx.tool_registry.shared_circuit_breaker() else {
        return;
    };
    ctx.error_recovery
        .write()
        .await
        .record_circuit_transition_from_snapshot(&breaker, tool_name, &before);
}

impl ValidatedToolCall<'_> {
    fn call_id(&self) -> &str {
        self.tool_call.call_id()
    }

    fn can_parallelize(&self) -> bool {
        self.prepared.readonly_classification
            && self.tool_call.is_parallel_safe()
            && self.prepared.parallel_safe_after_preflight
    }
}

#[cfg(test)]
fn planned_execution_group_stats(
    validated_calls: &[ValidatedToolCall<'_>],
    allow_parallel: bool,
) -> (usize, usize, usize) {
    let layout = planned_execution_layout(validated_calls, allow_parallel, 0);
    execution_group_stats_from_layout(&layout)
}

fn planned_execution_layout(
    validated_calls: &[ValidatedToolCall<'_>],
    allow_parallel: bool,
    max_parallel: usize,
) -> Vec<(PreparedToolBatchKind, usize)> {
    let mut semantic_calls = std::collections::HashSet::new();
    PreparedToolBatch::plan_layout_with_limit(
        validated_calls.iter().map(|call| {
            let semantic_key = format!("{}:{}", call.prepared.canonical_name, call.prepared.effective_args);
            call.can_parallelize() && semantic_calls.insert(semantic_key)
        }),
        allow_parallel,
        max_parallel,
    )
}

fn execution_group_stats_from_layout(layout: &[(PreparedToolBatchKind, usize)]) -> (usize, usize, usize) {
    let groups = layout.len();
    let parallel_groups = layout
        .iter()
        .filter(|(kind, _)| matches!(kind, PreparedToolBatchKind::ParallelReadonly))
        .count();
    let max_group_size = layout.iter().map(|(_, len)| *len).max().unwrap_or(0);

    (groups, parallel_groups, max_group_size)
}

fn exec_session_tool_active(tool_name: &str) -> bool {
    use vtcode_core::config::constants::tools as tool_names;

    matches!(tool_name, tool_names::RUN_PTY_CMD | tool_names::UNIFIED_EXEC | tool_names::SEND_PTY_INPUT)
}

async fn terminate_group_exec_sessions_if_needed(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    group_has_exec_sessions: bool,
    log_message: &str,
) {
    if group_has_exec_sessions && let Err(err) = registry.terminate_active_exec_sessions_async().await {
        tracing::warn!(error = %err, "{log_message}");
    }
}

async fn drain_parallel_group<F>(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    execution_futures: &mut FuturesUnordered<F>,
    group_has_exec_sessions: bool,
    log_message: &str,
) where
    F: Future,
{
    terminate_group_exec_sessions_if_needed(registry, group_has_exec_sessions, log_message).await;
    while execution_futures.next().await.is_some() {}
}

async fn interrupt_parallel_group<F>(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    execution_futures: &mut FuturesUnordered<F>,
    group_has_exec_sessions: bool,
    turn_result: crate::agent::runloop::unified::turn::context::TurnLoopResult,
    log_message: &str,
) -> TurnHandlerOutcome
where
    F: Future,
{
    drain_parallel_group(registry, execution_futures, group_has_exec_sessions, log_message).await;
    TurnHandlerOutcome::Break(turn_result)
}

async fn execute_parallel_group<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    validated_calls: Vec<ValidatedToolCall<'_>>,
    batch_tracker: &mut crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome,
) -> Result<Option<TurnHandlerOutcome>> {
    if validated_calls.is_empty() {
        return Ok(None);
    }

    // Every call in this group passed the final argument-dependent guards.
    // This admitted execution is real progress; validation-only denials never
    // reach this boundary and therefore retain the assistant-response streak.
    t_ctx.ctx.harness_state.reset_assistant_text_response_streak();

    let progress_reporter = ProgressReporter::new();
    let spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
        t_ctx.ctx.handle,
        t_ctx.ctx.input_status_state.left.clone(),
        t_ctx.ctx.input_status_state.right.clone(),
        format!("Executing {} tools...", validated_calls.len()),
        Some(&progress_reporter),
    );

    let registry = t_ctx.ctx.tool_registry.clone();
    let ctrl_c_state = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_state);
    let ctrl_c_notify = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_notify);
    let tool_result_cache = std::sync::Arc::clone(t_ctx.ctx.tool_result_cache);
    let vt_cfg = t_ctx.ctx.vt_cfg;
    let group_has_exec_sessions = validated_calls
        .iter()
        .any(|validated_call| exec_session_tool_active(&validated_call.prepared.canonical_name));

    t_ctx
        .ctx
        .handle
        .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::RunningTools);
    let mut execution_futures = FuturesUnordered::new();
    for validated_call in validated_calls {
        let registry = registry.clone();
        let ctrl_c_state = std::sync::Arc::clone(&ctrl_c_state);
        let ctrl_c_notify = std::sync::Arc::clone(&ctrl_c_notify);
        let tool_result_cache = std::sync::Arc::clone(&tool_result_cache);
        let reporter = progress_reporter.clone();
        let call_id = validated_call.call_id().to_string();
        let name = validated_call.prepared.canonical_name;
        let args = validated_call.prepared.effective_args;
        let harness_emitter = t_ctx.ctx.harness_emitter;
        let streamed_item = t_ctx.ctx.harness_state.take_streamed_tool_call_item_id(&call_id);
        let already_started = streamed_item.is_some();
        let harness_item_id = streamed_item
            .map(|item| item.item_id)
            .unwrap_or_else(|| resolve_harness_item_identity(&call_id).1);
        let tool_started_emitted =
            emit_tool_start_if_needed(harness_emitter, already_started, &harness_item_id, &call_id, &name, &args);

        let fut = async move {
            let start_time = std::time::Instant::now();
            let max_retries = resolve_max_tool_retries(&name, vt_cfg);
            let circuit_before = snapshot_circuit_diagnostics(&registry, &name);
            let status = execute_prevalidated_read_only_with_cache(
                &registry,
                &tool_result_cache,
                &name,
                &args,
                &ctrl_c_state,
                &ctrl_c_notify,
                Some(&reporter),
                max_retries,
                exec_settlement_mode_for_tool_call(true, &name, &args),
                // Every call in this group was safety-admitted by
                // validate_tool_call before parallel execution; the registry
                // must not re-admit it on the shared gateway (double-counting
                // halves the effective per-turn tool budget).
                true,
            )
            .await;
            let mut outcome = crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(status);
            if outcome.total_duration.is_zero() {
                outcome.total_duration = start_time.elapsed();
            }
            // Emit inside the future so cancellation/early-return draining also
            // closes every admitted call with its actual terminal result.
            emit_tool_completion_for_status(
                harness_emitter,
                tool_started_emitted,
                true,
                &harness_item_id,
                &call_id,
                &name,
                &args,
                &outcome.status,
            );
            emit_tool_outcome_observation(harness_emitter, &name, &outcome);
            (call_id, name, args, outcome, start_time, circuit_before)
        };
        execution_futures.push(fut);
    }

    while !execution_futures.is_empty() {
        // A probe can observe the interrupt before this batch waiter does.
        // Recheck the authoritative state before awaiting another tool result.
        if t_ctx.ctx.ctrl_c_state.is_exit_requested()
            || t_ctx.ctx.ctrl_c_state.is_cancel_requested()
            || t_ctx.ctx.ctrl_c_state.is_cancel_handled()
        {
            spinner.finish_with_restore(false);
            let turn_result = if t_ctx.ctx.ctrl_c_state.is_exit_requested() {
                crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit
            } else {
                crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled
            };
            return Ok(Some(
                interrupt_parallel_group(
                    &registry,
                    &mut execution_futures,
                    group_has_exec_sessions,
                    turn_result,
                    "Failed to terminate exec sessions after grouped tool interruption",
                )
                .await,
            ));
        }
        let next_result = tokio::select! {
            _ = t_ctx.ctx.ctrl_c_notify.notified() => {
                if t_ctx.ctx.ctrl_c_state.is_exit_requested()
                    || t_ctx.ctx.ctrl_c_state.is_cancel_requested()
                {
                    spinner.finish_with_restore(false);
                    let turn_result = if t_ctx.ctx.ctrl_c_state.is_exit_requested() {
                        crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit
                    } else {
                        crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled
                    };
                    return Ok(Some(interrupt_parallel_group(
                        &registry,
                        &mut execution_futures,
                        group_has_exec_sessions,
                        turn_result,
                        "Failed to terminate exec sessions during grouped tool cancellation",
                    )
                    .await));
                }
                continue;
            }
            result = execution_futures.next() => result,
        };

        let Some((call_id, name, args, outcome, start_time, circuit_before)) = next_result else {
            break;
        };

        batch_tracker.record(&outcome.status);
        record_circuit_transition(t_ctx.ctx, &name, circuit_before).await;

        if update_repetition_tracker(t_ctx.repeated_tool_attempts, &outcome, &name, &args) {
            // A failed verifier grants fix-up edits; give the model one
            // diagnostic explanation before the pending-verification text cap
            // re-applies instead of blocking the turn on the failure summary.
            t_ctx.ctx.harness_state.reset_assistant_text_response_streak();
        }
        t_ctx
            .ctx
            .session_stats
            .set_verification_snapshot(t_ctx.repeated_tool_attempts.verification_snapshot());

        let handler_outcome = match handle_tool_execution_result_with_spinner(
            t_ctx,
            call_id,
            &name,
            &args,
            &outcome,
            start_time,
            Some(&spinner),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                drain_parallel_group(
                    &registry,
                    &mut execution_futures,
                    group_has_exec_sessions,
                    "Failed to terminate exec sessions after grouped tool post-processing failure",
                )
                .await;
                return Err(error);
            }
        };
        flush_loop_notices(t_ctx.ctx, t_ctx.repeated_tool_attempts);
        if let Some(outcome) = handler_outcome {
            if let TurnHandlerOutcome::Break(
                turn_result @ (crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit
                | crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled),
            ) = outcome
            {
                spinner.finish_with_restore(false);
                return Ok(Some(
                    interrupt_parallel_group(
                        &registry,
                        &mut execution_futures,
                        group_has_exec_sessions,
                        turn_result,
                        "Failed to terminate exec sessions after grouped tool interruption",
                    )
                    .await,
                ));
            }
            if matches!(outcome, TurnHandlerOutcome::Continue)
                && t_ctx.ctx.harness_state.blocked_tool_recovery_pending()
            {
                // Finish already-admitted read-only futures so every tool
                // call in the assistant batch receives a response before the
                // recovery directive is appended.
                continue;
            }
            drain_parallel_group(
                &registry,
                &mut execution_futures,
                group_has_exec_sessions,
                "Failed to terminate exec sessions after grouped tool handoff",
            )
            .await;
            return Ok(Some(outcome));
        }
    }

    Ok(None)
}

pub(crate) async fn handle_tool_call_batch_prepared<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_calls: &[PreparedAssistantToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.set_phase(TurnPhase::ExecutingTools);

    let mut validated_calls = Vec::with_capacity(tool_calls.len());

    for (index, tool_call) in tool_calls.iter().enumerate() {
        let Some(args) = tool_call.args() else {
            if let Some(err) = tool_call.args_error() {
                if let Some(outcome) =
                    super::handle_preflight_failure(t_ctx.ctx, tool_call.call_id(), tool_call.tool_name(), err, None)
                {
                    super::drain_preflight_circuit_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                    flush_budget_synthesis_directives(t_ctx.ctx);
                    return Ok(Some(outcome));
                }
            }
            continue;
        };

        match block_mutation_until_verification(
            t_ctx.ctx,
            t_ctx.repeated_tool_attempts,
            tool_call.call_id(),
            tool_call.tool_name(),
            args,
        )? {
            super::MutationVerificationResult::Allowed => {}
            super::MutationVerificationResult::Blocked => continue,
            super::MutationVerificationResult::Outcome(outcome) => {
                if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                    super::drain_blocked_tool_recovery_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                }
                super::flush_blocked_tool_recovery(t_ctx.ctx);
                return Ok(Some(outcome));
            }
        }

        let validation_result = validate_tool_call(t_ctx.ctx, tool_call.call_id(), tool_call.tool_name(), args).await?;
        match finalize_validation_result(t_ctx.ctx, tool_call.call_id(), tool_call.tool_name(), args, validation_result)
        {
            ValidationTransition::Proceed(prepared) => {
                // A PreToolUse hook may have rewritten the arguments inside
                // validate_tool_call; re-evaluate the mutation guard against
                // the arguments that will actually execute.
                match block_mutation_until_verification(
                    t_ctx.ctx,
                    t_ctx.repeated_tool_attempts,
                    tool_call.call_id(),
                    &prepared.canonical_name,
                    &prepared.effective_args,
                )? {
                    super::MutationVerificationResult::Allowed => {
                        validated_calls.push(ValidatedToolCall { tool_call, prepared });
                    }
                    super::MutationVerificationResult::Blocked => continue,
                    super::MutationVerificationResult::Outcome(outcome) => {
                        if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                            super::drain_blocked_tool_recovery_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                        }
                        super::flush_blocked_tool_recovery(t_ctx.ctx);
                        return Ok(Some(outcome));
                    }
                }
            }
            ValidationTransition::Return(Some(outcome)) => {
                if t_ctx.ctx.harness_state.consecutive_preflight_failures
                    >= super::max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx)
                {
                    super::drain_preflight_circuit_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                }
                if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                    super::drain_blocked_tool_recovery_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                }
                flush_budget_synthesis_directives(t_ctx.ctx);
                super::flush_blocked_tool_recovery(t_ctx.ctx);
                return Ok(Some(outcome));
            }
            ValidationTransition::Return(None) => continue,
        }
    }

    // If the wall-clock budget tripped during validation, push the single
    // "synthesize now" directive after all tool responses (never interleaved).
    flush_budget_synthesis_directives(t_ctx.ctx);

    if validated_calls.is_empty() {
        return Ok(None);
    }

    let max_parallel_tool_calls = t_ctx
        .ctx
        .vt_cfg
        .map(|config| config.agent.harness.max_parallel_tool_calls)
        .unwrap_or(DEFAULT_MAX_PARALLEL_TOOL_CALLS);
    let planned_layout = planned_execution_layout(&validated_calls, true, max_parallel_tool_calls);
    let (group_count, parallel_group_count, maximum_group_size) = execution_group_stats_from_layout(&planned_layout);
    let admitted_parallel_calls = planned_layout
        .iter()
        .filter(|(kind, _)| matches!(kind, PreparedToolBatchKind::ParallelReadonly))
        .map(|(_, len)| *len)
        .sum::<usize>();
    tracing::debug!(
        target: "vtcode.turn.metrics",
        metric = "tool_dispatch_groups",
        configured_limit = max_parallel_tool_calls,
        admitted_parallel_calls,
        group_count,
        parallel_group_count,
        maximum_group_size,
        "turn metric"
    );

    let mut batch_tracker = crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome::new();
    let mut validated_calls = validated_calls.into_iter();

    for (kind, len) in planned_layout {
        let group = validated_calls.by_ref().take(len).collect::<Vec<_>>();
        match kind {
            PreparedToolBatchKind::ParallelReadonly => {
                if let Some(outcome) = execute_parallel_group(t_ctx, group, &mut batch_tracker).await? {
                    if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                        for remaining in validated_calls.by_ref() {
                            super::push_blocked_tool_recovery_response(t_ctx.ctx, remaining.tool_call);
                        }
                        super::flush_blocked_tool_recovery(t_ctx.ctx);
                    }
                    return Ok(Some(outcome));
                }
                if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                    for remaining in validated_calls.by_ref() {
                        super::push_blocked_tool_recovery_response(t_ctx.ctx, remaining.tool_call);
                    }
                    super::flush_blocked_tool_recovery(t_ctx.ctx);
                    return Ok(Some(TurnHandlerOutcome::Continue));
                }
            }
            PreparedToolBatchKind::Sequential => {
                let mut group_iter = group.into_iter().enumerate();
                while let Some((_group_index, validated_call)) = group_iter.next() {
                    let tool_call_id = validated_call.call_id().to_string();
                    let tool_name = validated_call.prepared.canonical_name;
                    let args = validated_call.prepared.effective_args;
                    if let Some(outcome) = execute_and_handle_tool_call(
                        t_ctx.ctx,
                        t_ctx.repeated_tool_attempts,
                        t_ctx.turn_modified_files,
                        tool_call_id,
                        &tool_name,
                        args,
                        None,
                        Some(&mut batch_tracker),
                    )
                    .await?
                    {
                        if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                            for (_, remaining) in group_iter {
                                super::push_blocked_tool_recovery_response(t_ctx.ctx, remaining.tool_call);
                            }
                            for remaining in validated_calls.by_ref() {
                                super::push_blocked_tool_recovery_response(t_ctx.ctx, remaining.tool_call);
                            }
                            super::flush_blocked_tool_recovery(t_ctx.ctx);
                        }
                        return Ok(Some(outcome));
                    }
                }
            }
        }
    }

    if batch_tracker.entries.len() > 1 {
        let stats = batch_tracker.stats();
        tracing::info!(
            target: "vtcode.tool.batch",
            total = stats.total,
            succeeded = stats.succeeded,
            failed = stats.failed,
            timed_out = stats.timed_out,
            cancelled = stats.cancelled,
            partial_success = batch_tracker.is_partial_success(),
            summary = %batch_tracker.summary_line(),
            "tool batch outcome"
        );
    }

    Ok(None)
}

pub(crate) fn execute_and_handle_tool_call<'a, 'b>(
    ctx: &'b mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &'b mut super::super::helpers::LoopTracker,
    turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &'b str,
    args_val: serde_json::Value,
    _batch_progress_reporter: Option<&'b ProgressReporter>,
    batch_tracker: Option<&'b mut crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome>,
) -> futures::future::BoxFuture<'b, Result<Option<TurnHandlerOutcome>>> {
    Box::pin(execute_and_handle_tool_call_inner(
        ctx,
        repeated_tool_attempts,
        turn_modified_files,
        tool_call_id,
        tool_name,
        args_val,
        batch_tracker,
    ))
}

async fn execute_and_handle_tool_call_inner<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &mut super::super::helpers::LoopTracker,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
    batch_tracker: Option<&mut crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome>,
) -> Result<Option<TurnHandlerOutcome>> {
    match block_mutation_until_verification(ctx, repeated_tool_attempts, tool_call_id.as_str(), tool_name, &args_val)? {
        super::MutationVerificationResult::Allowed => {}
        super::MutationVerificationResult::Blocked => return Ok(None),
        super::MutationVerificationResult::Outcome(outcome) => return Ok(Some(outcome)),
    }

    // Reset only after the final mutation guard. Permission or hook rewrites
    // can change a call's intent after initial preflight; a rewritten mutation
    // blocked above must not be mistaken for productive tool progress.
    ctx.harness_state.reset_assistant_text_response_streak();

    // Show pre-execution indicator for file modification operations
    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(tool_name, &args_val) {
        let summary_ctx = crate::agent::runloop::unified::tool_summary::ToolSummaryRenderContext {
            workspace_root: Some(ctx.config.workspace.as_path()),
        };
        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
            ctx.renderer,
            tool_name,
            &args_val,
            &summary_ctx,
        )?;
    }
    let tool_execution_start = std::time::Instant::now();
    let circuit_before = snapshot_circuit_diagnostics(ctx.tool_registry, tool_name);
    let pipeline_outcome = {
        let ctrl_c_state = ctx.ctrl_c_state;
        let ctrl_c_notify = ctx.ctrl_c_notify;
        let default_placeholder = ctx.default_placeholder.clone();
        let lifecycle_hooks = ctx.lifecycle_hooks;
        let vt_cfg = ctx.vt_cfg;
        let turn_index = ctx.working_history.len();
        let mut run_loop_ctx = ctx.as_run_loop_context();
        run_tool_call_with_args(
            &mut run_loop_ctx,
            tool_call_id.clone(),
            tool_name,
            &args_val,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            lifecycle_hooks,
            true,
            vt_cfg,
            turn_index,
            true,
        )
        .await?
    };
    if let Some(batch_tracker) = batch_tracker {
        batch_tracker.record(&pipeline_outcome.status);
    }
    record_circuit_transition(ctx, tool_name, circuit_before).await;

    if update_repetition_tracker(repeated_tool_attempts, &pipeline_outcome, tool_name, &args_val) {
        // A failed verifier grants fix-up edits; reset the text streak so the
        // model can diagnose the failure before the text cap re-applies.
        ctx.harness_state.reset_assistant_text_response_streak();
    }
    ctx.session_stats
        .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());

    let mut t_ctx = ToolOutcomeContext { ctx, repeated_tool_attempts, turn_modified_files };

    let outcome = handle_tool_execution_result(
        &mut t_ctx,
        tool_call_id,
        tool_name,
        &args_val,
        &pipeline_outcome,
        tool_execution_start,
    )
    .await?;
    flush_loop_notices(t_ctx.ctx, t_ctx.repeated_tool_attempts);

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::stream::FuturesUnordered;
    use tempfile::TempDir;
    use vtcode_core::config::constants::tools;
    use vtcode_core::tools::registry::ToolRegistry;

    use super::{PreparedToolCall, ValidatedToolCall, interrupt_parallel_group, planned_execution_group_stats};
    use crate::agent::runloop::unified::turn::context::{
        PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult,
    };

    fn validated_call<'a>(
        call_id: &'a str,
        tool_name: &str,
        readonly_classification: bool,
        parallel_safe_after_preflight: bool,
        effective_args: serde_json::Value,
    ) -> ValidatedToolCall<'a> {
        let raw_tool_call = vtcode_core::llm::provider::ToolCall::function(
            call_id.to_string(),
            tool_name.to_string(),
            serde_json::to_string(&effective_args).expect("serialize args"),
        );
        ValidatedToolCall {
            tool_call: Box::leak(Box::new(PreparedAssistantToolCall::new(raw_tool_call))),
            prepared: PreparedToolCall {
                canonical_name: tool_name.to_string(),
                readonly_classification,
                parallel_safe_after_preflight,
                effective_args: effective_args.clone(),
                fallback_recommendation: None,
                already_preflighted: true,
            },
        }
    }

    #[test]
    fn build_execution_groups_batches_contiguous_parallel_safe_reads() {
        let stats = planned_execution_group_stats(
            &[
                validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"turn loop"})),
                validated_call("call_2", tools::READ_FILE, true, true, serde_json::json!({"path":"src/main.rs"})),
            ],
            true,
        );

        assert_eq!(stats, (1, 1, 2));
    }

    #[test]
    fn build_execution_groups_preserves_order_around_mutating_calls() {
        let stats = planned_execution_group_stats(
            &[
                validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
                validated_call(
                    "call_2",
                    tools::UNIFIED_EXEC,
                    false,
                    false,
                    serde_json::json!({"action":"run","command":["cargo","check"]}),
                ),
                validated_call("call_3", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"omega"})),
            ],
            true,
        );

        assert_eq!(stats, (3, 0, 1));
    }

    #[test]
    fn build_execution_groups_batches_duplicate_parallel_tool_names() {
        let stats = planned_execution_group_stats(
            &[
                validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
                validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
            ],
            true,
        );

        assert_eq!(stats, (1, 1, 2));
    }

    #[test]
    fn build_execution_groups_falls_back_to_serial_when_parallel_disabled() {
        let stats = planned_execution_group_stats(
            &[
                validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
                validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
            ],
            false,
        );

        assert_eq!(stats, (2, 0, 1));
    }

    #[test]
    fn build_execution_groups_keeps_non_parallel_safe_reads_serial() {
        let stats = planned_execution_group_stats(
            &[
                validated_call("call_1", tools::LIST_PTY_SESSIONS, true, false, serde_json::json!({})),
                validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
            ],
            true,
        );

        assert_eq!(stats, (2, 0, 1));
    }

    #[test]
    fn build_execution_groups_respects_post_preflight_parallel_safety() {
        let raw_tool_call = vtcode_core::llm::provider::ToolCall::function(
            "call_remapped".to_string(),
            tools::UNIFIED_FILE.to_string(),
            serde_json::json!({"path":"src/main.rs"}).to_string(),
        );
        let remapped = ValidatedToolCall {
            tool_call: Box::leak(Box::new(PreparedAssistantToolCall::new(raw_tool_call))),
            prepared: PreparedToolCall {
                canonical_name: tools::UNIFIED_EXEC.to_string(),
                readonly_classification: true,
                parallel_safe_after_preflight: false,
                effective_args: serde_json::json!({"action":"run","command":"git status"}),
                fallback_recommendation: None,
                already_preflighted: true,
            },
        };

        let stats = planned_execution_group_stats(
            &[
                remapped,
                validated_call("call_read", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
            ],
            true,
        );

        assert_eq!(stats, (2, 0, 1));
    }

    #[tokio::test]
    async fn interrupt_parallel_group_drains_pending_futures() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let completions = Arc::new(AtomicUsize::new(0));
        let mut futures = FuturesUnordered::new();

        for _ in 0..2 {
            let completions = Arc::clone(&completions);
            futures.push(async move {
                completions.fetch_add(1, Ordering::SeqCst);
            });
        }

        let outcome = interrupt_parallel_group(
            &registry,
            &mut futures,
            false,
            TurnLoopResult::Cancelled,
            "test interruption cleanup",
        )
        .await;

        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        assert_eq!(completions.load(Ordering::SeqCst), 2);
        assert!(futures.is_empty());
    }
}
