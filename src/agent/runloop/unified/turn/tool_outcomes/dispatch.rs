//! Tool call dispatching for the turn loop.

use anyhow::Result;
use futures::future::join_all;
use std::sync::Arc;

use vtcode_core::llm::provider as uni;
use vtcode_core::tools::parallel_executor::ParallelExecutionPlanner;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolPipelineOutcome, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};

use super::handlers::validate_tool_call;
use super::helpers::resolve_max_tool_retries;
use super::execution_result::handle_tool_execution_result;
use super::helpers::update_repetition_tracker;
use call::handle_tool_call;

mod call;

pub(crate) async fn handle_tool_calls<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_calls: &[uni::ToolCall],
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
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
            // 1. Validate EACH tool call in the group (sequential phase)
            let mut validated_items = Vec::with_capacity(group_tool_calls.len());
            let mut all_validated = true;

            for tc in &group_tool_calls {
                let func = match tc.function.as_ref() {
                    Some(f) => f,
                    None => {
                        all_validated = false;
                        break;
                    }
                };
                let tool_name = &func.name;
                let args_val = tc.parsed_arguments().unwrap_or_else(|_| serde_json::json!({}));

                match validate_tool_call(ctx, &tc.id, tool_name, &args_val).await? {
                    Some(outcome) => return Ok(Some(outcome)),
                    None => {
                        // Check if it was blocked (response pushed to history)
                        if let Some(last_msg) = ctx.working_history.last() {
                            if last_msg.role == uni::MessageRole::Tool
                                && last_msg.tool_call_id.as_ref() == Some(&tc.id)
                            {
                                all_validated = false;
                                break;
                            }
                        }
                    }
                }
                validated_items.push((tc.id.clone(), tool_name.clone(), args_val));
            }

            if all_validated && !validated_items.is_empty() {
                let tool_names: Vec<_> = validated_items
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

                let futures: Vec<_> = validated_items
                    .iter()
                    .map(|(call_id, name, args)| {
                        let registry = registry.clone();
                        let ctrl_c_state = Arc::clone(&ctrl_c_state);
                        let ctrl_c_notify = Arc::clone(&ctrl_c_notify);
                        let name = name.clone();
                        let args = args.clone();
                        let reporter = progress_reporter.clone();
                        let vt_cfg = ctx.vt_cfg;

                        async move {
                            let start_time = std::time::Instant::now();
                            let max_tool_retries = resolve_max_tool_retries(&name, vt_cfg);
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

                    // Track repetition (Success only)
                    super::helpers::update_repetition_tracker(
                        repeated_tool_attempts,
                        &outcome,
                        &name,
                        &args,
                    );

                    super::execution_result::handle_tool_execution_result(
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
            if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                if let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
                {
                    return Ok(Some(outcome));
                }
            }
        }
    }

    Ok(None)
}
