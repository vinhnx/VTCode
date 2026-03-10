use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use anyhow::Result;
use call::{handle_preparsed_tool_call, push_invalid_tool_args_response};
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

    let mut parsed_calls = Vec::with_capacity(tool_calls.len());
    let planning_started = Instant::now();
    for tc in tool_calls {
        let Some(function) = tc.function.as_ref() else {
            continue;
        };
        let parsed_args = match tc.parsed_arguments() {
            Ok(args) => args,
            Err(err) => {
                push_invalid_tool_args_response(
                    t_ctx.ctx.working_history,
                    tc.id.as_str(),
                    &function.name,
                    &err.to_string(),
                );
                continue;
            }
        };

        parsed_calls.push(
            crate::agent::runloop::unified::turn::tool_outcomes::handlers::ParsedToolCall {
                call_id: tc.id.as_str(),
                tool_name: function.name.as_str(),
                args: parsed_args,
            },
        );
    }

    if parsed_calls.is_empty() {
        return Ok(None);
    }

    let can_parallelize = t_ctx.ctx.full_auto
        && parsed_calls.len() > 1
        && parsed_calls
            .iter()
            .all(|call| tool_intent::is_parallel_safe_call(call.tool_name, &call.args));
    let groups = if can_parallelize {
        1
    } else {
        parsed_calls.len()
    };
    let total_grouped_calls = parsed_calls.len();
    let max_group_size = if can_parallelize {
        parsed_calls.len()
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
        let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch_parsed(t_ctx, parsed_calls).await?;
        if let Some(o) = outcome {
            return Ok(Some(o));
        }
    } else {
        for parsed_call in parsed_calls {
            let outcome = handle_preparsed_tool_call(
                t_ctx,
                parsed_call.call_id,
                parsed_call.tool_name,
                parsed_call.args,
            )
            .await?;
            if let Some(o) = outcome {
                return Ok(Some(o));
            }
        }
    }

    Ok(None)
}
