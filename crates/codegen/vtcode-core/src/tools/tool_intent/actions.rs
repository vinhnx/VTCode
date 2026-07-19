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

/// Determine the action for command_session tool based on args.
/// Returns the action string or None if no inference is possible.
pub fn command_session_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        // Check for standard command fields
        if args.get("command").is_some()
            || args.get("cmd").is_some()
            || args.get("raw_command").is_some()
            || crate::tools::command_args::has_indexed_command_parts(args)
        {
            Some("run")
        } else if args.get("code").is_some() {
            Some("code")
        } else if args.get("input").is_some() || args.get("chars").is_some() || args.get("text").is_some() {
            Some("write")
        } else if args.get("spool_path").is_some()
            || args.get("query").is_some()
            || args.get("head_lines").is_some()
            || args.get("tail_lines").is_some()
            || args.get("max_matches").is_some()
            || args.get("literal").is_some()
        {
            Some("inspect")
        } else if args.get("session_id").is_some() || args.get("s").is_some() {
            Some("poll")
        } else {
            None
        }
    })
}

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
