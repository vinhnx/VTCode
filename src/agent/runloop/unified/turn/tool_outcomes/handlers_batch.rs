use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::run_tool_call_with_args;
use crate::agent::runloop::unified::tool_pipeline::should_settle_noninteractive_unified_exec;
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnProcessingContext,
};

use super::{
    PreparedToolCall, ToolOutcomeContext, ValidationTransition, finalize_validation_result,
    validate_tool_call,
};
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    push_invalid_tool_args_response, resolve_max_tool_retries, update_repetition_tracker,
};

struct ValidatedToolCall<'a> {
    tool_call: &'a PreparedAssistantToolCall,
    prepared: PreparedToolCall,
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

fn planned_execution_group_stats(
    validated_calls: &[ValidatedToolCall<'_>],
    allow_parallel: bool,
) -> (usize, usize, usize) {
    if !allow_parallel {
        let groups = validated_calls.len();
        return (groups, 0, usize::from(groups > 0));
    }

    let mut groups = 0usize;
    let mut parallel_groups = 0usize;
    let mut max_group_size = 1usize;
    let mut current_parallel_group_size = 0usize;

    for validated_call in validated_calls {
        if validated_call.can_parallelize() {
            current_parallel_group_size += 1;
            continue;
        }

        if current_parallel_group_size > 0 {
            groups += 1;
            parallel_groups += 1;
            max_group_size = max_group_size.max(current_parallel_group_size);
            current_parallel_group_size = 0;
        }
        groups += 1;
    }

    if current_parallel_group_size > 0 {
        groups += 1;
        parallel_groups += 1;
        max_group_size = max_group_size.max(current_parallel_group_size);
    }

    (groups, parallel_groups, max_group_size)
}

fn exec_session_tool_active(tool_name: &str) -> bool {
    use vtcode_core::config::constants::tools as tool_names;

    matches!(
        tool_name,
        tool_names::RUN_PTY_CMD | tool_names::UNIFIED_EXEC | tool_names::SEND_PTY_INPUT
    )
}

async fn terminate_group_exec_sessions_if_needed(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    group_has_exec_sessions: bool,
    log_message: &str,
) {
    if group_has_exec_sessions && let Err(err) = registry.terminate_all_exec_sessions_async().await
    {
        tracing::warn!(error = %err, "{log_message}");
    }
}

async fn interrupt_parallel_group<F>(
    registry: &vtcode_core::tools::registry::ToolRegistry,
    execution_futures: &mut FuturesUnordered<F>,
    group_has_exec_sessions: bool,
    turn_result: crate::agent::runloop::unified::turn::context::TurnLoopResult,
    log_message: &str,
) -> TurnHandlerOutcome
where
    F: futures::Future,
{
    terminate_group_exec_sessions_if_needed(registry, group_has_exec_sessions, log_message).await;
    while execution_futures.next().await.is_some() {}
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

    let progress_reporter = ProgressReporter::new();
    let _spinner =
        crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
            t_ctx.ctx.handle,
            t_ctx.ctx.input_status_state.left.clone(),
            t_ctx.ctx.input_status_state.right.clone(),
            format!("Executing {} tools...", validated_calls.len()),
            Some(&progress_reporter),
        );

    let registry = t_ctx.ctx.tool_registry.clone();
    let ctrl_c_state = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_state);
    let ctrl_c_notify = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_notify);
    let vt_cfg = t_ctx.ctx.vt_cfg;
    let group_has_exec_sessions = validated_calls
        .iter()
        .any(|validated_call| exec_session_tool_active(&validated_call.prepared.canonical_name));

    let mut execution_futures = FuturesUnordered::new();
    for validated_call in validated_calls {
        let registry = registry.clone();
        let ctrl_c_state = std::sync::Arc::clone(&ctrl_c_state);
        let ctrl_c_notify = std::sync::Arc::clone(&ctrl_c_notify);
        let reporter = progress_reporter.clone();
        let call_id = validated_call.call_id().to_string();
        let name = validated_call.prepared.canonical_name;
        let args = validated_call.prepared.effective_args;

        let fut = async move {
            let start_time = std::time::Instant::now();
            let max_retries = resolve_max_tool_retries(&name, vt_cfg);
            let status =
                crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref_prevalidated(
                    &registry,
                    &name,
                    &args,
                    &ctrl_c_state,
                    &ctrl_c_notify,
                    Some(&reporter),
                    max_retries,
                    should_settle_noninteractive_unified_exec(true, &name, &args),
                )
                .await;
            (call_id, name, args, status, start_time)
        };
        execution_futures.push(fut);
    }

    while !execution_futures.is_empty() {
        let next_result = tokio::select! {
            _ = t_ctx.ctx.ctrl_c_notify.notified() => {
                if t_ctx.ctx.ctrl_c_state.is_exit_requested()
                    || t_ctx.ctx.ctrl_c_state.is_cancel_requested()
                {
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

        let Some((call_id, name, args, status, start_time)) = next_result else {
            break;
        };

        batch_tracker.record(&status);

        let outcome =
            crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(status);
        update_repetition_tracker(t_ctx.repeated_tool_attempts, &outcome, &name, &args);

        if let Some(outcome) =
            handle_tool_execution_result(t_ctx, call_id, &name, &args, &outcome, start_time).await?
        {
            if matches!(
                outcome,
                TurnHandlerOutcome::Break(
                    crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit
                        | crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled
                )
            ) {
                let turn_result = match outcome {
                    TurnHandlerOutcome::Break(turn_result) => turn_result,
                    TurnHandlerOutcome::Continue => unreachable!("matched break outcome"),
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
            return Ok(Some(outcome));
        }
    }

    Ok(None)
}

async fn flush_parallel_group<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    parallel_group: &mut Vec<ValidatedToolCall<'_>>,
    batch_tracker: &mut crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome,
) -> Result<Option<TurnHandlerOutcome>> {
    if parallel_group.is_empty() {
        return Ok(None);
    }
    execute_parallel_group(t_ctx, std::mem::take(parallel_group), batch_tracker).await
}

pub(crate) async fn handle_tool_call_batch_prepared<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_calls: &[PreparedAssistantToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.set_phase(TurnPhase::ExecutingTools);

    let mut validated_calls = Vec::with_capacity(tool_calls.len());

    for tool_call in tool_calls {
        let Some(args) = tool_call.args() else {
            if let Some(err) = tool_call.args_error() {
                push_invalid_tool_args_response(
                    t_ctx.ctx.working_history,
                    tool_call.call_id(),
                    tool_call.tool_name(),
                    err,
                );
            }
            continue;
        };

        let validation_result =
            validate_tool_call(t_ctx.ctx, tool_call.call_id(), tool_call.tool_name(), args).await?;
        match finalize_validation_result(
            t_ctx.ctx,
            tool_call.call_id(),
            tool_call.tool_name(),
            args,
            validation_result,
        ) {
            ValidationTransition::Proceed(prepared) => {
                validated_calls.push(ValidatedToolCall {
                    tool_call,
                    prepared,
                });
            }
            ValidationTransition::Return(Some(outcome)) => return Ok(Some(outcome)),
            ValidationTransition::Return(None) => continue,
        }
    }

    if validated_calls.is_empty() {
        return Ok(None);
    }

    let (groups, parallel_groups, max_group_size) =
        planned_execution_group_stats(&validated_calls, t_ctx.ctx.full_auto);
    tracing::debug!(
        target: "vtcode.turn.metrics",
        metric = "tool_dispatch_groups",
        groups,
        parallel_groups,
        max_group_size,
        "turn metric"
    );

    let mut batch_tracker = crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome::new();
    let mut parallel_group = Vec::with_capacity(max_group_size);

    for validated_call in validated_calls {
        if t_ctx.ctx.full_auto && validated_call.can_parallelize() {
            parallel_group.push(validated_call);
            continue;
        }

        if let Some(outcome) =
            flush_parallel_group(t_ctx, &mut parallel_group, &mut batch_tracker).await?
        {
            return Ok(Some(outcome));
        }

        if let Some(outcome) = execute_and_handle_tool_call(
            t_ctx.ctx,
            t_ctx.repeated_tool_attempts,
            t_ctx.turn_modified_files,
            validated_call.call_id().to_string(),
            &validated_call.prepared.canonical_name,
            validated_call.prepared.effective_args,
            None,
        )
        .await?
        {
            return Ok(Some(outcome));
        }
    }

    if let Some(outcome) =
        flush_parallel_group(t_ctx, &mut parallel_group, &mut batch_tracker).await?
    {
        return Ok(Some(outcome));
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_and_handle_tool_call<'a, 'b>(
    ctx: &'b mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &'b mut super::super::helpers::LoopTracker,
    turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &'b str,
    args_val: serde_json::Value,
    _batch_progress_reporter: Option<&'b ProgressReporter>,
) -> futures::future::BoxFuture<'b, Result<Option<TurnHandlerOutcome>>> {
    Box::pin(execute_and_handle_tool_call_inner(
        ctx,
        repeated_tool_attempts,
        turn_modified_files,
        tool_call_id,
        tool_name,
        args_val,
    ))
}

async fn execute_and_handle_tool_call_inner<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &mut super::super::helpers::LoopTracker,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    // Show pre-execution indicator for file modification operations
    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(tool_name, &args_val)
    {
        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
            ctx.renderer,
            tool_name,
            &args_val,
        )?;
    }
    let tool_execution_start = std::time::Instant::now();
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

    update_repetition_tracker(
        repeated_tool_attempts,
        &pipeline_outcome,
        tool_name,
        &args_val,
    );

    let mut t_ctx = ToolOutcomeContext {
        ctx,
        repeated_tool_attempts,
        turn_modified_files,
    };

    let outcome = handle_tool_execution_result(
        &mut t_ctx,
        tool_call_id,
        tool_name,
        &args_val,
        &pipeline_outcome,
        tool_execution_start,
    )
    .await?;

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::{
        PreparedToolCall, ValidatedToolCall, interrupt_parallel_group,
        planned_execution_group_stats,
    };
    use crate::agent::runloop::unified::turn::context::PreparedAssistantToolCall;
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
    use futures::stream::FuturesUnordered;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use vtcode_core::config::constants::tools;
    use vtcode_core::tools::registry::ToolRegistry;

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
            },
        }
    }

    #[test]
    fn build_execution_groups_batches_contiguous_parallel_safe_reads() {
        let stats = planned_execution_group_stats(
            &[
                validated_call(
                    "call_1",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"turn loop"}),
                ),
                validated_call(
                    "call_2",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"tool outcomes"}),
                ),
            ],
            true,
        );

        assert_eq!(stats, (1, 1, 2));
    }

    #[test]
    fn build_execution_groups_preserves_order_around_mutating_calls() {
        let stats = planned_execution_group_stats(
            &[
                validated_call(
                    "call_1",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"alpha"}),
                ),
                validated_call(
                    "call_2",
                    tools::UNIFIED_EXEC,
                    false,
                    false,
                    serde_json::json!({"action":"run","command":["cargo","check"]}),
                ),
                validated_call(
                    "call_3",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"omega"}),
                ),
            ],
            true,
        );

        assert_eq!(stats, (3, 2, 1));
    }

    #[test]
    fn build_execution_groups_falls_back_to_serial_when_parallel_disabled() {
        let stats = planned_execution_group_stats(
            &[
                validated_call(
                    "call_1",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"alpha"}),
                ),
                validated_call(
                    "call_2",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"beta"}),
                ),
            ],
            false,
        );

        assert_eq!(stats, (2, 0, 1));
    }

    #[test]
    fn build_execution_groups_keeps_non_parallel_safe_reads_serial() {
        let stats = planned_execution_group_stats(
            &[
                validated_call(
                    "call_1",
                    tools::LIST_PTY_SESSIONS,
                    true,
                    false,
                    serde_json::json!({}),
                ),
                validated_call(
                    "call_2",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"beta"}),
                ),
            ],
            true,
        );

        assert_eq!(stats, (2, 1, 1));
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
            },
        };

        let stats = planned_execution_group_stats(
            &[
                remapped,
                validated_call(
                    "call_read",
                    tools::UNIFIED_SEARCH,
                    true,
                    true,
                    serde_json::json!({"action":"grep","pattern":"beta"}),
                ),
            ],
            true,
        );

        assert_eq!(stats, (2, 1, 1));
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

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)
        ));
        assert_eq!(completions.load(Ordering::SeqCst), 2);
        assert!(futures.is_empty());
    }
}
