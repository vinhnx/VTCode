//! Tool execution history accessors for ToolRegistry.

use super::{ToolExecutionRecord, ToolRegistry};

impl ToolRegistry {
    /// Get recent tool execution records.
    pub fn get_recent_tool_executions(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool executions (successes and failures).
    pub fn get_recent_tool_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool execution failures.
    pub fn get_recent_tool_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_failures(count)
    }

    /// Clear the execution history.
    pub fn clear_execution_history(&self) {
        self.execution_history.clear();
    }
}
