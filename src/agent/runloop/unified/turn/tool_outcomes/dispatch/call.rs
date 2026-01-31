use anyhow::Result;

use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnProcessingContext,
};

use super::super::handlers::handle_single_tool_call;

pub(crate) async fn handle_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call: &uni::ToolCall,
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    let function = tool_call
        .function
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
    let tool_name = &function.name;
    let args_val = tool_call
        .parsed_arguments()
        .unwrap_or_else(|_| serde_json::json!({}));

    handle_single_tool_call(
        ctx,
        tool_call.id.clone(),
        tool_name,
        args_val,
        repeated_tool_attempts,
        turn_modified_files,
        traj,
    ).await
}
