use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::tools::registry::ToolRegistry;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) use super::execution_attempts::{
    execute_tool_with_timeout_ref, execute_tool_with_timeout_ref_prevalidated,
};
pub(crate) use super::execution_run::{run_tool_call, run_tool_call_with_args};
use super::{execution_attempts, execution_helpers, status::ToolExecutionStatus};

#[allow(dead_code)]
pub(crate) async fn execute_tool_with_timeout(
    registry: &ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    execution_attempts::execute_tool_with_timeout(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        max_tool_retries,
    )
    .await
}

pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    execution_helpers::process_llm_tool_output(output)
}
