use super::*;
use std::borrow::Cow;
use tempfile::tempdir;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};

#[test]
fn fallback_from_error_extracts_unified_exec_poll() {
    let error = "Tool failed. Use unified_exec with action=\"poll\" and session_id=\"run-ab12\" instead of read_file.";
    let fallback = fallback_from_error(tool_names::UNIFIED_FILE, error, None);
    assert_eq!(
        fallback,
        Some((
            tool_names::UNIFIED_EXEC.to_string(),
            serde_json::json!({"action":"poll","session_id":"run-ab12"}),
        ))
    );
}

#[test]
fn fallback_from_error_recovers_unified_search_invalid_read_action() {
    let error = "Tool execution failed: Invalid action: read";
    let fallback = fallback_from_error(tool_names::UNIFIED_SEARCH, error, None);
    assert_eq!(
        fallback,
        Some((
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::json!({
                "action": "list",
                "path": "."
            }),
        ))
    );
}

#[test]
fn fallback_from_error_extracts_read_file_for_patch_context_mismatch() {
    let error = "Tool 'apply_patch' execution failed: failed to locate expected lines in 'vtcode-exec-events/src/trace.rs': context mismatch";
    let fallback = fallback_from_error(tool_names::APPLY_PATCH, error, None);
    assert_eq!(
        fallback,
        Some((
            tool_names::READ_FILE.to_string(),
            serde_json::json!({
                "path": "vtcode-exec-events/src/trace.rs",
                "offset": 1,
                "limit": 120
            }),
        ))
    );
}

#[test]
fn fallback_from_error_uses_task_tracker_list_for_update_argument_errors() {
    let error = "Tool 'task_tracker' execution failed: Tool execution failed: 'index' is required for 'update' (1-indexed)";
    let fallback = fallback_from_error(tool_names::TASK_TRACKER, error, None);
    assert_eq!(
        fallback,
        Some((
            tool_names::TASK_TRACKER.to_string(),
            serde_json::json!({"action": "list"}),
        ))
    );
}

#[test]
fn fallback_from_error_redirects_background_agent_spawn() {
    let error = "spawn_agent no longer launches managed background helpers. Use spawn_background_subprocess for agent 'background-demo' instead.";
    let fallback = fallback_from_error(
        tool_names::SPAWN_AGENT,
        error,
        Some(&serde_json::json!({
            "agent_type": "background-demo",
            "message": "Run the demo.",
            "background": false,
            "fork_context": true
        })),
    );
    assert_eq!(
        fallback,
        Some((
            tool_names::SPAWN_BACKGROUND_SUBPROCESS.to_string(),
            serde_json::json!({
                "agent_type": "background-demo",
                "message": "Run the demo."
            }),
        ))
    );
}

