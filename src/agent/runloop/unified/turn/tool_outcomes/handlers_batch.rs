use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::run_tool_call_with_args;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};

use super::{PreparedToolCall, ToolOutcomeContext, ValidationResult, validate_tool_call};
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
    push_tool_response, resolve_max_tool_retries, update_repetition_tracker,
};

pub(crate) struct ParsedToolCall<'a> {
    pub tool_call: &'a uni::ToolCall,
    pub args: serde_json::Value,
}

fn can_parallelize_batch_tool_call(prepared: &PreparedToolCall) -> bool {
    if matches!(
        prepared.canonical_name.as_str(),
        tools::ENTER_PLAN_MODE
            | tools::EXIT_PLAN_MODE
            | tools::REQUEST_USER_INPUT
            | tools::RUN_PTY_CMD
            | tools::UNIFIED_EXEC
            | tools::SEND_PTY_INPUT
            | tools::SHELL
    ) {
        return false;
    }
    prepared.readonly_classification
}

#[allow(dead_code)]
pub(crate) async fn handle_tool_call_batch<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_calls: &[&uni::ToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    let mut parsed_calls = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let Some(func) = tool_call.function.as_ref() else {
            continue;
        };
        let args = match tool_call.parsed_arguments() {
            Ok(args) => args,
            Err(err) => {
                push_tool_response(
                    t_ctx.ctx.working_history,
                    tool_call.id.clone(),
                    serde_json::json!({
                        "error": format!(
                            "Invalid tool arguments for '{}': {}",
                            func.name,
                            err
                        )
                    })
                    .to_string(),
                );
                continue;
            }
        };
        parsed_calls.push(ParsedToolCall { tool_call, args });
    }

    handle_tool_call_batch_parsed(t_ctx, parsed_calls).await
}

pub(crate) async fn handle_tool_call_batch_parsed<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_calls: Vec<ParsedToolCall<'_>>,
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.harness_state.set_phase(TurnPhase::ExecutingTools);

    let mut validated_calls = Vec::new();

    // 1. Validate all calls sequentially (safety first)
    for parsed_call in tool_calls {
        let func = match parsed_call.tool_call.function.as_ref() {
            Some(f) => f,
            None => continue, // Skip non-function calls
        };
        let tool_name = func.name.as_str();
        let args_val = parsed_call.args;

        match validate_tool_call(t_ctx.ctx, &parsed_call.tool_call.id, tool_name, &args_val).await?
        {
            ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
            ValidationResult::Blocked => {
                if let Some(outcome) = super::enforce_blocked_tool_call_guard(
                    t_ctx.ctx,
                    &parsed_call.tool_call.id,
                    tool_name,
                    &args_val,
                ) {
                    return Ok(Some(outcome));
                }
                continue;
            }
            ValidationResult::Proceed(prepared) => {
                t_ctx.ctx.harness_state.reset_blocked_tool_call_streak();
                validated_calls.push((parsed_call.tool_call, prepared));
            }
        }
    }

    if validated_calls.is_empty() {
        return Ok(None);
    }

    let can_parallelize = validated_calls
        .iter()
        .all(|(_, prepared)| can_parallelize_batch_tool_call(prepared));
    if !can_parallelize {
        for (tool_call, prepared) in validated_calls {
            if let Some(outcome) = execute_and_handle_tool_call(
                t_ctx.ctx,
                t_ctx.repeated_tool_attempts,
                t_ctx.turn_modified_files,
                tool_call.id.clone(),
                &prepared.canonical_name,
                prepared.effective_args,
                None,
            )
            .await?
            {
                return Ok(Some(outcome));
            }
        }
        return Ok(None);
    }

    // 2. Parallel Execution
    let progress_reporter = ProgressReporter::new();
    let validated_call_count = validated_calls.len();
    let _spinner =
        crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
            t_ctx.ctx.handle,
            t_ctx.ctx.input_status_state.left.clone(),
            t_ctx.ctx.input_status_state.right.clone(),
            format!("Executing {} tools...", validated_call_count),
            Some(&progress_reporter),
        );

    let registry = t_ctx.ctx.tool_registry.clone();
    let ctrl_c_state = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_state);
    let ctrl_c_notify = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_notify);
    let vt_cfg = t_ctx.ctx.vt_cfg;

    let mut execution_futures = FuturesUnordered::new();
    for (tool_call, prepared) in validated_calls {
        let registry = registry.clone();
        let ctrl_c_state = std::sync::Arc::clone(&ctrl_c_state);
        let ctrl_c_notify = std::sync::Arc::clone(&ctrl_c_notify);
        let reporter = progress_reporter.clone();
        let name = prepared.canonical_name;
        let call_id = tool_call.id.clone();
        let args = prepared.effective_args;

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
                )
                .await;
            (call_id, name, args, status, start_time)
        };
        execution_futures.push(fut);
    }

    // 3. Sequential Result Handling as each parallel call finishes.
    let mut batch_tracker = crate::agent::runloop::unified::tool_pipeline::ToolBatchOutcome::new();

    while !execution_futures.is_empty() {
        let next_result = tokio::select! {
            _ = t_ctx.ctx.ctrl_c_notify.notified() => {
                if t_ctx.ctx.ctrl_c_state.is_exit_requested() {
                    return Ok(Some(TurnHandlerOutcome::Break(crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit)));
                }
                if t_ctx.ctx.ctrl_c_state.is_cancel_requested() {
                    return Ok(Some(TurnHandlerOutcome::Break(crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled)));
                }
                continue;
            }
            result = execution_futures.next() => result,
        };

        let Some((call_id, name, args, status, start_time)) = next_result else {
            break;
        };

        // Record in the batch tracker before wrapping into ToolPipelineOutcome.
        batch_tracker.record(&name, &call_id, &status);

        let outcome =
            crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(status);

        // Track repetition
        update_repetition_tracker(t_ctx.repeated_tool_attempts, &outcome, &name, &args);

        // Handle result
        if let Some(outcome) = crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result(
            t_ctx,
            call_id,
            &name,
            &args,
            &outcome,
            start_time,
        ).await?
        {
            return Ok(Some(outcome));
        }
    }

    // Emit structured batch-level metrics when more than one tool was executed.
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
    tool_name: &str,
    args_val: serde_json::Value,
    batch_progress_reporter: Option<&'b ProgressReporter>,
) -> futures::future::BoxFuture<'b, Result<Option<TurnHandlerOutcome>>> {
    let tool_name_owned = tool_name.to_string();

    Box::pin(async move {
        execute_and_handle_tool_call_inner(
            ctx,
            repeated_tool_attempts,
            turn_modified_files,
            tool_call_id,
            &tool_name_owned,
            args_val,
            batch_progress_reporter,
        )
        .await
    })
}

async fn execute_and_handle_tool_call_inner<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &mut super::super::helpers::LoopTracker,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
    _batch_progress_reporter: Option<&ProgressReporter>,
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
