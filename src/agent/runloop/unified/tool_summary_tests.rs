use serde_json::json;
use std::path::Path;
use vtcode_core::config::constants::tools;

use super::tool_summary::{describe_tool_action, is_file_modification_tool};

#[test]
fn test_describe_tool_action_exec_command_shows_command_text() {
    let args = json!({"command": "cargo check"});
    let (desc, _) = describe_tool_action(tools::EXEC_COMMAND, &args, None::<&Path>);
    assert!(desc.contains("cargo check"), "exec_command summary must show the command text, got: {desc}");
    assert!(
        !desc.contains("exec_command"),
        "exec_command summary must not fall back to the bare tool name, got: {desc}"
    );
}

#[test]
fn test_is_file_modification_tool_write_file() {
    let args = json!({"path": "/tmp/test.txt", "content": "hello"});
    assert!(is_file_modification_tool("write_file", &args));
}

#[test]
fn test_is_file_modification_tool_edit_file() {
    let args = json!({"path": "/tmp/test.txt", "old_str": "foo", "new_str": "bar"});
    assert!(is_file_modification_tool("edit_file", &args));
}

#[test]
fn test_is_file_modification_tool_apply_patch() {
    let args = json!({"path": "/tmp/test.txt", "patch": "diff content"});
    assert!(is_file_modification_tool("apply_patch", &args));
}

#[test]
fn test_is_file_modification_tool_file_operation_write() {
    let args = json!({"path": "/tmp/test.txt", "content": "hello"});
    assert!(is_file_modification_tool("file_operation", &args));
}

#[test]
fn test_is_file_modification_tool_file_operation_edit() {
    let args = json!({"path": "/tmp/test.txt", "old_str": "foo", "new_str": "bar"});
    assert!(is_file_modification_tool("file_operation", &args));
}

#[test]
fn test_is_file_modification_tool_file_operation_read() {
    let args = json!({"path": "/tmp/test.txt", "action": "read"});
    assert!(!is_file_modification_tool("file_operation", &args));
}

#[test]
fn test_is_file_modification_tool_read_file() {
    let args = json!({"path": "/tmp/test.txt"});
    assert!(!is_file_modification_tool("read_file", &args));
}

#[test]
fn test_is_file_modification_tool_grep_file() {
    let args = json!({"pattern": "test", "path": "/tmp"});
    assert!(!is_file_modification_tool(tools::GREP_FILE, &args));
}