#[test]
fn build_error_content_includes_fallback_args() {
    let payload = build_error_content(
        "boom".to_string(),
        Some(tool_names::READ_PTY_SESSION.to_string()),
        Some(serde_json::json!({"session_id":"run-1"})),
        "execution",
    );

    assert_eq!(
        payload.get("fallback_tool").and_then(|v| v.as_str()),
        Some(tool_names::READ_PTY_SESSION)
    );
    assert_eq!(
        payload.get("fallback_tool_args"),
        Some(&serde_json::json!({"session_id":"run-1"}))
    );
    assert_eq!(
        payload.get("error_class").and_then(|v| v.as_str()),
        Some("execution_failure")
    );
    assert_eq!(
        payload.get("is_recoverable").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn build_error_content_truncates_large_errors() {
    let large_error = format!("Tool failed: {}", "x".repeat(700));
    let payload = build_error_content(large_error, None, None, "execution");

    assert_eq!(
        payload.get("error_truncated").and_then(|v| v.as_bool()),
        Some(true)
    );
    let rendered = payload
        .get("error")
        .and_then(|v| v.as_str())
        .expect("error field");
    assert!(rendered.contains("[truncated]"));
}

#[test]
fn build_error_content_marks_policy_denials_non_recoverable() {
    let payload = build_error_content(
        "tool permission denied by policy".to_string(),
        None,
        None,
        "execution",
    );

    assert_eq!(
        payload.get("error_class").and_then(|v| v.as_str()),
        Some("policy_blocked")
    );
    assert_eq!(
        payload.get("is_recoverable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(payload.get("next_action").is_none());
}

#[test]
fn build_error_content_compacts_large_fallback_args() {
    let payload = build_error_content(
        "boom".to_string(),
        Some(tool_names::READ_FILE.to_string()),
        Some(serde_json::json!({"content": "x".repeat(600)})),
        "execution",
    );

    assert!(payload.get("fallback_tool_args").is_none());
    assert_eq!(
        payload
            .get("fallback_tool_args_truncated")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert!(
        payload
            .get("fallback_tool_args_preview")
            .and_then(|v| v.as_str())
            .is_some()
    );
    assert_eq!(
        payload.get("next_action").and_then(|v| v.as_str()),
        Some("Try an alternative tool or narrower scope.")
    );
}

#[test]
fn build_error_content_keeps_structured_fallback_fields_only() {
    let payload = build_error_content(
        "boom".to_string(),
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(serde_json::json!({"action":"list","path":"."})),
        "execution",
    );

    assert_eq!(
        payload.get("fallback_tool"),
        Some(&serde_json::json!(tool_names::UNIFIED_SEARCH))
    );
    assert_eq!(
        payload.get("fallback_tool_args"),
        Some(&serde_json::json!({"action":"list","path":"."}))
    );
    assert_eq!(
        payload.get("is_recoverable").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        payload.get("next_action").and_then(|v| v.as_str()),
        Some("Try an alternative tool or narrower scope.")
    );
}

#[test]
fn build_structured_error_content_preserves_retry_and_partial_state_fields() {
    let mut error = ToolExecutionError::new(
        tool_names::WRITE_FILE.to_string(),
        ToolErrorType::ExecutionError,
        "write failed".to_string(),
    )
    .with_partial_state(true, false)
    .with_surface("unified_runloop")
    .with_attempt(2);
    error.is_recoverable = true;
    error.retryable = true;
    error.retry_delay_ms = Some(750);
    error.recovery_suggestions = vec![Cow::Borrowed("Retry with smaller scope.")];

    let payload = build_structured_error_content(&error, None, None, "execution");

    assert_eq!(
        payload
            .get("partial_state_possible")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        payload
            .get("retry_delay_ms")
            .and_then(|value| value.as_u64()),
        Some(750)
    );
    assert_eq!(
        payload.get("next_action").and_then(|value| value.as_str()),
        Some("Retry with smaller scope.")
    );
    assert_eq!(
        payload
            .get("error")
            .and_then(|value| value.get("partial_state_possible"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn structured_retry_summary_ignores_initial_attempt() {
    let error = ToolExecutionError::new(
        tool_names::READ_FILE.to_string(),
        ToolErrorType::ExecutionError,
        "read failed".to_string(),
    )
    .with_attempt(1);

    assert_eq!(error.retry_summary(), None);
}

#[test]
fn build_structured_error_content_round_trips_tool_error() {
    let mut error = ToolExecutionError::new(
        tool_names::WRITE_FILE.to_string(),
        ToolErrorType::ExecutionError,
        "write failed".to_string(),
    )
    .with_partial_state(true, false)
    .with_surface("unified_runloop")
    .with_attempt(2);
    error.retry_delay_ms = Some(750);

    let payload = build_structured_error_content(&error, None, None, "execution");
    let parsed = ToolExecutionError::from_tool_output(&payload).expect("structured error");

    assert_eq!(parsed.tool_name, tool_names::WRITE_FILE);
    assert!(parsed.partial_state_possible);
    assert_eq!(parsed.retry_delay_ms, Some(750));
}

#[test]
fn maybe_inline_spooled_removes_redundant_fields() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "tail",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "spool_hint": "verbose hint",
            "spooled_bytes": 12345,
            "success": true,
            "status": "success",
            "message": "ok",
            "metadata": {"size_bytes": 100},
            "no_spool": false,
            "id": "run-1",
            "session_id": "run-1",
            "process_id": "run-1",
            "command": "cargo check -p vtcode",
            "is_exited": false,
            "working_directory": null,
            "rows": 24,
            "cols": 80,
            "wall_time": 1.23,
            "stderr": "warn",
            "stderr_preview": "warn",
            "follow_up_prompt": "More output available.",
            "has_more": false,
            "truncated": false,
            "auto_recovered": false,
            "query_truncated": false,
            "stdout": "tail",
            "next_continue_args": {
                "action": "continue",
                "session_id": "run-1"
            }
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("spool_hint").is_none());
    assert!(parsed.get("spooled_bytes").is_none());
    assert!(parsed.get("spooled_to_file").is_none());
    assert!(parsed.get("success").is_none());
    assert!(parsed.get("status").is_none());
    assert!(parsed.get("message").is_none());
    assert!(parsed.get("metadata").is_none());
    assert!(parsed.get("no_spool").is_none());
    assert!(parsed.get("id").is_none());
    assert!(parsed.get("process_id").is_none());
    assert!(parsed.get("command").is_none());
    assert!(parsed.get("is_exited").is_none());
    assert!(parsed.get("working_directory").is_none());
    assert!(parsed.get("rows").is_none());
    assert!(parsed.get("cols").is_none());
    assert!(parsed.get("wall_time").is_none());
    assert!(parsed.get("follow_up_prompt").is_none());
    assert!(parsed.get("has_more").is_none());
    assert!(parsed.get("truncated").is_none());
    assert!(parsed.get("auto_recovered").is_none());
    assert!(parsed.get("query_truncated").is_none());
    assert_eq!(
        parsed.get("stderr_preview"),
        Some(&serde_json::json!("warn"))
    );
    assert!(parsed.get("stdout").is_none());
    assert!(parsed.get("next_poll_args").is_none());
    assert!(parsed.get("preferred_next_action").is_none());
    assert!(parsed.get("session_id").is_none());
    assert_eq!(
        parsed.get("next_continue_args"),
        Some(&serde_json::json!({"s":"run-1"}))
    );
}

#[test]
fn maybe_inline_spooled_compacts_next_read_duplicates() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
            "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
            "has_more": true,
            "next_offset": 81,
            "chunk_limit": 40,
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                "offset": 81,
                "limit": 40
            }
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("spooled_to_file").is_none());
    assert!(parsed.get("has_more").is_none());
    assert!(parsed.get("next_offset").is_none());
    assert!(parsed.get("chunk_limit").is_none());
    assert!(parsed.get("spool_path").is_none());
    assert_eq!(
        parsed.get("path"),
        Some(&serde_json::json!(
            ".vtcode/context/tool_outputs/unified_exec_1.txt"
        ))
    );
    assert_eq!(
        parsed.get("next_read_args"),
        Some(&serde_json::json!({
            "p": ".vtcode/context/tool_outputs/unified_exec_1.txt",
            "o": 81,
            "l": 40
        }))
    );
}

#[test]
fn maybe_inline_spooled_preserves_extra_continue_args_fields() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "next_continue_args": {
                "action": "continue",
                "session_id": "run-1",
                "cursor": 42
            }
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(
        parsed.get("next_continue_args"),
        Some(&serde_json::json!({
            "s": "run-1",
            "cursor": 42
        }))
    );
}

#[test]
fn maybe_inline_spooled_keeps_loop_recovery_fields_and_drops_notes() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "loop_detected": true,
            "spool_path": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
                "offset": 81,
                "limit": 40
            },
            "reused_spooled_output": true,
            "spool_ref_only": true,
            "loop_detected_note": "Read the spool file instead of re-running this call.",
            "repeat_count": 4,
            "limit": 3,
            "tool": "read_file"
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("loop_detected"), Some(&serde_json::json!(true)));
    assert_eq!(
        parsed.get("spool_path"),
        Some(&serde_json::json!(
            ".vtcode/context/tool_outputs/unified_exec_loop.txt"
        ))
    );
    assert_eq!(
        parsed.get("next_read_args"),
        Some(&serde_json::json!({
            "p": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
            "o": 81,
            "l": 40
        }))
    );
    assert!(parsed.get("reused_spooled_output").is_none());
    assert!(parsed.get("spool_ref_only").is_none());
    assert!(parsed.get("loop_detected_note").is_none());
    assert!(parsed.get("repeat_count").is_none());
    assert!(parsed.get("limit").is_none());
    assert!(parsed.get("tool").is_none());
}

