use crate::acp;
use serde_json::{Value, json};
use std::path::PathBuf;

pub(crate) const TOOL_FAILURE_PREFIX: &str = "Tool execution failed";
pub(crate) const TOOL_SUCCESS_LABEL: &str = "success";
const TOOL_ERROR_LABEL: &str = "error";
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
