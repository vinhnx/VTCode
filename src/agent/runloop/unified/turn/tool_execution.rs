use std::sync::Arc;
use tokio::sync::Notify;

use vtcode_core::tools::registry::ToolRegistry;

/// Execute a single tool call after permissions have been approved
#[allow(dead_code)]
pub(crate) async fn execute_single_tool_call(
    tool_registry: &mut ToolRegistry,
    name: &str,
    args_val: &serde_json::Value,
    ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&crate::agent::runloop::unified::progress::ProgressReporter>,
) -> crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus {
    // Execute the tool using the pipeline
    crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout(
        tool_registry,
        name,
        args_val.clone(),
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
    )
    .await
}

/// Execute a batch of tool calls with simplified pipeline
#[allow(dead_code)]
pub(crate) async fn execute_tool_pipeline(
    tool_calls: &[vtcode_core::llm::provider::ToolCall],
    tool_registry: &mut ToolRegistry,
    ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Vec<crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus> {
    let mut results = Vec::new();

    // For now, just execute each tool call sequentially
    // TODO: Add parallel execution and batching logic
    for tool_call in tool_calls {
        let name = &tool_call
            .function
            .as_ref()
            .expect("Tool call must have function")
            .name;
        let args_val = tool_call
            .parsed_arguments()
            .unwrap_or_else(|_| serde_json::json!({}));

        // Execute the single tool call
        let result = execute_single_tool_call(
            tool_registry,
            name,
            &args_val,
            ctrl_c_state,
            ctrl_c_notify,
            Some(&crate::agent::runloop::unified::progress::ProgressReporter::new()),
        )
        .await;

        results.push(result);
    }

    results
}
