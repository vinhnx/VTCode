use std::borrow::Cow;

use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};

use super::*;

#[test]
fn fallback_from_error_extracts_command_session_poll() {
    let error =
        "Tool failed. Use command_session with action=\"poll\" and session_id=\"run-ab12\" instead of read_file.";
    let fallback = fallback_from_error(tool_names::UNIFIED_FILE, error, None);
    assert_eq!(
        fallback,
        Some((tool_names::UNIFIED_EXEC.to_string(), serde_json::json!({"action":"poll","session_id":"run-ab12"}),))
    );
}

#[test]
fn fallback_from_error_extracts_read_file_for_patch_context_mismatch() {
    let error = "Tool 'apply_patch' execution failed: failed to locate expected lines in 'crates/common/vtcode-exec-events/src/trace.rs': context mismatch";
    let fallback = fallback_from_error(tool_names::APPLY_PATCH, error, None);
    assert_eq!(
        fallback,
        Some((
            tool_names::READ_FILE.to_string(),
            serde_json::json!({
                "path": "crates/common/vtcode-exec-events/src/trace.rs",
                "offset": 1,
                "limit": 120
            }),
        ))
    );
}

#[test]
fn fallback_from_error_uses_task_tracker_list_for_update_argument_errors() {
    let error =
        "Tool 'task_tracker' execution failed: Tool execution failed: 'index' is required for 'update' (1-indexed)";
    let fallback = fallback_from_error(tool_names::TASK_TRACKER, error, None);
    assert_eq!(fallback, Some((tool_names::TASK_TRACKER.to_string(), serde_json::json!({"action": "list"}),)));
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
fn build_error_content_preview_exhaustion_gate_is_terminal_for_inspections() {
    // Regression: the generic "narrower scope" default invited consecutive
    // retries (three gate rejections on distinct reads in one execution turn),
    // each feeding the blocked-call fuse. The gate is budget-based, so a
    // narrower retry can only return another stub.
    let payload = build_error_content(
        "Tool preview budget is exhausted this turn; further inspection returns hidden stubs.".to_string(),
        None,
        None,
        "preview_exhaustion_gate",
    );

    assert_eq!(payload.get("failure_kind").and_then(|v| v.as_str()), Some("preview_exhaustion_gate"));
    assert_eq!(payload.get("error_class").and_then(|v| v.as_str()), Some("execution_failure"));
    assert_eq!(payload.get("is_recoverable").and_then(|v| v.as_bool()), Some(true));
    let next_action = payload.get("next_action").and_then(|v| v.as_str()).expect("next_action");
    assert!(next_action.contains("returns another stub"), "unexpected next_action: {next_action}");
    assert!(!next_action.contains("narrower scope"), "retry-inviting wording survived: {next_action}");

    // The generic default is unchanged for ordinary execution failures.
    let generic = build_error_content("boom".to_string(), None, None, "execution");
    assert_eq!(
        generic.get("next_action").and_then(|v| v.as_str()),
        Some("Try an alternative tool or narrower scope.")
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

    assert_eq!(payload.get("fallback_tool").and_then(|v| v.as_str()), Some(tool_names::READ_PTY_SESSION));
    assert_eq!(payload.get("fallback_tool_args"), Some(&serde_json::json!({"session_id":"run-1"})));
    assert_eq!(payload.get("error_class").and_then(|v| v.as_str()), Some("execution_failure"));
    assert_eq!(payload.get("is_recoverable").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn build_error_content_truncates_large_errors() {
    let large_error = format!("Tool failed: {}", "x".repeat(700));
    let payload = build_error_content(large_error, None, None, "execution");

    assert_eq!(payload.get("error_truncated").and_then(|v| v.as_bool()), Some(true));
    let rendered = payload.get("error").and_then(|v| v.as_str()).expect("error field");
    assert!(rendered.contains("[truncated]"));
}

#[test]
fn build_error_content_marks_policy_denials_non_recoverable() {
    let payload = build_error_content("tool permission denied by policy".to_string(), None, None, "execution");

    assert_eq!(payload.get("error_class").and_then(|v| v.as_str()), Some("policy_blocked"));
    assert_eq!(payload.get("is_recoverable").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(payload.get("next_action").and_then(|v| v.as_str()), Some("Switch to an allowed tool or mode."));
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
    assert_eq!(payload.get("fallback_tool_args_truncated").and_then(|v| v.as_bool()), Some(true));
    assert!(payload.get("fallback_tool_args_preview").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        payload.get("next_action").and_then(|v| v.as_str()),
        Some("Try an alternative tool or narrower scope.")
    );
}

#[test]
fn build_error_content_keeps_structured_fallback_fields_only() {
    let payload = build_error_content(
        "boom".to_string(),
        Some(tool_names::CODE_SEARCH.to_string()),
        Some(serde_json::json!({"query":"Widget","path":"."})),
        "execution",
    );

    assert_eq!(payload.get("fallback_tool"), Some(&serde_json::json!(tool_names::CODE_SEARCH)));
    assert_eq!(payload.get("fallback_tool_args"), Some(&serde_json::json!({"query":"Widget","path":"."})));
    assert_eq!(payload.get("is_recoverable").and_then(|v| v.as_bool()), Some(true));
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

    assert_eq!(payload.get("partial_state_possible").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(payload.get("retry_delay_ms").and_then(|value| value.as_u64()), Some(750));
    assert_eq!(payload.get("next_action").and_then(|value| value.as_str()), Some("Retry with smaller scope."));
    assert_eq!(
        payload
            .get("error")
            .and_then(|value| value.get("partial_state_possible"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn build_structured_error_content_preview_exhaustion_gate_is_terminal_for_inspections() {
    // Mirrors the unstructured-arm regression: a structured error carrying
    // the preview-exhaustion kind must not invite narrower-scope retries,
    // which can only return more stubs and feed the blocked-call fuse.
    let mut error = ToolExecutionError::new(
        tool_names::READ_FILE.to_string(),
        ToolErrorType::ExecutionError,
        "Tool preview budget is exhausted this turn.".to_string(),
    );
    error.is_recoverable = true;
    // No caller suggestions: the arm's default must win (suggestions-first
    // precedence is covered by
    // `build_structured_error_content_preserves_retry_and_partial_state_fields`).
    error.recovery_suggestions = Vec::new();

    let payload = build_structured_error_content(&error, None, None, "preview_exhaustion_gate");

    assert_eq!(payload.get("failure_kind").and_then(|value| value.as_str()), Some("preview_exhaustion_gate"));
    let next_action = payload
        .get("next_action")
        .and_then(|value| value.as_str())
        .expect("next_action");
    assert!(next_action.contains("returns another stub"), "unexpected next_action: {next_action}");
    assert!(!next_action.contains("narrower scope"), "retry-inviting wording survived: {next_action}");
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("spool_hint").is_none());
    assert_eq!(parsed["spooled_bytes"], 12345);
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
    assert_eq!(parsed.get("stderr_preview"), Some(&serde_json::json!("warn")));
    assert!(parsed.get("stdout").is_none());
    assert!(parsed.get("next_poll_args").is_none());
    assert!(parsed.get("preferred_next_action").is_none());
    assert!(parsed.get("session_id").is_none());
    assert_eq!(parsed.get("next_continue_args"), Some(&serde_json::json!({"s":"run-1"})));
}

#[test]
fn maybe_inline_spooled_compacts_next_read_duplicates() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "path": ".vtcode/context/tool_outputs/command_session_1.txt",
            "spool_path": ".vtcode/context/tool_outputs/command_session_1.txt",
            "has_more": true,
            "next_offset": 81,
            "chunk_limit": 40,
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/command_session_1.txt",
                "offset": 81,
                "limit": 40
            }
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("spooled_to_file").is_none());
    assert!(parsed.get("has_more").is_none());
    assert!(parsed.get("next_offset").is_none());
    assert!(parsed.get("chunk_limit").is_none());
    assert!(parsed.get("spool_path").is_none());
    assert_eq!(parsed.get("path"), Some(&serde_json::json!(".vtcode/context/tool_outputs/command_session_1.txt")));
    assert_eq!(
        parsed.get("next_read_args"),
        Some(&serde_json::json!({
            "p": ".vtcode/context/tool_outputs/command_session_1.txt",
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(
        parsed.get("next_continue_args"),
        Some(&serde_json::json!({
            "s": "run-1",
            "cursor": 42
        }))
    );
}

#[test]
fn maybe_inline_spooled_keeps_loop_recovery_fields() {
    let serialized = maybe_inline_spooled(
        tool_names::READ_FILE,
        &serde_json::json!({
            "loop_detected": true,
            "spool_path": ".vtcode/context/tool_outputs/command_session_loop.txt",
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/command_session_loop.txt",
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    // `loop_detected` is internal control logic and is stripped from model output.
    assert!(parsed.get("loop_detected").is_none());
    assert_eq!(
        parsed.get("spool_path"),
        Some(&serde_json::json!(".vtcode/context/tool_outputs/command_session_loop.txt"))
    );
    assert_eq!(
        parsed.get("next_read_args"),
        Some(&serde_json::json!({
            "p": ".vtcode/context/tool_outputs/command_session_loop.txt",
            "o": 81,
            "l": 40
        }))
    );
    // Loop-recovery metadata fields are kept so the model understands why
    // its tool call was intercepted and can act on the guidance.
    assert_eq!(parsed.get("reused_spooled_output"), Some(&serde_json::json!(true)));
    assert_eq!(parsed.get("spool_ref_only"), Some(&serde_json::json!(true)));
    assert_eq!(
        parsed.get("loop_detected_note"),
        Some(&serde_json::json!("Read the spool file instead of re-running this call."))
    );
    assert_eq!(parsed.get("repeat_count"), Some(&serde_json::json!(4)));
    assert!(parsed.get("limit").is_none());
    assert_eq!(parsed.get("tool"), Some(&serde_json::json!("read_file")));
}

#[test]
fn maybe_inline_spooled_uses_reference_only_for_spooled_exec_output() {
    let serialized = maybe_inline_spooled(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "preview text",
            "stdout": "preview text",
            "stderr": "warning text",
            "spool_path": ".vtcode/context/tool_outputs/command_session_1.txt",
            "exit_code": 0,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("output").is_none());
    assert!(parsed.get("stdout").is_none());
    assert!(parsed.get("stderr").is_none());
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
    assert_eq!(
        parsed.get("spool_path"),
        Some(&serde_json::json!(".vtcode/context/tool_outputs/command_session_1.txt"))
    );
    assert_eq!(parsed.get("stderr_preview"), Some(&serde_json::json!("warning text")));
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
}

#[test]
fn maybe_inline_spooled_uses_reference_only_for_spooled_code_search_output() {
    let large_result_payload = "result\n".repeat(10_000);
    let serialized = maybe_inline_spooled(
        tool_names::CODE_SEARCH,
        &serde_json::json!({
            "query": "Widget",
            "results": [{"result_type":"text","path":"src/lib.rs","line":1,"column":1,"snippet":large_result_payload}],
            "spool_path": ".vtcode/context/tool_outputs/code_search_1.txt",
            "truncated": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("results").is_none());
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!(".vtcode/context/tool_outputs/code_search_1.txt")));
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
    assert!(
        serialized.len() < 1_000,
        "spooled search payload should stay compact, got {} bytes",
        serialized.len()
    );
}

#[test]
fn turn_911_style_exec_inspection_preview_keeps_head_tail_within_budget() {
    let spool_relative = ".vtcode/context/tool_outputs/command_session_test.txt";
    let mut content = String::new();
    for idx in 0..40 {
        content.push_str(&format!("./crate/file_{idx}.rs\n"));
    }
    let serialized = maybe_inline_spooled_with_preview(
        tool_names::EXEC_COMMAND,
        &serde_json::json!({
            "command": "rg --files crate",
            "output": "",
            "spool_path": ".vtcode/context/tool_outputs/command_session_test.txt",
            "preview": content,
            "exit_code": 0,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("output").is_none(), "output should be stripped");
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
    let preview = parsed
        .get("preview")
        .and_then(serde_json::Value::as_str)
        .expect("preview should be present so the model sees actual content");
    assert!(preview.contains("file_0"), "inspection preview should retain head context: {preview}");
    assert!(preview.contains("./crate/file_"), "preview should include actual spool content, got: {preview}");
    assert!(preview.contains("file_39"), "inspection preview should retain tail context: {preview}");
    assert!(preview.lines().count() <= 80);
    assert!(serialized.len() < 7_000, "turn-911 regression payload exceeded preview budget");
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!(spool_relative)));
}

#[test]
fn maybe_inline_spooled_with_preview_uses_tail_focused_preview_for_verification_logs() {
    let spool_relative = ".vtcode/context/tool_outputs/verification_log.txt";
    let mut content = String::new();
    for idx in 101..=120 {
        content.push_str(&format!("build-line-{idx}\n"));
    }
    let serialized = maybe_inline_spooled_with_preview(
        tool_names::EXEC_COMMAND,
        &serde_json::json!({
            "output": "",
            "command": "cargo nextest run -p vtcode-core",
            "spool_path": ".vtcode/context/tool_outputs/verification_log.txt",
            "preview": content,
            "exit_code": 101,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    let preview = parsed
        .get("preview")
        .and_then(serde_json::Value::as_str)
        .expect("verification preview should be present");
    assert!(
        preview.lines().all(|line| line != "build-line-1"),
        "verification logs should be tail-focused instead of preserving the head: {preview}"
    );
    assert!(
        preview.contains("build-line-120"),
        "tail-focused preview should retain the end of the log: {preview}"
    );
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!(spool_relative)));
}

#[test]
fn maybe_inline_spooled_with_preview_skips_preview_when_spool_missing() {
    let serialized = maybe_inline_spooled_with_preview(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "",
            "command": "cargo check -p vtcode-core",
            "spool_path": ".vtcode/context/tool_outputs/missing.txt",
            "exit_code": 0,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("preview").is_none());
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!(".vtcode/context/tool_outputs/missing.txt")));
}

#[test]
fn maybe_inline_spooled_with_preview_keeps_spool_reference_when_path_is_invalid() {
    let serialized = maybe_inline_spooled_with_preview(
        tool_names::EXEC_COMMAND,
        &serde_json::json!({
            "output": "",
            "command": "cat ../outside.txt",
            "spool_path": "../outside.txt",
            "exit_code": 0,
            "is_exited": true
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert!(parsed.get("preview").is_none(), "invalid spool paths must not be previewed");
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!("../outside.txt")));
}

#[test]
fn maybe_inline_spooled_with_preview_keeps_preview_and_failure_diagnostics() {
    let spool_relative = ".vtcode/context/tool_outputs/test_failure.txt";
    let serialized = maybe_inline_spooled_with_preview(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({
            "output": "",
            "command": "cargo nextest run -p my-crate",
            "spool_path": spool_relative,
            "spooled_bytes": 38_912,
            "preview": "PASS test_pass_1\nFAIL my_test",
            "exit_code": 100,
            "is_exited": true,
            "auto_recovered": true,
            "failure_diagnostics": {
                "kind": "cargo_test_failure",
                "package": "my-crate",
                "test_fqname": "my_test",
                "panic": "assertion failed",
                "rerun_hint": "cargo nextest run -p my-crate my_test",
                "critical_note": "Cargo reported a concrete failing test.",
                "next_action": "Rerun with: cargo nextest run -p my-crate my_test"
            }
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("preview"), Some(&serde_json::json!("PASS test_pass_1\nFAIL my_test")));
    assert_eq!(parsed.get("result_ref_only"), Some(&serde_json::json!(true)));
    assert_eq!(parsed.get("spool_path"), Some(&serde_json::json!(spool_relative)));
    assert_eq!(parsed.get("spooled_bytes"), Some(&serde_json::json!(38_912)));
    assert_eq!(parsed.get("auto_recovered"), Some(&serde_json::json!(true)));
    assert!(
        parsed.get("failure_diagnostics").is_some(),
        "failure_diagnostics should remain alongside the bounded preview"
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("output"), Some(&serde_json::json!("bash: pip: command not found")));
    assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(127)));
    assert_eq!(parsed.get("critical_note"), Some(&serde_json::json!("Command `pip` was not found in PATH.")));
    assert_eq!(
        parsed.get("next_action"),
        Some(&serde_json::json!("Check the command name or install the missing binary, then rerun the command."))
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
            "fallback_tool": tool_names::CODE_SEARCH,
            "fallback_tool_args": {"query":"Widget","path":"."}
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("next_action"), Some(&serde_json::json!("Retry with fallback_tool_args.")));
    assert_eq!(parsed.get("fallback_tool"), Some(&serde_json::json!(tool_names::CODE_SEARCH)));
}

#[test]
fn maybe_inline_spooled_keeps_code_search_refinement_guidance() {
    let serialized = maybe_inline_spooled(
        tool_names::CODE_SEARCH,
        &serde_json::json!({
            "results": [],
            "is_recoverable": true,
            "hint": "Try narrowing the path.",
            "next_action": "Retry with narrower filters."
        }),
    );

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("next_action"), Some(&serde_json::json!("Retry with narrower filters.")));
    assert_eq!(parsed.get("hint"), Some(&serde_json::json!("Try narrowing the path.")));
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
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

    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("serialized JSON payload");
    assert_eq!(parsed.get("output"), Some(&serde_json::json!("ok")));
    assert!(parsed.get("critical_note").is_none());
    assert!(parsed.get("next_action").is_none());
}

#[test]
fn tool_output_summary_input_uses_bounded_preview_for_exec_output() {
    let spool_content = (1..=150).map(|idx| format!("line-{idx}")).collect::<Vec<_>>().join("\n");
    let output = serde_json::json!({
        "output": "preview text",
        "stderr_preview": "warning text",
        "spool_path": ".vtcode/context/tool_outputs/command_session_1.txt",
        "preview": spool_content,
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);
    let input = tool_output_summary_input_or_serialized(tool_names::UNIFIED_EXEC, &output, &serialized);

    assert!(input.contains("Tool payload:"));
    assert!(input.contains("stderr_preview:\nwarning text"));
    assert!(input.contains("tail_excerpt:"));
    assert!(input.contains("line-150"));
    assert!(!input.contains("line-1\nline-2\nline-3"));
}

#[test]
fn tool_output_summary_input_uses_bounded_preview_excerpt_for_large_reads() {
    let spool_content = (1..=200).map(|idx| format!("read-line-{idx}")).collect::<Vec<_>>().join("\n");
    let output = serde_json::json!({
        "path": "src/main.rs",
        "spool_path": ".vtcode/context/tool_outputs/read_1.txt",
        "preview": spool_content
    });
    let serialized = serialize_json_for_model(&output);
    let input = tool_output_summary_input_or_serialized(tool_names::READ_FILE, &output, &serialized);

    assert!(input.contains("source_path: src/main.rs"));
    assert!(input.contains("content_excerpt:"));
    assert!(input.contains("read-line-1"));
    assert!(input.contains("read-line-200"));
}

#[test]
fn tool_output_summary_input_falls_back_to_serialized_output_when_spool_missing() {
    let output = serde_json::json!({
        "spool_path": ".vtcode/context/tool_outputs/missing.txt",
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(tool_names::UNIFIED_EXEC, &output, &serialized);

    assert_eq!(input, serialized);
}

#[test]
fn tool_output_summary_input_preserves_utf8_preview() {
    let output = serde_json::json!({
        "output": "preview text",
        "stderr_preview": "warning text",
        "spool_path": ".vtcode/context/tool_outputs/command_session_invalid.txt",
        "preview": "ok\n�\nlast line\n",
        "exit_code": 0,
        "is_exited": true
    });
    let serialized = serialize_json_for_model(&output);

    let input = tool_output_summary_input_or_serialized(tool_names::UNIFIED_EXEC, &output, &serialized);

    assert!(input.contains("warning text"));
    assert!(input.contains("last line"));
    assert_ne!(input, serialized);
}

#[test]
fn blocked_or_denied_failure_detects_guardable_errors() {
    assert!(is_blocked_or_denied_failure(
        "task_tracker is only available when planning workflow state is active."
    ));
    assert!(is_blocked_or_denied_failure("Tool permission denied"));
    assert!(is_blocked_or_denied_failure("Policy violation: exceeded max tool calls per turn (32)"));
    assert!(is_blocked_or_denied_failure("Safety validation failed: command pattern denied"));
}

#[test]
fn blocked_or_denied_failure_ignores_runtime_execution_failures() {
    assert!(!is_blocked_or_denied_failure("command exited with status 1"));
    assert!(!is_blocked_or_denied_failure("stream request timed out after 30000ms"));
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
    assert_eq!(parsed.touched_files, vec!["src/agent/runloop/unified/turn/compaction.rs"]);
    assert_eq!(parsed.verification, vec!["Run cargo check"]);
    assert_eq!(parsed.open_questions, vec!["Should batch reads be deduped too?"]);
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
    assert_eq!(update.touched_files, vec!["src/agent/runloop/unified/turn/compaction.rs".to_string()]);
    assert_eq!(update.verification_todo, vec!["Run cargo check".to_string()]);
    assert_eq!(
        update.delegation_notes,
        vec!["reviewer: Investigated compaction flow | Confirmed contract".to_string()]
    );
}

#[test]
fn build_subagent_memory_update_ignores_background_subprocess_entries() {
    // `agent wait` returns managed background subprocess entries with the same
    // `completed`/`entry` envelope as delegated children. Their readiness text
    // is not a delegated summary and must not enter session memory.
    let output = serde_json::json!({
        "completed": true,
        "entry": {
            "agent_name": "background-demo",
            "desired_enabled": false,
            "status": "stopped",
            "summary": "[demo-background-subagent] ready pid=123"
        }
    });

    assert!(
        build_subagent_memory_update(&output).is_none(),
        "background subprocess completion must not become a delegated memory note"
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
    assert_eq!(update.delegation_notes, vec!["worker: plain child summary".to_string()]);
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
