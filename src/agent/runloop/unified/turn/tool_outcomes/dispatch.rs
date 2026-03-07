use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use anyhow::Result;
use call::{handle_preparsed_tool_call, handle_tool_call, push_invalid_tool_args_response};
use hashbrown::HashMap;
use std::sync::Arc;
use std::time::Instant;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tool_intent;

mod call;

pub(crate) async fn handle_tool_calls<'a, 'b>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_calls: &[uni::ToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    let mut calls_by_id: HashMap<String, (&uni::ToolCall, String, Arc<serde_json::Value>)> =
        HashMap::with_capacity(tool_calls.len());

    let mut planner_calls = Vec::with_capacity(tool_calls.len());
    let planning_started = Instant::now();
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
        let parsed_args = Arc::new(parsed_args);

        planner_calls.push((name.clone(), Arc::clone(&parsed_args), tc.id.clone()));
        calls_by_id.insert(tc.id.clone(), (tc, name, parsed_args));
    }

    if planner_calls.is_empty() {
        return Ok(None);
    }

    let can_parallelize = t_ctx.ctx.full_auto
        && planner_calls.len() > 1
        && planner_calls
            .iter()
            .all(|(name, args, _)| tool_intent::is_parallel_safe_call(name, args.as_ref()));
    let groups = if can_parallelize {
        1
    } else {
        planner_calls.len()
    };
    let total_grouped_calls = planner_calls.len();
    let max_group_size = if can_parallelize {
        planner_calls.len()
    } else {
        1
    };
    let parallel_group_count = usize::from(can_parallelize);
    tracing::debug!(
        target: "vtcode.turn.metrics",
        metric = "tool_dispatch_plan",
        groups,
        total_calls = total_grouped_calls,
        max_group_size,
        parallel_groups = parallel_group_count,
        planning_ms = planning_started.elapsed().as_millis(),
        "turn metric"
    );

    if can_parallelize {
        let mut parsed_group_calls = Vec::with_capacity(planner_calls.len());
        for (_, _, call_id) in &planner_calls {
            if let Some((tc, _, args)) = calls_by_id.remove(call_id) {
                parsed_group_calls.push(
                    crate::agent::runloop::unified::turn::tool_outcomes::handlers::ParsedToolCall {
                        tool_call: tc,
                        args,
                    },
                );
            }
        }

        let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch_parsed(t_ctx, parsed_group_calls).await?;
        if let Some(o) = outcome {
            return Ok(Some(o));
        }
    } else {
        for (_, _, call_id) in &planner_calls {
            if let Some((tc, name, args)) = calls_by_id.remove(call_id) {
                let outcome =
                    handle_preparsed_tool_call(t_ctx, tc.id.clone(), &name, (*args).clone())
                        .await?;
                if let Some(o) = outcome {
                    return Ok(Some(o));
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
