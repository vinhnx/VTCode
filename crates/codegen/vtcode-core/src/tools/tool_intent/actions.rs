use serde_json::Value;

use crate::config::constants::tools;

/// Determine the action for file_operation tool based on args.
/// Returns the action string or a default if inference is possible.
pub fn file_operation_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        let has_read_path = args.get("path").is_some()
            || args.get("file_path").is_some()
            || args.get("filepath").is_some()
            || args.get("target_path").is_some()
            || args.get("file").is_some()
            || args.get("p").is_some();

        if args.get("old_str").is_some() {
            Some("edit")
        } else if args.get("content").is_some() {
            Some("write")
        } else if args.get("destination").is_some() {
            Some("move")
        } else if has_read_path {
            Some("read")
        } else {
            None
        }
    })
}

// Canonical probe lives in `vtcode-commons::tool_intent` so transcript
// surfaces share it; re-exported here so dispatch import paths are untouched.
pub use vtcode_commons::tool_intent::command_session_action;

fn action_matches(action: Option<&str>, expected: &str) -> bool {
    action.is_some_and(|candidate| candidate.eq_ignore_ascii_case(expected))
}

pub fn action_matches_any(action: Option<&str>, expected: &[&str]) -> bool {
    action.is_some_and(|candidate| {
        expected
            .iter()
            .any(|expected_action| candidate.eq_ignore_ascii_case(expected_action))
    })
}

pub fn file_operation_action_is(args: &Value, expected: &str) -> bool {
    action_matches(file_operation_action(args), expected)
}

pub fn file_operation_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(file_operation_action(args), expected)
}

pub fn command_session_action_is(args: &Value, expected: &str) -> bool {
    action_matches(command_session_action(args), expected)
}

pub fn command_session_action_in(args: &Value, expected: &[&str]) -> bool {
    action_matches_any(command_session_action(args), expected)
}

/// Return the explicit action requested through the consolidated MCP tool.
pub fn mcp_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(Value::as_str)
}

pub fn mcp_action_is(args: &Value, expected: &str) -> bool {
    action_matches(mcp_action(args), expected)
}

/// Return the policy identity for actions whose risk differs from their
/// otherwise low-risk parent tool.
pub fn action_qualified_policy_name(tool_name: &str, args: Option<&Value>) -> Option<&'static str> {
    if tool_name == tools::MCP_CONNECT_SERVER
        || (tool_name == tools::MCP && args.is_some_and(|args| mcp_action_is(args, "connect")))
    {
        return Some("mcp:connect");
    }

    if tool_name == tools::MCP_DISCONNECT_SERVER
        || (tool_name == tools::MCP && args.is_some_and(|args| mcp_action_is(args, "disconnect")))
    {
        return Some("mcp:disconnect");
    }

    None
}
