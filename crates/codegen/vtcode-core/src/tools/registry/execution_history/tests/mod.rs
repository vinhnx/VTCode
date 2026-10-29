pub(super) use super::*;
pub(super) use serde_json::json;
pub(super) use tempfile::tempdir;

fn make_snapshot() -> HarnessContextSnapshot {
    HarnessContextSnapshot::new("session_test".to_string(), None)
}

fn make_task_snapshot(task_id: &str) -> HarnessContextSnapshot {
    HarnessContextSnapshot::new("session_test".to_string(), Some(task_id.to_string()))
}

fn loop_test_record(tool_name: &str, args: Value) -> ToolExecutionRecord {
    ToolExecutionRecord::success(
        tool_name.to_string(),
        tool_name.to_string(),
        false,
        None,
        args,
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    )
}

mod code_search_replay;
mod exec_slice;
mod read_target;
mod read_tracking;
mod records_telemetry;