#[test]
fn maybe_inline_spooled_uses_reference_only_for_spooled_exec_output() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "preview text",
            "stdout": "preview text",
            "stderr": "warning text",
            "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
            "exit_code": 0,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("output").is_none());
    assert!(parsed.get("stdout").is_none());
    assert!(parsed.get("stderr").is_none());
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
    assert_eq!(
        parsed.get("spool_path"),
        Some(&serde_json::json!(
            ".vtcode/context/tool_outputs/unified_exec_1.txt"
        ))
    );
    assert_eq!(
        parsed.get("stderr_preview"),
        Some(&serde_json::json!("warning text"))
    );
    assert_eq!(
        parsed.get("result_ref_only"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn maybe_inline_spooled_drops_terminal_exec_metadata_without_continuation() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "ok",
            "command": "cargo check -p vtcode-core",
            "session_id": "run-1",
            "process_id": "run-1",
            "working_directory": "/workspace",
            "is_exited": true,
            "exit_code": 0,
            "rows": 24,
            "cols": 80,
            "wall_time": 0.5
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("output"), Some(&serde_json::json!("ok")));
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
    assert!(parsed.get("command").is_none());
    assert!(parsed.get("session_id").is_none());
    assert!(parsed.get("process_id").is_none());
    assert!(parsed.get("working_directory").is_none());
    assert!(parsed.get("is_exited").is_none());
    assert!(parsed.get("rows").is_none());
    assert!(parsed.get("cols").is_none());
    assert!(parsed.get("wall_time").is_none());
}

