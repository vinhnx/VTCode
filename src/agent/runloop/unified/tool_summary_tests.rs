use serde_json::json;

use super::tool_summary::is_file_modification_tool;

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
fn test_is_file_modification_tool_unified_file_write() {
    let args = json!({"path": "/tmp/test.txt", "content": "hello"});
    assert!(is_file_modification_tool("unified_file", &args));
}

#[test]
fn test_is_file_modification_tool_unified_file_edit() {
    let args = json!({"path": "/tmp/test.txt", "old_str": "foo", "new_str": "bar"});
    assert!(is_file_modification_tool("unified_file", &args));
}

#[test]
fn test_is_file_modification_tool_unified_file_read() {
    let args = json!({"path": "/tmp/test.txt", "action": "read"});
    assert!(!is_file_modification_tool("unified_file", &args));
}

#[test]
fn test_is_file_modification_tool_read_file() {
    let args = json!({"path": "/tmp/test.txt"});
    assert!(!is_file_modification_tool("read_file", &args));
}

#[test]
fn test_is_file_modification_tool_grep_file() {
    let args = json!({"pattern": "test", "path": "/tmp"});
    assert!(!is_file_modification_tool("grep_file", &args));
}
