#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
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
        assert_ne!(base_key, low_signal_family_key(tool_names::CODE_SEARCH, &changed));
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
    assert_eq!(path, Some(".vtcode/context/tool_outputs/command_session_123.txt"));
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
    assert_eq!(signature, Some("command_session::cargo check -p vtcode-core".to_string()));
}

#[test]
fn shell_run_signature_handles_command_session_run_action() {
    let args = json!({
        "action": "run",
        "command": ["cargo", "check", "-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(signature, Some("command_session::cargo check -p vtcode-core".to_string()));
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
fn exhausted_turn_budget_errors_do_not_invite_another_tool_call() {
    use crate::agent::runloop::unified::run_loop_context::ToolWallClockExhaustion;

    let calls = ToolBudgetExhaustion { used: 32, max: 32, remaining: 0 };
    let wall_clock = ToolWallClockExhaustion { max_secs: 60 };
    for error in [
        calls.policy_violation_message(),
        calls.skipped_call_message(),
        wall_clock.policy_violation_message(),
        wall_clock.skipped_call_message(),
    ] {
        let payload: serde_json::Value =
            serde_json::from_str(&super::super::build_turn_budget_error_content(error.clone())).unwrap();
        assert_eq!(payload["error"], error);
        if error.contains("call skipped") {
            let next_action = payload["next_action"].as_str().expect("compact stub recovery guidance");
            assert!(next_action.contains("further tool calls are skipped"));
            assert!(next_action.contains("Synthesize your final answer now"));
        }
        assert!(!payload.to_string().contains("alternative tool"));
        assert!(!payload.to_string().contains("narrower scope"));
    }

    let ordinary: serde_json::Value =
        serde_json::from_str(&super::super::build_failure_error_content("temporary failure".into(), "policy")).unwrap();
    assert_eq!(ordinary["next_action"], "Try an alternative tool or narrower scope.");
}

#[test]
fn tool_budget_exhausted_directive_demands_synthesis() {
    let exhaustion = ToolBudgetExhaustion { used: 32, max: 32, remaining: 0 };
    let directive = exhaustion.synthesis_directive_message();
    assert!(directive.contains("32/32"));
    assert!(directive.contains("Synthesize your final answer now"));
    assert!(exhaustion.skipped_call_message().contains("call skipped"));
}

#[test]
fn preflight_name_mistakes_are_llm_mistakes_and_do_not_trip() {
    // Prose-blob tool names and unknown tools must classify as LLM mistakes
    // so one malformed name cannot trip the preflight circuit and skip valid
    // sibling calls in the same assistant batch.
    assert!(preflight_failure_is_llm_mistake(
        "tool name is not a clean identifier: exec_command\n... the fence opener is unclosed"
    ));
    assert!(preflight_failure_is_llm_mistake("Unknown tool: exec_command\n: Tool error"));
    assert!(preflight_failure_is_llm_mistake("Tool call has an empty tool name. Provide a valid tool name."));
    assert!(preflight_failure_is_llm_mistake("Tool preflight validation failed: Unknown tool: ` — `"));
    // A tool name that merely contains "sandbox" is still a name mistake.
    assert!(preflight_failure_is_llm_mistake("Unknown tool: sandbox_helper"));
    // Prose inside the embedded name must not flip the classification even
    // when it contains policy phrases: classification runs on the static
    // prefix before the name, never on the name itself.
    assert!(preflight_failure_is_llm_mistake(
        "tool name is not a clean identifier: write_file is not allowed here, use apply_patch"
    ));
    assert!(preflight_failure_is_llm_mistake(
        "tool name is not a clean identifier: I will not invoke command injection"
    ));
    assert!(preflight_failure_is_llm_mistake(
        "tool name is not a clean identifier: sandbox policy requires approval"
    ));
}

#[test]
fn preflight_policy_and_argument_failures_still_trip() {
    // Security/policy blocks and repeated argument-schema failures keep
    // counting toward the circuit: those mean the model is fighting the
    // harness or stuck on bad JSON.
    assert!(!preflight_failure_is_llm_mistake(
        "Command security check failed: Command injection pattern detected"
    ));
    assert!(!preflight_failure_is_llm_mistake(
        "Tool preflight validation failed for 'exec_command': Missing required argument: command"
    ));
    assert!(!preflight_failure_is_llm_mistake("Invalid arguments: expected value"));
    assert!(!preflight_failure_is_llm_mistake("Policy violation: sandbox denied"));
    assert!(!preflight_failure_is_llm_mistake("Command security check failed: sandbox policy denied"));
    // A policy prefix wrapping the name-mistake constant still counts: the
    // static prefix carries the policy phrase.
    assert!(!preflight_failure_is_llm_mistake(
        "command security check failed: tool name is not a clean identifier: exec_command"
    ));
}
