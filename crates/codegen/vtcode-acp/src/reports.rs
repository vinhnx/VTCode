use crate::acp;
use serde_json::{Value, json};
use std::path::PathBuf;

pub(crate) const TOOL_FAILURE_PREFIX: &str = "Tool execution failed";
pub(crate) const TOOL_SUCCESS_LABEL: &str = "success";
pub(crate) const TOOL_ERROR_LABEL: &str = "error";
pub(crate) const TOOL_RESPONSE_KEY_STATUS: &str = "status";
pub(crate) const TOOL_RESPONSE_KEY_TOOL: &str = "tool";
pub(crate) const TOOL_RESPONSE_KEY_PATH: &str = "path";
pub(crate) const TOOL_RESPONSE_KEY_CONTENT: &str = "content";
pub(crate) const TOOL_RESPONSE_KEY_TRUNCATED: &str = "truncated";
const TOOL_RESPONSE_KEY_MESSAGE: &str = "message";

const TOOL_EXECUTION_CANCELLED_MESSAGE: &str = "Tool execution cancelled at the client's request";
pub const TOOL_PERMISSION_ALLOW_OPTION_ID: &str = "allow-once";
pub const TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID: &str = "allow-always";
pub const TOOL_PERMISSION_DENY_OPTION_ID: &str = "reject-once";
pub const TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID: &str = "reject-always";
pub(crate) const TOOL_PERMISSION_ALLOW_PREFIX: &str = "Allow";
pub(crate) const TOOL_PERMISSION_DENY_PREFIX: &str = "Deny";
pub const TOOL_PERMISSION_DENIED_MESSAGE: &str = "Tool execution cancelled: permission denied by the user";
pub const TOOL_PERMISSION_CANCELLED_MESSAGE: &str = "Tool execution cancelled: permission request interrupted";
pub(crate) const TOOL_PERMISSION_REQUEST_FAILURE_LOG: &str =
    "Failed to request ACP tool permission, cancelling the tool invocation";
pub(crate) const TOOL_PERMISSION_UNKNOWN_OPTION_LOG: &str = "Received unsupported ACP permission option selection";
pub const TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE: &str = "Tool execution cancelled: permission request failed";

pub struct ToolExecutionReport {
    pub(crate) status: acp::ToolCallStatus,
    pub(crate) llm_response: String,
    pub(crate) content: Vec<acp::ToolCallContent>,
    pub(crate) locations: Vec<acp::ToolCallLocation>,
    pub(crate) raw_output: Option<Value>,
}

impl ToolExecutionReport {
    pub(crate) fn success(
        content: Vec<acp::ToolCallContent>,
        locations: Vec<acp::ToolCallLocation>,
        payload: Value,
    ) -> Self {
        Self {
            status: acp::ToolCallStatus::Completed,
            llm_response: payload.to_string(),
            content,
            locations,
            raw_output: Some(payload),
        }
    }

    /// Build the report for a local tool that returned registry output.
    ///
    /// Exec tools return `Ok` with `exit_code` in the payload even when the
    /// command exits non-zero, so the ACP call status must be derived from the
    /// payload instead of being assumed successful.
    pub(crate) fn from_local_output(
        tool_name: &str,
        output: Value,
        content: Vec<acp::ToolCallContent>,
        locations: Vec<acp::ToolCallLocation>,
    ) -> Self {
        let exit_failed = output.get("exit_code").and_then(Value::as_i64).is_some_and(|code| code != 0);
        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: if exit_failed { TOOL_ERROR_LABEL } else { TOOL_SUCCESS_LABEL },
            TOOL_RESPONSE_KEY_TOOL: tool_name,
            "result": output,
        });
        Self {
            status: if exit_failed {
                acp::ToolCallStatus::Failed
            } else {
                acp::ToolCallStatus::Completed
            },
            llm_response: payload.to_string(),
            content,
            locations,
            raw_output: Some(payload),
        }
    }

    pub(crate) fn failure(tool_name: &str, message: &str) -> Self {
        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_ERROR_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tool_name,
            TOOL_RESPONSE_KEY_MESSAGE: message,
        });
        Self {
            status: acp::ToolCallStatus::Failed,
            llm_response: payload.to_string(),
            content: vec![acp::ToolCallContent::from(format!("{TOOL_FAILURE_PREFIX}: {message}"))],
            locations: Vec::new(),
            raw_output: Some(payload),
        }
    }

    pub(crate) fn cancelled(tool_name: &str) -> Self {
        Self::failure(tool_name, TOOL_EXECUTION_CANCELLED_MESSAGE)
    }
}

pub(crate) fn create_diff_content(path: &str, old_text: Option<&str>, new_text: &str) -> acp::ToolCallContent {
    acp::ToolCallContent::Diff(
        acp::Diff::new(PathBuf::from(path), new_text.to_string()).old_text(old_text.map(|s| s.to_string())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::constants::tools;

    fn exec_payload(exit_code: i64) -> Value {
        json!({
            "success": true,
            "output": "MARKER_7319",
            "exit_code": exit_code,
            "session_id": "run-42",
        })
    }

    #[test]
    fn non_zero_exit_code_reports_a_failed_tool_call() {
        let report =
            ToolExecutionReport::from_local_output(tools::EXEC_COMMAND, exec_payload(3), Vec::new(), Vec::new());

        assert_eq!(report.status, acp::ToolCallStatus::Failed);
        let payload: Value = serde_json::from_str(&report.llm_response).expect("payload should be JSON");
        assert_eq!(payload[TOOL_RESPONSE_KEY_STATUS], json!(TOOL_ERROR_LABEL));
        assert_eq!(payload["result"]["exit_code"], json!(3));
        assert_eq!(payload["result"]["output"], json!("MARKER_7319"));
    }

    #[test]
    fn zero_exit_code_reports_a_completed_tool_call() {
        let report =
            ToolExecutionReport::from_local_output(tools::EXEC_COMMAND, exec_payload(0), Vec::new(), Vec::new());

        assert_eq!(report.status, acp::ToolCallStatus::Completed);
        let payload: Value = serde_json::from_str(&report.llm_response).expect("payload should be JSON");
        assert_eq!(payload[TOOL_RESPONSE_KEY_STATUS], json!(TOOL_SUCCESS_LABEL));
    }

    #[test]
    fn payload_without_an_exit_code_stays_completed() {
        let report = ToolExecutionReport::from_local_output(
            tools::APPLY_PATCH,
            json!({ "path": "hello.txt", "status": "success" }),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(report.status, acp::ToolCallStatus::Completed);
    }
}
