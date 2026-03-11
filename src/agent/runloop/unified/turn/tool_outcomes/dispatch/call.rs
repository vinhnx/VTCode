use anyhow::Result;

use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome,
};

use super::super::handlers::handle_prepared_tool_call;
pub(crate) use super::super::helpers::push_invalid_tool_args_response;

pub(crate) async fn handle_prepared_tool_call_dispatch<'a, 'b>(
    t_ctx: &mut super::super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_call: &PreparedAssistantToolCall,
) -> Result<Option<TurnHandlerOutcome>> {
    handle_prepared_tool_call(t_ctx, tool_call).await
}
