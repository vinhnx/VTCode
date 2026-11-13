use serde_json::json;

use super::{
    ExecOutputPreview, PtyEphemeralCapture, attach_exec_response_context, attach_pty_continuation, build_exec_response,
    build_exec_session_command_display,
};
use crate::tools::types::VTCodeExecSession;

#[test]
fn build_exec_session_command_display_unwraps_shell_c_argument() {
    let session = VTCodeExecSession {
        id: "run-123".to_string().into(),
        backend: "pty".to_string(),
        command: "zsh".to_string(),
        args: vec!["-l".to_string(), "-c".to_string(), "cargo check".to_string()],
        working_dir: Some(".".to_string()),
        background: false,
        rows: Some(24),
        cols: Some(80),
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    };

    assert_eq!(build_exec_session_command_display(&session), "cargo check");
}

#[test]
fn attach_exec_response_context_sets_expected_keys() {
    let mut response = json!({ "output": "ok" });
    let session = VTCodeExecSession {
        id: "run-123".to_string().into(),
        backend: "pty".to_string(),
        command: "zsh".to_string(),
        args: vec!["-l".to_string(), "-c".to_string(), "cargo check".to_string()],
        working_dir: Some(".".to_string()),
        background: false,
        rows: Some(30),
        cols: Some(120),
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    };

    attach_exec_response_context(&mut response, &session, "cargo check", false);

    assert_eq!(response["session_id"], "run-123");
    assert_eq!(response["command"], "cargo check");
    assert_eq!(response["working_directory"], ".");
    assert_eq!(response["backend"], "pty");
    assert_eq!(response["rows"], 30);
    assert_eq!(response["cols"], 120);
    assert_eq!(response["is_exited"], false);
}

#[test]
fn attach_pty_continuation_compacts_next_continue_args() {
    let mut response = json!({ "output": "ok" });
    attach_pty_continuation(&mut response, "run-123");

    assert!(response.get("follow_up_prompt").is_none());
    assert!(response.get("next_poll_args").is_none());
    assert_eq!(response["next_continue_args"], json!({ "session_id": "run-123" }));
    assert!(response.get("preferred_next_action").is_none());
}

#[test]
fn attach_pty_continuation_keeps_payload_compact() {
    let mut response = json!({ "output": "ok" });
    attach_pty_continuation(&mut response, "run-123");

    assert!(response.get("follow_up_prompt").is_none());
    assert!(response.get("next_poll_args").is_none());
    assert_eq!(response["next_continue_args"], json!({ "session_id": "run-123" }));
}

#[test]
fn build_exec_response_skips_continuation_after_exit() {
    let session = VTCodeExecSession {
        id: "run-123".to_string().into(),
        backend: "pipe".to_string(),
        command: "cargo".to_string(),
        args: vec!["check".to_string()],
        working_dir: Some(".".to_string()),
        background: false,
        rows: None,
        cols: None,
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    };
    let capture = PtyEphemeralCapture {
        output: "first\nsecond\n".to_string(),
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(25),
    };

    let response = build_exec_response(
        &session,
        "cargo check",
        &capture,
        ExecOutputPreview {
            raw_output: "first\nsecond\n".to_string(),
            output: "first\n[Output truncated]".to_string(),
            truncated: true,
        },
        None,
        false,
        None,
    );

    assert_eq!(response["exit_code"], 0);
    assert!(response.get("next_continue_args").is_none());
}

#[test]
fn build_exec_response_steers_still_running_to_wait_action() {
    let session = VTCodeExecSession {
        id: "run-abc".to_string().into(),
        backend: "pipe".to_string(),
        command: "cargo".to_string(),
        args: vec!["build".to_string()],
        working_dir: Some(".".to_string()),
        background: false,
        rows: None,
        cols: None,
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    };
    let capture = PtyEphemeralCapture {
        output: "   Compiling vtcode-core\n".to_string(),
        exit_code: None,
        duration: std::time::Duration::from_secs(10),
    };

    let response = build_exec_response(
        &session,
        "cargo build",
        &capture,
        ExecOutputPreview {
            raw_output: "   Compiling vtcode-core\n".to_string(),
            output: "   Compiling vtcode-core\n".to_string(),
            truncated: false,
        },
        None,
        false,
        Some("run-abc"),
    );

    // The poll-oriented continuation is still attached for incremental peeks.
    assert_eq!(response["next_continue_args"], json!({ "session_id": "run-abc" }));
    // The no-burn wait action is pre-filled and ready to reuse.
    assert_eq!(response["next_wait_args"]["session_id"], "run-abc");
    assert_eq!(response["next_wait_args"]["action"], "wait");
    assert_eq!(response["next_wait_args"]["wait_timeout_seconds"], 600);
    // The hint ranks wait ahead of polling and names the tool to call.
    let hint = response["next_action_hint"].as_str().expect("hint present");
    assert!(hint.contains("write_stdin"));
    assert!(hint.contains("next_wait_args"));
    assert!(hint.contains("no model round-trips"));
    assert!(hint.contains("excluded from the per-turn wall-clock budget"));
    assert!(hint.contains("next_continue_args"));
    assert_eq!(response["is_exited"], false);
    assert_eq!(response["process_id"], "run-abc");
}
