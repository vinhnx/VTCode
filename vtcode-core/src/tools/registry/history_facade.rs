//! Tool execution history accessors for ToolRegistry.

use serde_json::Value;
use std::time::Duration;

use super::{ToolExecutionRecord, ToolRegistry, ToolTaskTelemetrySnapshot};

impl ToolRegistry {
    /// Get recent tool executions (successes and failures).
    pub fn get_recent_tool_records(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_records(count)
    }

    /// Get recent tool execution failures.
    pub fn get_recent_tool_failures(&self, count: usize) -> Vec<ToolExecutionRecord> {
        self.execution_history.get_recent_failures(count)
    }

    /// Aggregate representative task telemetry from recorded tool executions.
    pub fn task_tool_telemetry_snapshot(
        &self,
        task_id: Option<&str>,
        task_completed_successfully: Option<bool>,
    ) -> ToolTaskTelemetrySnapshot {
        self.execution_history
            .task_telemetry_snapshot(task_id, task_completed_successfully)
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

    /// Find a recent successful output for a tool call with identical args.
    pub fn find_recent_successful_output(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        self.execution_history
            .find_recent_successful_result(tool_name, args, max_age)
    }

    /// Find the most recent successful output for a read-only tool call that
    /// targets the same file path, ignoring pagination fields.  Returns `None`
    /// for non-read-only tools or when no path can be extracted.
    pub fn find_recent_successful_by_read_target(
        &self,
        tool_name: &str,
        args: &Value,
        max_age: Duration,
    ) -> Option<Value> {
        self.execution_history
            .find_recent_successful_by_read_target(tool_name, args, max_age)
    }

    /// Find continuation metadata from a recent chunked file-read result for the same path.
    ///
    /// Supports both `read_file` and `unified_file` (read action) history records.
    pub fn find_recent_read_file_spool_progress(
        &self,
        path: &str,
        max_age: Duration,
    ) -> Option<(usize, usize)> {
        self.execution_history
            .find_recent_read_file_spool_progress(path, max_age)
    }

    /// Clear the execution history.
    pub fn clear_execution_history(&self) {
        self.execution_history.clear();
    }

    /// Get the current number of stored execution records.
    pub fn execution_history_len(&self) -> usize {
        self.execution_history.len()
    }
}
