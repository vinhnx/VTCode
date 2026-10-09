#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
pub(crate) use super::execution_attempts::execute_tool_with_timeout;
pub(crate) use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
pub(crate) use super::execution_run::{run_tool_call, run_tool_call_with_args};
#[cfg(test)]
use super::{execution_helpers, status::ToolExecutionStatus};

#[cfg(test)]
pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    execution_helpers::process_llm_tool_output(output)
}
