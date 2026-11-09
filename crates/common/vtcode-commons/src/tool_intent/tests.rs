use super::*;
use serde_json::json;

#[test]
fn indexed_command_parts_probe_only_seeds() {
    assert!(has_indexed_command_parts(&json!({"command.0": "ls"})));
    assert!(has_indexed_command_parts(&json!({"command.1": "ls"})));
    assert!(!has_indexed_command_parts(&json!({"command.2": "ls"})));
    assert!(!has_indexed_command_parts(&json!({"command": "ls"})));
    assert!(!has_indexed_command_parts(&json!([])));
}

#[test]
fn command_session_action_routes_explicit_then_fields() {
    // Explicit action wins over every field probe.
    assert_eq!(command_session_action(&json!({"action": "wait", "command": "ls", "session_id": "s"})), Some("wait"));
    // Field probes route in order: command, code, input, inspect, session.
    assert_eq!(command_session_action(&json!({"command": "ls"})), Some("run"));
    assert_eq!(command_session_action(&json!({"cmd": "ls"})), Some("run"));
    assert_eq!(command_session_action(&json!({"command.0": "ls"})), Some("run"));
    assert_eq!(command_session_action(&json!({"code": "print(1)"})), Some("code"));
    assert_eq!(command_session_action(&json!({"chars": "y"})), Some("write"));
    assert_eq!(command_session_action(&json!({"spool_path": "/tmp/x"})), Some("inspect"));
    assert_eq!(command_session_action(&json!({"session_id": "s"})), Some("poll"));
    assert_eq!(command_session_action(&json!({"s": "s"})), Some("poll"));
    // Nothing inferable stays silent.
    assert_eq!(command_session_action(&json!({"path": "a"})), None);
    assert_eq!(command_session_action(&json!({})), None);
}
