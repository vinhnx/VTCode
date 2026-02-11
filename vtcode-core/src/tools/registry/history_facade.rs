//! Tool execution history accessors for ToolRegistry.

use serde_json::Value;
use std::time::Duration;

use super::{ToolExecutionRecord, ToolRegistry};

impl ToolRegistry {
    /// Get recent tool executions (successes and failures).
    pub fn get_recent_tool_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool execution failures.
    pub fn get_recent_tool_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_failures(count)
    }

    /// Find a recent spooled output for a tool call with identical args.
    pub fn find_recent_spooled_output(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        self.execution_history
            .find_recent_spooled_result(tool_name, args, max_age)
    }

    /// Clear the execution history.
    pub fn clear_execution_history(&self) {
        self.execution_history.clear();
    }
}
