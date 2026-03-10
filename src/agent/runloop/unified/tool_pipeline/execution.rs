#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use tokio::sync::Notify;
#[cfg(test)]
use vtcode_core::tools::registry::ToolRegistry;

#[cfg(test)]
use crate::agent::runloop::unified::progress::ProgressReporter;
#[cfg(test)]
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
pub(crate) use super::execution_run::{run_tool_call, run_tool_call_with_args};
#[cfg(test)]
use super::{execution_attempts, execution_helpers, status::ToolExecutionStatus};

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    execution_helpers::process_llm_tool_output(output)
}
