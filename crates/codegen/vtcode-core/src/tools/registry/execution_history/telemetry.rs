//! Task-scoped tool telemetry projection.
use super::*;
use std::collections::{BTreeMap, HashMap};

/// Aggregated tool-use telemetry for one repository task.
///
/// The public label maps internal legacy implementation names onto the current
/// model-facing tool surface so exported metrics do not reintroduce removed
/// tool names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTaskTelemetrySnapshot {
    pub task_id: Option<String>,
    pub total_tool_calls: usize,
    pub repeated_equivalent_calls: usize,
    pub failed_tool_calls: usize,
    pub spooled_outputs: usize,
    pub fallback_calls: usize,
    pub read_after_spool_calls: usize,
    pub command_approval_prompts: usize,
    pub task_completed_successfully: Option<bool>,
    pub calls_by_tool: BTreeMap<String, usize>,
}

impl ToolTaskTelemetrySnapshot {
    fn empty(task_id: Option<String>, task_completed_successfully: Option<bool>) -> Self {
        Self {
            task_id,
            total_tool_calls: 0,
            repeated_equivalent_calls: 0,
            failed_tool_calls: 0,
            spooled_outputs: 0,
            fallback_calls: 0,
            read_after_spool_calls: 0,
            command_approval_prompts: 0,
            task_completed_successfully,
            calls_by_tool: BTreeMap::new(),
        }
    }

    /// Export the snapshot as stable JSON for eval reports and trace fixtures.
    pub fn to_json(&self) -> Value {
        json!({
            "task_id": self.task_id,
            "total_tool_calls": self.total_tool_calls,
            "repeated_equivalent_calls": self.repeated_equivalent_calls,
            "failed_tool_calls": self.failed_tool_calls,
            "spooled_outputs": self.spooled_outputs,
            "fallback_calls": self.fallback_calls,
            "read_after_spool_calls": self.read_after_spool_calls,
            "command_approval_prompts": self.command_approval_prompts,
            "task_completed_successfully": self.task_completed_successfully,
            "calls_by_tool": self.calls_by_tool,
        })
    }
}

fn public_tool_telemetry_label(tool_name: &str) -> String {
    match tool_name {
        tools::UNIFIED_EXEC => tools::EXEC_COMMAND.to_string(),
        tools::UNIFIED_FILE => "file_operation".to_string(),
        _ => tool_name.to_string(),
    }
}

fn result_spool_path(record: &ToolExecutionRecord) -> Option<String> {
    record
        .result
        .as_ref()
        .ok()
        .and_then(|value| value.get("spool_path"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn arg_spool_path(record: &ToolExecutionRecord) -> Option<String> {
    record.args.get("spool_path").and_then(Value::as_str).map(str::to_string)
}

fn has_fallback_marker(record: &ToolExecutionRecord) -> bool {
    let Ok(result) = &record.result else {
        return false;
    };
    result.get("fallback_from").is_some()
        || result.get("fallback_to").is_some()
        || result.get("fallback_note").is_some()
}

fn command_requested_approval(record: &ToolExecutionRecord) -> bool {
    let label = public_tool_telemetry_label(&record.tool_name);
    if label != tools::EXEC_COMMAND {
        return false;
    }
    if let Ok(result) = &record.result
        && (result.get("approval_required").and_then(Value::as_bool).unwrap_or(false)
            || result.get("requires_approval").and_then(Value::as_bool).unwrap_or(false)
            || result
                .get("approval_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty()))
    {
        return true;
    }
    let permissions = record
        .args
        .get("sandbox_permissions")
        .and_then(Value::as_str)
        .unwrap_or("use_default");
    matches!(permissions, "require_escalated" | "with_additional_permissions")
}

fn equivalent_call_key(record: &ToolExecutionRecord) -> String {
    let label = public_tool_telemetry_label(&record.tool_name);
    let args = serde_json::to_string(&record.args).unwrap_or_else(|_| "<non-json>".to_string());
    format!("{label}\0{args}")
}

impl ToolExecutionHistory {
    /// Aggregate representative task telemetry from recorded tool calls.
    ///
    /// When `task_id` is `Some`, only records with the same harness task id are
    /// included. When `None`, the snapshot covers all stored records.
    pub fn task_telemetry_snapshot(
        &self,
        task_id: Option<&str>,
        task_completed_successfully: Option<bool>,
    ) -> ToolTaskTelemetrySnapshot {
        let snapshot_task_id = task_id.map(str::to_string);
        let mut snapshot = ToolTaskTelemetrySnapshot::empty(snapshot_task_id, task_completed_successfully);
        let Ok(records) = self.records.read() else {
            return snapshot;
        };

        let mut equivalent_calls_by_key: HashMap<String, usize> = HashMap::new();
        let mut seen_spool_paths: HashMap<String, usize> = HashMap::new();

        for record in records
            .iter()
            .filter(|record| task_id.is_none_or(|expected| record.context.task_id.as_deref() == Some(expected)))
        {
            snapshot.total_tool_calls += 1;
            let label = public_tool_telemetry_label(&record.tool_name);
            *snapshot.calls_by_tool.entry(label).or_default() += 1;

            if !record.success {
                snapshot.failed_tool_calls += 1;
            }
            let arg_spool_path = arg_spool_path(record);
            let result_spool_path = result_spool_path(record);
            if result_spool_path
                .as_deref()
                .is_some_and(|spool_path| arg_spool_path.as_deref() != Some(spool_path))
            {
                snapshot.spooled_outputs += 1;
            }
            if has_fallback_marker(record) {
                snapshot.fallback_calls += 1;
            }
            if command_requested_approval(record) {
                snapshot.command_approval_prompts += 1;
            }

            if let Some(spool_path) = arg_spool_path.as_ref()
                && seen_spool_paths.contains_key(spool_path)
            {
                snapshot.read_after_spool_calls += 1;
            }
            if let Some(spool_path) = result_spool_path
                && arg_spool_path.as_deref() != Some(spool_path.as_str())
            {
                *seen_spool_paths.entry(spool_path).or_default() += 1;
            }

            let count = equivalent_calls_by_key.entry(equivalent_call_key(record)).or_default();
            if *count > 0 {
                snapshot.repeated_equivalent_calls += 1;
            }
            *count += 1;
        }

        snapshot
    }
}
