use anyhow::Result;
use std::future::Future;

use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome,
};

use super::super::handlers::handle_prepared_tool_call;
pub(crate) use super::super::helpers::push_invalid_tool_args_response;

pub(crate) fn handle_prepared_tool_call_dispatch<'a, 'b, 'c>(
    t_ctx: &'c mut super::super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_call: &'c PreparedAssistantToolCall,
) -> impl Future<Output = Result<Option<TurnHandlerOutcome>>> + 'c {
    handle_prepared_tool_call(t_ctx, tool_call)
}
