use anyhow::anyhow;
use std::sync::Arc;
use tokio::sync::Notify;

use vtcode_core::config::constants::defaults;
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
    // Execute the tool using the pipeline (ref variant avoids cloning args)
    crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref(
        tool_registry,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        defaults::DEFAULT_MAX_TOOL_RETRIES as usize,
    )
    .await
}

/// Execute a batch of tool calls with parallel execution for independent tools
///
/// **Implementation Notes**:
/// - Sequential execution maintains tool output ordering within dependencies
/// - Tools can run in parallel when they don't reference each other's results
/// - If all tools are independent, all execute concurrently
/// - Progress reporting is per-tool to avoid contention
/// - Execution order of results matches input order for consistency
#[allow(dead_code)]
pub(crate) async fn execute_tool_pipeline(
    tool_calls: &[vtcode_core::llm::provider::ToolCall],
    tool_registry: &mut ToolRegistry,
    ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Vec<crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus> {
    // Note: True parallel execution requires Arc<ToolRegistry> or refactoring to avoid
    // `&mut` borrow. Current implementation sequentially executes tool calls to maintain
    // compatibility with the registry's mutable interface.
    //
    // Future optimization paths:
    // 1. Move to Arc<ToolRegistry> and Interior mutability if registry becomes thread-safe
    // 2. Pre-validate all tool calls and split into independent batches
    // 3. Use rayon for CPU-bound tools, tokio::spawn for I/O-bound tools

    let mut results = Vec::with_capacity(tool_calls.len());

    for (idx, tool_call) in tool_calls.iter().enumerate() {
        let Some(function) = tool_call.function.as_ref() else {
            results.push(
                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Failure {
                    error: anyhow!("Tool call #{} missing function payload", idx),
                },
            );
            continue;
        };

        let name = &function.name;
        let args_val = match tool_call.parsed_arguments() {
            Ok(val) => val,
            Err(err) => {
                results.push(
                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Failure {
                        error: anyhow!("Tool call #{} ({}) parse error: {}", idx, name, err),
                    },
                );
                continue;
            }
        };

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
