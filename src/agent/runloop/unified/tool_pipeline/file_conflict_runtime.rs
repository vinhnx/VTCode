use serde_json::Value;
use vtcode_core::tools::edited_file_monitor::{
    FILE_CONFLICT_DETECTED_FIELD, FILE_CONFLICT_PATH_FIELD,
};
use vtcode_core::tools::tool_intent::is_edited_file_conflict_guarded_call;

use super::status::ToolExecutionStatus;

#[derive(Clone, Debug)]
pub(super) struct PendingFileConflictStatus {
    pub(super) output: Value,
    pub(super) display_path: String,
    pub(super) message: String,
    pub(super) approved_snapshot: Option<Value>,
    pub(super) disk_content: Option<String>,
    pub(super) intended_content: Option<String>,
    pub(super) emit_hitl_notification: bool,
}

impl PendingFileConflictStatus {
    pub(super) fn from_output(output: Value) -> Option<Self> {
        if output
            .get(FILE_CONFLICT_DETECTED_FIELD)
            .and_then(Value::as_bool)
            != Some(true)
        {
            return None;
        }

        let display_path = output
            .get(FILE_CONFLICT_PATH_FIELD)
            .and_then(Value::as_str)?
            .to_string();
        let message = output
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("File changed on disk since the agent last read it.")
            .to_string();
        let approved_snapshot = output
            .get("disk_snapshot")
            .cloned()
            .filter(Value::is_object);
        let disk_content = output
            .get("disk_content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let intended_content = output
            .get("intended_content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let emit_hitl_notification = output
            .get("emit_hitl_notification")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        Some(Self {
            output,
            display_path,
            message,
            approved_snapshot,
            disk_content,
            intended_content,
            emit_hitl_notification,
        })
    }
}

#[derive(Debug)]
pub(super) enum RuntimeToolExecution {
    Completed(ToolExecutionStatus),
    PendingFileConflict(PendingFileConflictStatus),
}

pub(super) fn into_runtime_tool_execution(
    name: &str,
    args: &Value,
    status: ToolExecutionStatus,
) -> RuntimeToolExecution {
    if !is_edited_file_conflict_guarded_call(name, args) {
        return RuntimeToolExecution::Completed(status);
    }

    match status {
        ToolExecutionStatus::Success {
            output,
            stdout,
            modified_files,
            command_success,
        } => {
            if output
                .get(FILE_CONFLICT_DETECTED_FIELD)
                .and_then(Value::as_bool)
                == Some(true)
                && let Some(conflict) = PendingFileConflictStatus::from_output(output.clone())
            {
                return RuntimeToolExecution::PendingFileConflict(conflict);
            }

            RuntimeToolExecution::Completed(ToolExecutionStatus::Success {
                output,
                stdout,
                modified_files,
                command_success,
            })
        }
        other => RuntimeToolExecution::Completed(other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools;

    use super::{RuntimeToolExecution, into_runtime_tool_execution};
    use crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus;

    fn success(output: serde_json::Value) -> ToolExecutionStatus {
        ToolExecutionStatus::Success {
            output,
            stdout: None,
            modified_files: vec![],
            command_success: true,
        }
    }

    #[test]
    fn unrelated_tools_do_not_enter_file_conflict_flow() {
        let output = json!({
            "success": true,
            "conflict_detected": true,
            "conflict_path": "sample.txt"
        });

        let wrapped = into_runtime_tool_execution(
            tools::READ_FILE,
            &json!({"path": "sample.txt"}),
            success(output.clone()),
        );

        match wrapped {
            RuntimeToolExecution::Completed(ToolExecutionStatus::Success {
                output: observed,
                ..
            }) => assert_eq!(observed, output),
            other => panic!("unexpected runtime execution: {other:?}"),
        }
    }

    #[test]
    fn unified_file_read_bypasses_file_conflict_flow() {
        let output = json!({
            "success": true,
            "conflict_detected": true,
            "conflict_path": "sample.txt"
        });

        let wrapped = into_runtime_tool_execution(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "sample.txt"}),
            success(output.clone()),
        );

        match wrapped {
            RuntimeToolExecution::Completed(ToolExecutionStatus::Success {
                output: observed,
                ..
            }) => assert_eq!(observed, output),
            other => panic!("unexpected runtime execution: {other:?}"),
        }
    }

    #[test]
    fn create_file_conflicts_are_wrapped() {
        let wrapped = into_runtime_tool_execution(
            tools::CREATE_FILE,
            &json!({"path": "sample.txt", "content": "agent"}),
            success(json!({
                "success": true,
                "conflict_detected": true,
                "conflict_path": "sample.txt",
                "disk_content": "external\n",
                "intended_content": "agent\n",
            })),
        );

        match wrapped {
            RuntimeToolExecution::PendingFileConflict(conflict) => {
                assert_eq!(conflict.display_path, "sample.txt");
                assert_eq!(conflict.disk_content.as_deref(), Some("external\n"));
                assert_eq!(conflict.intended_content.as_deref(), Some("agent\n"));
            }
            other => panic!("unexpected runtime execution: {other:?}"),
        }
    }
}