#[test]
fn maybe_inline_spooled_keeps_exec_recovery_guidance() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "bash: pip: command not found",
            "command": "pip install pymupdf",
            "session_id": "run-127",
            "process_id": "run-127",
            "is_exited": true,
            "exit_code": 127,
            "critical_note": "Command `pip` was not found in PATH.",
            "next_action": "Check the command name or install the missing binary, then rerun the command."
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(
        parsed.get("output"),
        Some(&serde_json::json!("bash: pip: command not found"))
    );
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(127)));
    assert_eq!(
        parsed.get("critical_note"),
        Some(&serde_json::json!("Command `pip` was not found in PATH."))
    );
    assert_eq!(
        parsed.get("next_action"),
        Some(&serde_json::json!(
            "Check the command name or install the missing binary, then rerun the command."
        ))
    );
    assert!(parsed.get("command").is_none());
    assert!(parsed.get("session_id").is_none());
    assert!(parsed.get("process_id").is_none());
    assert!(parsed.get("is_exited").is_none());
}

#[test]
fn maybe_inline_spooled_keeps_recoverable_failure_next_action() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "error": "Tool preflight validation failed: x",
            "is_recoverable": true,
            "next_action": "Retry with fallback_tool_args.",
            "fallback_tool": tool_names::UNIFIED_SEARCH,
            "fallback_tool_args": {"action":"list","path":"."}
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(
        parsed.get("next_action"),
        Some(&serde_json::json!("Retry with fallback_tool_args."))
    );
    assert_eq!(
        parsed.get("fallback_tool"),
        Some(&serde_json::json!(tool_names::UNIFIED_SEARCH))
    );
}

#[test]
fn maybe_inline_spooled_keeps_structural_recovery_success_next_action() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_SEARCH,
        &serde_json::json!({
            "backend": "ast-grep",
            "matches": [],
            "is_recoverable": true,
            "hint": "Pattern looks like a code fragment.",
            "next_action": "Retry with a larger parseable pattern."
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(
        parsed.get("next_action"),
        Some(&serde_json::json!("Retry with a larger parseable pattern."))
    );
    assert_eq!(
        parsed.get("hint"),
        Some(&serde_json::json!("Pattern looks like a code fragment."))
    );
}

#[test]
fn maybe_inline_spooled_drops_non_recoverable_failure_next_action() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "error": "tool permission denied by policy",
            "is_recoverable": false,
            "next_action": "Switch to an allowed tool or mode."
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("next_action").is_none());
}

