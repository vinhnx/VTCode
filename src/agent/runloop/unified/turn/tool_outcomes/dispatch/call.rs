use anyhow::Result;

use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;

use super::super::handlers::handle_single_tool_call;
use super::super::helpers::push_tool_response;

pub(crate) async fn handle_tool_call<'a, 'b>(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_call: &uni::ToolCall,
) -> Result<Option<TurnHandlerOutcome>> {
    let function = tool_call
        .function
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
    let tool_name = &function.name;
    let args_val = match tool_call.parsed_arguments() {
        Ok(args) => args,
        Err(err) => {
            let payload = serde_json::json!({
                "error": format!(
                    "Invalid tool arguments for '{}': {}",
                    tool_name,
                    err
                )
            });
            push_tool_response(
                t_ctx.ctx.working_history,
                tool_call.id.clone(),
                payload.to_string(),
            );
            return Ok(None);
        }
    };

    handle_single_tool_call(t_ctx, tool_call.id.clone(), tool_name, args_val).await
}
