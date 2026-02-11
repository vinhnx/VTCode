use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use anyhow::Result;
use call::handle_tool_call;
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
        let is_parallel = group.len() > 1 && t_ctx.ctx.full_auto;
        if is_parallel {
            let all_parallel_safe = group.tool_calls.iter().all(|(name, _, _)| {
                vtcode_core::tools::parallel_tool_batch::ParallelToolBatch::is_parallel_safe(name)
            });
            if !all_parallel_safe {
                for (_, _, call_id) in &group.tool_calls {
                    if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                        let outcome = handle_tool_call(t_ctx, tc).await?;
                        if let Some(o) = outcome {
                            return Ok(Some(o));
                        }
                    }
                }
                continue;
            }
            // HP-5: Implement true parallel execution for non-conflicting groups in full-auto mode
            let mut group_tool_calls = Vec::with_capacity(group.len());
            for (_, _, call_id) in &group.tool_calls {
                if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                    group_tool_calls.push(tc);
                }
            }
            // 2. Execute parallel tools using centralized batch handler
            let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch(
                t_ctx,
                &group_tool_calls,
            ).await?;
            if let Some(o) = outcome {
                return Ok(Some(o));
            }
        } else {
            for (_, _, call_id) in &group.tool_calls {
                if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                    let outcome = handle_tool_call(t_ctx, tc).await?;
                    if let Some(o) = outcome {
                        return Ok(Some(o));
                    }
                }
            }
        }
    }

    Ok(None)
}
