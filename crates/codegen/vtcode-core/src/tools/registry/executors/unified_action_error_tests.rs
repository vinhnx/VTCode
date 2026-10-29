use std::time::Duration;

use serde_json::json;

use super::{
    CargoTestCommandKind, ExecOutputPreview, PtyEphemeralCapture, attach_exec_recovery_guidance,
    attach_failure_diagnostics_metadata, build_exec_output_preview, build_exec_response, build_head_tail_preview,
    cargo_selector_error_diagnostics, cargo_test_failure_diagnostics, cargo_test_rerun_hint, clamp_inspect_lines,
    clamp_max_matches, extract_run_session_id_from_read_file_error, extract_run_session_id_from_tool_output_path,
    filter_lines, missing_command_session_action_error, resolve_exec_run_session_id, summarized_arg_keys,
};
use crate::tools::types::VTCodeExecSession;

#[test]
fn summarized_arg_keys_reports_shape_for_non_object_payloads() {
    assert_eq!(summarized_arg_keys(&json!(null)), "<null>");
    assert_eq!(summarized_arg_keys(&json!(["a", "b"])), "<array>");
    assert_eq!(summarized_arg_keys(&json!("x")), "<string>");
}

#[test]
fn exec_command_missing_action_error_includes_received_keys() {
    let err = missing_command_session_action_error(&json!({
        "foo": "bar",
        "session_id": "123"
    }));
    let text = err.to_string();
    assert!(text.contains("Missing command session action"));
    assert!(text.contains("foo"));
    assert!(text.contains("session_id"));
}

#[test]
fn extracts_run_session_id_from_tool_output_path() {
    assert_eq!(
        extract_run_session_id_from_tool_output_path(".vtcode/context/tool_outputs/run-abc123.txt"),
        Some("run-abc123".to_string())
    );
    assert_eq!(extract_run_session_id_from_tool_output_path(".vtcode/context/tool_outputs/not-a-session.txt"), None);
}

#[test]
fn extracts_run_session_id_from_read_file_error() {
    let error = "Use exec_command with session_id=\"run-zz9\" instead of read_file.";
    assert_eq!(extract_run_session_id_from_read_file_error(error), Some("run-zz9".to_string()));
    assert_eq!(extract_run_session_id_from_read_file_error("no session"), None);
}

#[test]
fn resolve_exec_run_session_id_prefers_requested_session_id() {
    let payload = json!({ "session_id": " check_sh " });
    let payload = payload.as_object().expect("object");

    assert_eq!(resolve_exec_run_session_id(payload).expect("requested session id"), "check_sh");
}

#[test]
fn resolve_exec_run_session_id_generates_default_when_missing() {
    let payload = json!({});
    let payload = payload.as_object().expect("object");
    let session_id = resolve_exec_run_session_id(payload).expect("generated session id");

    assert!(session_id.starts_with("run-"));
}

#[test]
fn resolve_exec_run_session_id_rejects_invalid_values() {
    let payload = json!({ "session_id": "bad id" });
    let payload = payload.as_object().expect("object");
    let err = resolve_exec_run_session_id(payload).expect_err("invalid session id");

    assert!(err.to_string().contains("Invalid session_id"));
}

#[test]
fn inspect_helpers_clamp_limits() {
    assert_eq!(clamp_inspect_lines(Some(0), 30), 0);
    assert_eq!(clamp_inspect_lines(Some(9_999), 30), 5_000);
    assert_eq!(clamp_max_matches(None), 200);
    assert_eq!(clamp_max_matches(Some(0)), 1);
    assert_eq!(clamp_max_matches(Some(50_000)), 10_000);
}

#[test]
fn inspect_helpers_build_head_tail_preview() {
    let content = "l1\nl2\nl3\nl4\nl5\nl6";
    let (preview, truncated) = build_head_tail_preview(content, 2, 2);
    assert!(truncated);
    assert!(preview.contains("l1"));
    assert!(preview.contains("l2"));
    assert!(preview.contains("l5"));
    assert!(preview.contains("l6"));
}

#[test]
fn inspect_helpers_filter_lines_literal() {
    let (output, matched, truncated) = filter_lines("alpha\nbeta\nalpha2", "alpha", true, 1).expect("filter");
    assert_eq!(matched, 2);
    assert!(truncated);
    assert!(output.contains("1: alpha"));
}

