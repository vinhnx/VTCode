use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use anyhow::Result;
use call::{handle_preparsed_tool_call, handle_tool_call, push_invalid_tool_args_response};
use std::collections::HashMap;
use std::sync::Arc;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::parallel_executor::ParallelExecutionPlanner;

mod call;

pub(crate) async fn handle_tool_calls<'a, 'b>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_calls: &[uni::ToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    let mut calls_by_id: HashMap<String, (&uni::ToolCall, String, serde_json::Value)> =
        HashMap::with_capacity(tool_calls.len());

    // HP-4: Use ParallelExecutionPlanner to group independent tool calls
    let planner = ParallelExecutionPlanner::new();
    let mut planner_calls = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        let Some(function) = tc.function.as_ref() else {
            continue;
        };
        let name = function.name.clone();
        let parsed_args = match tc.parsed_arguments() {
            Ok(args) => args,
            Err(err) => {
                push_invalid_tool_args_response(
                    t_ctx.ctx.working_history,
                    tc.id.clone(),
                    &function.name,
                    &err.to_string(),
                );
                continue;
            }
        };

        planner_calls.push((name.clone(), Arc::new(parsed_args.clone()), tc.id.clone()));
        calls_by_id.insert(tc.id.clone(), (tc, name, parsed_args));
    }

    if planner_calls.is_empty() {
        return Ok(None);
    }

    let groups = planner.plan(&planner_calls);

    for group in groups {
        let is_parallel = group.len() > 1 && t_ctx.ctx.full_auto;
        if is_parallel {
            let all_parallel_safe = group.tool_calls.iter().all(|(name, _, _)| {
                vtcode_core::tools::parallel_tool_batch::ParallelToolBatch::is_parallel_safe(name)
            });
            if !all_parallel_safe {
                for (_, _, call_id) in &group.tool_calls {
                    if let Some((tc, name, args)) = calls_by_id.remove(call_id) {
                        let outcome =
                            handle_preparsed_tool_call(t_ctx, tc.id.clone(), &name, args).await?;
                        if let Some(o) = outcome {
                            return Ok(Some(o));
                        }
                    }
                }
                continue;
            }
            // HP-5: Implement true parallel execution for non-conflicting groups in full-auto mode
            let mut parsed_group_calls = Vec::with_capacity(group.len());
            for (_, _, call_id) in &group.tool_calls {
                if let Some((tc, _, args)) = calls_by_id.remove(call_id) {
                    parsed_group_calls.push(
                        crate::agent::runloop::unified::turn::tool_outcomes::handlers::ParsedToolCall {
                            tool_call: tc,
                            args,
                        },
                    );
                }
            }
            // 2. Execute parallel tools using centralized batch handler
            let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch_parsed(t_ctx, parsed_group_calls).await?;
            if let Some(o) = outcome {
                return Ok(Some(o));
            }
        } else {
            for (_, _, call_id) in &group.tool_calls {
                if let Some((tc, name, args)) = calls_by_id.remove(call_id) {
                    let outcome =
                        handle_preparsed_tool_call(t_ctx, tc.id.clone(), &name, args).await?;
                    if let Some(o) = outcome {
                        return Ok(Some(o));
                    }
                }
            }
        }
    }

    for (_, (tc, _, _)) in calls_by_id {
        // Fallback to existing path if planner omitted a call unexpectedly.
        let outcome = handle_tool_call(t_ctx, tc).await?;
        if let Some(o) = outcome {
            return Ok(Some(o));
        }
    }

    Ok(None)
}
