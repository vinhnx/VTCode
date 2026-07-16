#![allow(missing_docs)]
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
fn low_signal_family_for_code_search_uses_limit_insensitive_loop_identity() {
    let first = low_signal_family_key(
        tool_names::CODE_SEARCH,
        &json!({
            "query": " Widget ",
            "file_types": [".rs", "python", "rust"],
            "result_types": ["path", "definition", "path"],
            "max_results": 5
        }),
    );
    let second = low_signal_family_key(
        tool_names::CODE_SEARCH,
        &json!({
            "query": "Widget",
            "path": ".",
            "file_types": ["rust", ".py"],
            "result_types": ["definition", "path"],
            "max_results": 100
        }),
    );

    assert_eq!(first, second);
}

#[test]
fn low_signal_family_for_code_search_distinguishes_query_and_filters() {
    let base = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust"],
        "result_types": ["definition"]
    });
    let base_key = low_signal_family_key(tool_names::CODE_SEARCH, &base);

    for changed in [
        json!({"query": "Other", "path": "src", "file_types": ["rust"], "result_types": ["definition"]}),
        json!({"query": "Widget", "path": "tests", "file_types": ["rust"], "result_types": ["definition"]}),
        json!({"query": "Widget", "path": "src", "file_types": ["python"], "result_types": ["definition"]}),
        json!({"query": "Widget", "path": "src", "file_types": ["rust"], "result_types": ["usage"]}),
    ] {
        assert_ne!(
            base_key,
            low_signal_family_key(tool_names::CODE_SEARCH, &changed)
        );
    }
}

#[test]
fn spool_chunk_read_path_detects_spooled_read_calls() {
    let args = json!({
        "path": ".vtcode/context/tool_outputs/command_session_123.txt",
        "offset": 41,
        "limit": 40
    });
    let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
    assert_eq!(
        path,
        Some(".vtcode/context/tool_outputs/command_session_123.txt")
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
        Some("command_session::cargo check -p vtcode-core".to_string())
    );
}

#[test]
fn shell_run_signature_handles_command_session_run_action() {
    let args = json!({
        "action": "run",
        "command": ["cargo", "check", "-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(
        signature,
        Some("command_session::cargo check -p vtcode-core".to_string())
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
fn shell_run_signature_ignores_non_run_command_session_action() {
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