#[test]
fn exec_output_preview_truncates_on_utf8_boundaries() {
    let (preview, truncated) = build_exec_output_preview("a🙂b", 1);

    assert!(truncated);
    // The byte window is head+tail, so the last output content survives
    // the cut (the part that carries build/test summaries).
    assert!(preview.starts_with('a'));
    assert!(preview.ends_with('b'));
    assert!(preview.contains("bytes omitted"));
    std::str::from_utf8(preview.as_bytes()).unwrap();
}

#[test]
fn exec_output_preview_keeps_tail_of_long_output_within_budget() {
    let lines: String = (0..1000).map(|idx| format!("line {idx}\n")).collect();
    let (preview, truncated) = build_exec_output_preview(&lines, 1000);

    assert!(truncated);
    // Head shows the first line, tail shows the last — a head-only cut
    // lost the tail where build/test summaries live.
    assert!(preview.starts_with("line 0\n"));
    assert!(preview.trim_end().ends_with("line 999"));
    assert!(preview.contains("bytes omitted"));
    // The inline budget stays essentially intact (marker bytes aside).
    assert!(preview.len() <= 1000 * 4 + 64, "preview {} must stay near the 4000-byte budget", preview.len());
}

#[test]
fn exec_recovery_guidance_sets_command_not_found_metadata() {
    let session = VTCodeExecSession {
        id: "run-123".to_string().into(),
        backend: "pipe".to_string(),
        command: "zsh".to_string(),
        args: vec!["-c".to_string(), "pip install pymupdf".to_string()],
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
        output: String::new(),
        exit_code: Some(127),
        duration: Duration::from_millis(42),
    };

    let response = build_exec_response(
        &session,
        "pip install pymupdf",
        &capture,
        ExecOutputPreview {
            raw_output: "bash: pip: command not found".to_string(),
            output: "bash: pip: command not found".to_string(),
            truncated: false,
        },
        None,
        false,
        None,
    );

    assert_eq!(response["output"], "bash: pip: command not found");
    assert_eq!(response["exit_code"], 127);
    assert_eq!(response["session_id"], "run-123");
    assert_eq!(response["command"], "pip install pymupdf");
    assert_eq!(response["critical_note"], "Command `pip` was not found in PATH.");
    assert_eq!(
        response["next_action"],
        "Check the command name or install the missing binary, then rerun the command."
    );
}

#[test]
fn exec_recovery_guidance_ignores_non_command_not_found_exit_codes() {
    let mut response = json!({});
    attach_exec_recovery_guidance(&mut response, "cargo test", Some(1));
    assert!(response.get("critical_note").is_none());
    assert!(response.get("next_action").is_none());
}

#[test]
fn exec_recovery_guidance_redirects_apply_patch_shell_collision_to_tool() {
    let mut response = json!({});
    attach_exec_recovery_guidance(&mut response, "apply_patch", Some(127));
    assert_eq!(
        response["critical_note"],
        "Command `apply_patch` is not a shell binary — use the `apply_patch` tool instead."
    );
    assert!(
        response["next_action"]
            .as_str()
            .expect("next_action present")
            .contains("Call the `apply_patch` tool")
    );

    let mut aliased = json!({});
    attach_exec_recovery_guidance(&mut aliased, "applypatch", Some(127));
    assert_eq!(aliased["critical_note"], response["critical_note"]);

    let mut with_args = json!({});
    attach_exec_recovery_guidance(&mut with_args, "/usr/bin/apply_patch --help", Some(127));
    assert_eq!(with_args["critical_note"], response["critical_note"]);

    let mut sudo_prefixed = json!({});
    attach_exec_recovery_guidance(&mut sudo_prefixed, "sudo apply_patch", Some(127));
    assert_eq!(sudo_prefixed["critical_note"], "Command `sudo` was not found in PATH.");
}

#[test]
fn cargo_selector_error_diagnostics_classifies_missing_test_target() {
    let output =
        "error: no test target named `exec_only_policy_skips_when_full_auto_is_disabled` in `vtcode-core` package\n";

    let diagnostics = cargo_selector_error_diagnostics(
        CargoTestCommandKind::Nextest,
        "cargo nextest run --test exec_only_policy_skips_when_full_auto_is_disabled -p vtcode-core --no-capture",
        output,
    )
    .expect("selector diagnostics");

    assert_eq!(diagnostics["kind"], "cargo_test_selector_error");
    assert_eq!(diagnostics["package"], "vtcode-core");
    assert_eq!(diagnostics["requested_test_target"], "exec_only_policy_skips_when_full_auto_is_disabled");
    assert_eq!(diagnostics["selector_error"], true);
    assert_eq!(
        diagnostics["validation_hint"],
        "cargo test -p vtcode-core --lib -- --list | rg 'exec_only_policy_skips_when_full_auto_is_disabled'"
    );
    assert_eq!(
        diagnostics["rerun_hint"],
        "cargo nextest run -p vtcode-core exec_only_policy_skips_when_full_auto_is_disabled"
    );
}

