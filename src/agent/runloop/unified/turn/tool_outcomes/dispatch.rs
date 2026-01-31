//! Tool call dispatching for the turn loop.

use anyhow::Result;
use futures::future::join_all;
use std::sync::Arc;

use vtcode_core::llm::provider as uni;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::parallel_executor::ParallelExecutionPlanner;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolPipelineOutcome, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};

use super::execution_result::handle_tool_execution_result;
use super::helpers::{resolve_max_tool_retries, signature_key_for};
use call::handle_tool_call;

mod call;

pub(crate) async fn handle_tool_calls(
    ctx: &mut TurnProcessingContext<'_>,
    tool_calls: &[uni::ToolCall],
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    // HP-4: Use ParallelExecutionPlanner to group independent tool calls
    let planner = ParallelExecutionPlanner::new();
    let mut planner_calls = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        let name = tc.function.as_ref().map(|f| f.name.as_str()).unwrap_or("");
        let args = Arc::new(tc.parsed_arguments().unwrap_or(serde_json::json!({})));
        planner_calls.push((name.to_string(), args, tc.id.clone()));
    }

    let groups = planner.plan(&planner_calls);

    for group in groups {
        if group.len() > 1 && ctx.full_auto {
            // HP-5: Implement true parallel execution for non-conflicting groups in full-auto mode
            let mut group_tool_calls = Vec::with_capacity(group.len());
            for (_, _, call_id) in &group.tool_calls {
                if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                    group_tool_calls.push(tc);
                }
            }

            // Check if all tools in group are safe and approved
            let mut can_run_parallel = true;
            let mut execution_items = Vec::with_capacity(group_tool_calls.len());

            for tc in &group_tool_calls {
                let func = match tc.function.as_ref() {
                    Some(f) => f,
                    None => {
                        can_run_parallel = false;
                        break;
                    }
                };
                let tool_name = &func.name;
                let args_val = tc
                    .parsed_arguments()
                    .unwrap_or_else(|_| serde_json::json!({}));

                // Quick safety check
                {
                    let mut validator = ctx.safety_validator.write().await;
                    if validator.validate_call(tool_name).is_err() {
                        can_run_parallel = false;
                        break;
                    }
                }

                let is_allowed = matches!(
                    ctx.tool_registry.get_tool_policy(tool_name).await,
                    ToolPolicy::Allow
                );

                if !is_allowed {
                    can_run_parallel = false;
                    break;
                }

                execution_items.push((tc.id.clone(), tool_name.clone(), args_val));
            }

            if can_run_parallel && !execution_items.is_empty() {
                for (_, name, args) in &execution_items {
                    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(
                        name, args,
                    ) {
                        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
                            ctx.renderer,
                            name,
                            args,
                        )?;
                    }
                }

                let tool_names: Vec<_> = execution_items
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect();
                let batch_msg = format!("Executing batch: [{}]", tool_names.join(", "));

                let progress_reporter = ProgressReporter::new();
                let _spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    batch_msg,
                    Some(&progress_reporter),
                );

                // Execute all in parallel
                let registry = ctx.tool_registry.clone();
                let ctrl_c_state = Arc::clone(ctx.ctrl_c_state);
                let ctrl_c_notify = Arc::clone(ctx.ctrl_c_notify);

                let max_tool_retries = resolve_max_tool_retries(ctx.vt_cfg);
                let futures: Vec<_> = execution_items
                    .iter()
                    .map(|(call_id, name, args)| {
                        let registry = registry.clone();
                        let ctrl_c_state = Arc::clone(&ctrl_c_state);
                        let ctrl_c_notify = Arc::clone(&ctrl_c_notify);
                        let name = name.clone();
                        let args = args.clone();
                        let reporter = progress_reporter.clone();

                        async move {
                            let start_time = std::time::Instant::now();
                            let result = execute_tool_with_timeout_ref(
                                &registry,
                                &name,
                                &args,
                                &ctrl_c_state,
                                &ctrl_c_notify,
                                Some(&reporter),
                                max_tool_retries,
                            )
                            .await;
                            (call_id.clone(), name, args, result, start_time)
                        }
                    })
                    .collect();

                let results = join_all(futures).await;

                for (call_id, name, args, status, start_time) in results {
                    let outcome = ToolPipelineOutcome::from_status(status);

                    // Only count SUCCESSFUL tool calls for turn balancer repetition tracking.
                    // Failed, timed out, or cancelled tool calls should NOT trigger the turn balancer.
                    if matches!(
                        &outcome.status,
                        crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success { .. }
                    ) {
                        let signature_key = signature_key_for(&name, &args);
                        let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
                        *current_count += 1;
                    }

                    handle_tool_execution_result(
                        ctx,
                        call_id,
                        &name,
                        &args,
                        &outcome,
                        turn_modified_files,
                        traj,
                        start_time,
                    )
                    .await?;
                }
                continue; // Move to next group
            }

            for tc in group_tool_calls {
                if let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
                {
                    return Ok(Some(outcome));
                }
            }
        } else {
            let call_id = &group.tool_calls[0].2;
            if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id)
                && let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
            {
                return Ok(Some(outcome));
            }
        }
    }

    Ok(None)
}
