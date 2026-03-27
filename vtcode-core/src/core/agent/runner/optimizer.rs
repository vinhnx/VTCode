use super::AgentRunner;
use crate::core::agent::harness_kernel::reduce_tool_result;
use serde_json::Value;

impl AgentRunner {
    pub(super) fn optimize_tool_result(&self, name: &str, result: Value) -> Value {
        reduce_tool_result(name, result)
    }
}