#[test]
fn cargo_test_failure_diagnostics_extracts_unit_test_failure_details() {
    let output = r#"────────────
    Nextest run ID 18fffe01-0ef9-4113-9a81-2344a7cc3c16 with nextest profile: default
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    stderr ───
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at crates/codegen/vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;

    let diagnostics = cargo_test_failure_diagnostics("cargo nextest run -p vtcode-core", output, Some(100))
        .expect("failure diagnostics");

    assert_eq!(diagnostics["kind"], "cargo_test_failure");
    assert_eq!(diagnostics["package"], "vtcode-core");
    assert_eq!(diagnostics["binary_kind"], "unit");
    assert_eq!(
        diagnostics["test_fqname"],
        "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
    );
    assert_eq!(diagnostics["panic"], "task result: Invalid request: QueuedProvider has no queued responses");
    assert_eq!(diagnostics["source_file"], "crates/codegen/vtcode-core/src/core/agent/runner/tests.rs");
    assert_eq!(diagnostics["source_line"], 692);
    assert_eq!(
        diagnostics["rerun_hint"],
        cargo_test_rerun_hint(
            CargoTestCommandKind::Nextest,
            "vtcode-core",
            "unit",
            "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled",
        )
    );
}

#[test]
fn build_exec_response_attaches_cargo_failure_diagnostics() {
    let session = VTCodeExecSession {
        id: "run-123".to_string().into(),
        backend: "pipe".to_string(),
        command: "cargo".to_string(),
        args: vec![
            "nextest".to_string(),
            "run".to_string(),
            "-p".to_string(),
            "vtcode-core".to_string(),
        ],
        working_dir: Some(".".to_string()),
        background: false,
        rows: None,
        cols: None,
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    };
    let raw_output = r#"
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at crates/codegen/vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;
    let capture = PtyEphemeralCapture {
        output: raw_output.to_string(),
        exit_code: Some(100),
        duration: Duration::from_millis(42),
    };

    let response = build_exec_response(
        &session,
        "cargo nextest run -p vtcode-core",
        &capture,
        ExecOutputPreview {
            raw_output: raw_output.to_string(),
            output: raw_output.to_string(),
            truncated: false,
        },
        None,
        false,
        None,
    );

    assert_eq!(
        response["failure_diagnostics"]["test_fqname"],
        "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
    );
    assert_eq!(response["package"], "vtcode-core");
    assert_eq!(response["binary_kind"], "unit");
    assert_eq!(response["source_file"], "crates/codegen/vtcode-core/src/core/agent/runner/tests.rs");
    assert_eq!(response["source_line"], 692);
    assert_eq!(
        response["rerun_hint"],
        "cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
    );
    assert_eq!(
        response["next_action"],
        "Rerun the failing test directly with: cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
    );
}

#[test]
fn attach_failure_diagnostics_metadata_promotes_selector_hints() {
    let mut response = json!({
        "success": true,
        "command": "cargo nextest run --test bad -p vtcode-core"
    });
    let diagnostics = json!({
        "kind": "cargo_test_selector_error",
        "package": "vtcode-core",
        "binary_kind": "test_target_selector",
        "requested_test_target": "bad",
        "selector_error": true,
        "validation_hint": "cargo test -p vtcode-core --lib -- --list | rg 'bad'",
        "rerun_hint": "cargo nextest run -p vtcode-core bad",
        "critical_note": "selector mismatch",
        "next_action": "validate first"
    });

    attach_failure_diagnostics_metadata(&mut response, &diagnostics);

    assert_eq!(response["package"], "vtcode-core");
    assert_eq!(response["binary_kind"], "test_target_selector");
    assert_eq!(response["selector_error"], true);
    assert_eq!(response["validation_hint"], "cargo test -p vtcode-core --lib -- --list | rg 'bad'");
    assert_eq!(response["rerun_hint"], "cargo nextest run -p vtcode-core bad");
    assert_eq!(response["critical_note"], "selector mismatch");
    assert_eq!(response["next_action"], "validate first");
    assert_eq!(response["failure_diagnostics"]["kind"], "cargo_test_selector_error");
}