#[test]
fn maybe_inline_spooled_drops_generic_success_recovery_guidance() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "output": "ok",
            "critical_note": "This should not survive for non-exec payloads.",
            "next_action": "This should stay compacted away."
        }),
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("output"), Some(&serde_json::json!("ok")));
    assert!(parsed.get("critical_note").is_none());
    assert!(parsed.get("next_action").is_none());
}

#[tokio::test]
async fn tool_output_summary_input_uses_spool_file_tail_for_exec_output() {
    let temp = tempdir().unwrap();
    let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let spool_path = spool_dir.join("unified_exec_1.txt");
    let spool_content = (1..=150)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&spool_path, spool_content).unwrap();

    let output = serde_json::json!({
        "output": "preview text",
        "stderr_preview": "warning text",
        "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(
        temp.path(),
        tool_names::UNIFIED_EXEC,
        &output,
        &serialized,
    )
    .await;

    assert!(input.contains("Tool payload:"));
    assert!(input.contains("stderr_preview:\nwarning text"));
    assert!(input.contains("tail_excerpt:"));
    assert!(input.contains("line-150"));
    assert!(!input.contains("line-1\nline-2\nline-3"));
}

#[tokio::test]
async fn tool_output_summary_input_uses_spool_file_excerpt_for_large_reads() {
    let temp = tempdir().unwrap();
    let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let spool_path = spool_dir.join("read_1.txt");
    let spool_content = (1..=200)
        .map(|idx| format!("read-line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&spool_path, spool_content).unwrap();

    let output = serde_json::json!({
        "path": "src/main.rs",
        "spool_path": ".vtcode/context/tool_outputs/read_1.txt"
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(
        temp.path(),
        tool_names::READ_FILE,
        &output,
        &serialized,
    )
    .await;

    assert!(input.contains("source_path: src/main.rs"));
    assert!(input.contains("content_excerpt:"));
    assert!(input.contains("read-line-1"));
    assert!(input.contains("read-line-200"));
}

#[tokio::test]
async fn tool_output_summary_input_falls_back_to_serialized_output_when_spool_missing() {
    let temp = tempdir().unwrap();
    let output = serde_json::json!({
        "spool_path": ".vtcode/context/tool_outputs/missing.txt",
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(
        temp.path(),
        tool_names::UNIFIED_EXEC,
        &output,
        &serialized,
    )
    .await;

    assert_eq!(input, serialized);
}

#[tokio::test]
async fn tool_output_summary_input_decodes_invalid_utf8_spool_lossily() {
    let temp = tempdir().unwrap();
    let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let spool_path = spool_dir.join("unified_exec_invalid.txt");
    std::fs::write(&spool_path, b"ok\n\xff\xfe\nlast line\n").unwrap();

    let output = serde_json::json!({
        "output": "preview text",
        "stderr_preview": "warning text",
        "spool_path": ".vtcode/context/tool_outputs/unified_exec_invalid.txt",
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(
        temp.path(),
        tool_names::UNIFIED_EXEC,
        &output,
        &serialized,
    )
    .await;

    assert!(input.contains("warning text"));
    assert!(input.contains("last line"));
    assert_ne!(input, serialized);
}

#[test]
fn blocked_or_denied_failure_detects_guardable_errors() {
    assert!(is_blocked_or_denied_failure(
        "plan_task_tracker is a Plan Mode compatibility alias. Use task_tracker in Edit mode, or switch to Plan Mode."
    ));
    assert!(is_blocked_or_denied_failure("Tool permission denied"));
    assert!(is_blocked_or_denied_failure(
        "Policy violation: exceeded max tool calls per turn (32)"
    ));
    assert!(is_blocked_or_denied_failure(
        "Safety validation failed: command pattern denied"
    ));
}

#[test]
fn blocked_or_denied_failure_ignores_runtime_execution_failures() {
    assert!(!is_blocked_or_denied_failure(
        "command exited with status 1"
    ));
    assert!(!is_blocked_or_denied_failure(
        "stream request timed out after 30000ms"
    ));
}

#[test]
fn parse_subagent_summary_markdown_reads_fixed_contract() {
    let parsed = parse_subagent_summary_markdown(
        "## Summary\n- Investigated compaction flow\n\n## Facts\n- `context.dynamic.retained_user_messages` defaults to 4\n- `read_file` duplicates are deduped locally\n\n## Touched Files\n- src/agent/runloop/unified/turn/compaction.rs\n\n## Verification\n- Run cargo check\n\n## Open Questions\n- Should batch reads be deduped too?\n",
    )
    .expect("structured summary should parse");

    assert_eq!(parsed.summary, vec!["Investigated compaction flow"]);
    assert_eq!(
        parsed.facts,
        vec![
            "`context.dynamic.retained_user_messages` defaults to 4",
            "`read_file` duplicates are deduped locally",
        ]
    );
    assert_eq!(
        parsed.touched_files,
        vec!["src/agent/runloop/unified/turn/compaction.rs"]
    );
    assert_eq!(parsed.verification, vec!["Run cargo check"]);
    assert_eq!(
        parsed.open_questions,
        vec!["Should batch reads be deduped too?"]
    );
}

#[test]
fn parse_subagent_summary_markdown_treats_none_sections_as_empty() {
    let parsed = parse_subagent_summary_markdown(
        "## Summary\n- None\n\n## Facts\n- None\n\n## Touched Files\n- None\n\n## Verification\n- None\n\n## Open Questions\n- None\n",
    )
    .expect("structured summary should parse");

    assert!(parsed.summary.is_empty());
    assert!(parsed.facts.is_empty());
    assert!(parsed.touched_files.is_empty());
    assert!(parsed.verification.is_empty());
    assert!(parsed.open_questions.is_empty());
}

#[test]
fn parse_subagent_summary_markdown_rejects_unstructured_text() {
    assert!(parse_subagent_summary_markdown("plain paragraph without headings").is_none());
}

#[test]
fn build_subagent_memory_update_aggregates_structured_child_results() {
    let output = serde_json::json!({
        "completed": true,
        "entry": {
            "agent_name": "reviewer",
            "summary": "## Summary\n- Investigated compaction flow\n- Confirmed contract\n\n## Facts\n- Local compaction dedupes repeated reads\n\n## Touched Files\n- src/agent/runloop/unified/turn/compaction.rs\n\n## Verification\n- Run cargo check\n\n## Open Questions\n- None\n"
        }
    });

    let update = build_subagent_memory_update(&output).expect("update");

    assert_eq!(
        update.grounded_facts,
        vec![GroundedFactRecord {
            fact: "Local compaction dedupes repeated reads".to_string(),
            source: "subagent:reviewer".to_string(),
        }]
    );
    assert_eq!(
        update.touched_files,
        vec!["src/agent/runloop/unified/turn/compaction.rs".to_string()]
    );
    assert_eq!(
        update.verification_todo,
        vec!["Run cargo check".to_string()]
    );
    assert_eq!(
        update.delegation_notes,
        vec!["reviewer: Investigated compaction flow | Confirmed contract".to_string()]
    );
}

#[test]
fn build_subagent_memory_update_falls_back_to_raw_summary() {
    let output = serde_json::json!({
        "status": "completed",
        "agent_name": "worker",
        "summary": "plain child summary"
    });

    let update = build_subagent_memory_update(&output).expect("update");

    assert!(update.grounded_facts.is_empty());
    assert!(update.touched_files.is_empty());
    assert_eq!(
        update.delegation_notes,
        vec!["worker: plain child summary".to_string()]
    );
}

#[test]
fn build_subagent_memory_update_ignores_empty_structured_summary() {
    let output = serde_json::json!({
        "status": "completed",
        "agent_name": "worker",
        "summary": "## Summary\n- None\n\n## Facts\n- None\n\n## Touched Files\n- None\n\n## Verification\n- None\n\n## Open Questions\n- None\n"
    });

    assert!(build_subagent_memory_update(&output).is_none());
}

#[test]
fn stderr_preview_truncates_unicode_safely() {
    let stderr = "an’t ".repeat(200);
    let preview = truncate_stderr_preview(&stderr);
    assert!(preview.ends_with("... (truncated)"));
}
