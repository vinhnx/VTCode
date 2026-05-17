use super::*;

#[tokio::test]
async fn build_tool_permissions_context_propagates_skip_confirmations() {
    let mut backing = TestContextBacking::new(2).await;
    let mut ctx = backing.turn_processing_context();

    let permissions = build_tool_permissions_context(&mut ctx);
    assert!(permissions.skip_confirmations);
    drop(permissions);

    ctx.skip_confirmations = false;

    let permissions = build_tool_permissions_context(&mut ctx);
    assert!(!permissions.skip_confirmations);
}

#[test]
fn low_signal_family_for_unified_search_normalizes_missing_default_path() {
    let first = low_signal_family_key(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","pattern":"-> Result","globs":["**/*.rs"]}),
    );
    let second = low_signal_family_key(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","path":".","pattern":"Result<","globs":["**/*.rs"]}),
    );

    assert_eq!(first, second);
}

#[test]
fn spool_chunk_read_path_detects_spooled_read_calls() {
    let args = json!({
        "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
        "offset": 41,
        "limit": 40
    });
    let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
    assert_eq!(
        path,
        Some(".vtcode/context/tool_outputs/unified_exec_123.txt")
    );
}

#[test]
fn spool_chunk_read_path_ignores_regular_reads() {
    let args = json!({
        "path": "src/main.rs",
        "offset": 1,
        "limit": 100
    });
    let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
    assert_eq!(path, None);
}

#[test]
fn task_tracker_create_signature_matches_identical_payloads() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_eq!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_payload_changes() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_title_change() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings later",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_notes_change() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "updated"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_ignores_non_create_calls() {
    let args = json!({
        "action": "update",
        "index": 1,
        "status": "completed"
    });
    let sig = task_tracker_create_signature(tool_names::TASK_TRACKER, &args);
    assert!(sig.is_none());
}

#[test]
fn shell_run_signature_normalizes_run_pty_command_and_args() {
    let args = json!({
        "command": "  cargo   check  ",
        "args": ["-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::RUN_PTY_CMD, &args);
    assert_eq!(
        signature,
        Some("unified_exec::cargo check -p vtcode-core".to_string())
    );
}

#[test]
fn shell_run_signature_handles_unified_exec_run_action() {
    let args = json!({
        "action": "run",
        "command": ["cargo", "check", "-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(
        signature,
        Some("unified_exec::cargo check -p vtcode-core".to_string())
    );
}

#[test]
fn shell_run_signature_normalizes_trivial_shell_quoting_differences() {
    let first = shell_run_signature(
        tool_names::UNIFIED_EXEC,
        &json!({
            "action": "run",
            "command": "grep -n '-> Result' vtcode-tui/src/**/*.rs"
        }),
    );
    let second = shell_run_signature(
        tool_names::UNIFIED_EXEC,
        &json!({
            "action": "run",
            "command": "grep -n \"-> Result\" vtcode-tui/src/**/*.rs"
        }),
    );

    assert_eq!(first, second);
}

#[test]
fn shell_run_signature_ignores_non_run_unified_exec_action() {
    let args = json!({
        "action": "poll",
        "session_id": "run-123"
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert!(signature.is_none());
}

#[test]
fn tool_budget_exhausted_reason_mentions_new_instruction_option() {
    let reason = build_tool_budget_exhausted_reason(32, 32);
    assert!(reason.contains("\"continue\" or provide a new instruction"));
}
