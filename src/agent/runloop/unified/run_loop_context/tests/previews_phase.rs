use super::*;

#[test]
fn repeated_tool_previews_keep_fresh_evidence_visible() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 10, 1);
    for index in 0..40 {
        let evidence = format!("source-{index} {}", "x".repeat(8_000));
        assert_eq!(state.bound_model_visible_tool_preview(Some("read_file"), evidence.clone()), evidence);
        let verifier =
            serde_json::json!({"exit_code": 1, "output": "check failed", "stderr": "missing dependency"}).to_string();
        assert_eq!(state.bound_model_visible_tool_preview(Some("exec_command"), verifier.clone()), verifier);
    }
    assert!(!state.model_visible_preview_budget_exhausted());
    assert_eq!(state.suppressed_tool_previews, 0);
}
#[test]
fn oversized_result_retains_head_tail_and_does_not_blind_next_call() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 10, 1);
    let oversized = serde_json::json!({
        "output": format!("HEAD{}TAIL", "界".repeat(24_000)),
        "spool_path": ".vtcode/context/tool_outputs/result.txt", "spool_complete": true,
        "exit_code": 1, "stderr": "error: missing dependency", "success": false,
    })
    .to_string();
    let visible = state.bound_model_visible_tool_preview(Some("exec_command"), oversized);
    let parsed: serde_json::Value = serde_json::from_str(&visible).unwrap();
    assert!(visible.len() < TURN_PREVIEW_BUDGET_BYTES);
    assert_eq!(parsed["exit_code"], 1);
    assert_eq!(parsed["success"], false);
    assert_eq!(parsed["spool_path"], ".vtcode/context/tool_outputs/result.txt");
    assert_eq!(parsed["stderr"], "error: missing dependency");
    let preview = parsed["preview"].as_str().unwrap();
    assert!(preview.starts_with("HEAD"));
    assert!(preview.ends_with("TAIL"));
    assert!(parsed.get("preview_budget_exhausted").is_none());
    assert!(!state.model_visible_preview_budget_exhausted());
    assert_eq!(state.bound_model_visible_tool_preview(Some("read_file"), "fresh evidence".into()), "fresh evidence");
}
#[test]
fn oversized_preview_preserves_failure_metadata_without_secrets_and_counts_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 10, 1);
    let content = serde_json::json!({
        "output": "x".repeat(80_000), "exit_code": 1, "success": false,
        "spool_line_count": 1,
        "error": {"message": "permission denied: token=secret-not-for-context", "retryable": false},
        "diagnosis": {"observed": "exit 1", "next_action": "\u{1b}[31minspect compiler error\u{1b}[0m\npassword=secret-not-for-context"},
    }).to_string();
    for _ in 0..2 {
        let result = state.bound_model_visible_tool_preview_for_call_with_budget(
            "failure",
            Some("exec_command"),
            content.clone(),
            TURN_PREVIEW_BUDGET_BYTES,
        );
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["exit_code"], 1);
        assert_eq!(parsed["spool_line_count"], 1);
        assert_eq!(parsed["error"]["retryable"], false);
        assert!(parsed["error"]["message"].as_str().unwrap().contains("permission denied"));
        assert!(
            parsed["diagnosis"]["next_action"]
                .as_str()
                .unwrap()
                .contains("inspect compiler error")
        );
        assert!(!result.contains("secret-not-for-context"));
        assert!(!result.contains("\\u001b"));
        assert!(parsed.get("preview_budget_exhausted").is_none());
    }
    assert_eq!(state.suppressed_tool_previews, 1);
    assert!(!state.model_visible_preview_budget_exhausted());
}
#[test]
fn upstream_preview_exhaustion_latches_before_body_checks_and_is_idempotent() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);
    let registry_stub = serde_json::json!({
        "content_type": "git_diff",
        "exit_code": 0,
        "total_output_bytes": 5_212,
        "preview_budget_exhausted": true,
    })
    .to_string();

    assert!(state.observe_upstream_preview_budget_exhaustion("call-diff", &registry_stub, TURN_PREVIEW_BUDGET_BYTES,));
    assert!(state.model_visible_preview_budget_exhausted());
    assert_eq!(state.suppressed_tool_previews, 1);

    // A terminal update for the same call must not inflate diagnostics.
    assert!(state.observe_upstream_preview_budget_exhaustion("call-diff", &registry_stub, TURN_PREVIEW_BUDGET_BYTES,));
    assert_eq!(state.suppressed_tool_previews, 1);

    // A distinct suppressed result is counted independently.
    assert!(
        state.observe_upstream_preview_budget_exhaustion("call-diff-2", &registry_stub, TURN_PREVIEW_BUDGET_BYTES,)
    );
    let diagnostics = state.snapshot_turn_diagnostics(Default::default(), 0);
    assert!(diagnostics.model_visible_tool_preview_budget_exhausted);
    assert_eq!(diagnostics.suppressed_tool_previews, 2);
}
#[test]
fn upstream_preview_exhaustion_ignores_non_marker_payloads() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);
    let cases = [
        ("plain text is not JSON", "tool output without any marker"),
        ("missing flag defaults to absent", r#"{"tool":"exec_command","exit_code":0}"#),
        ("wrong-typed flag is not exhaustion", r#"{"preview_budget_exhausted":"true"}"#),
        ("explicit false is not exhaustion", r#"{"preview_budget_exhausted":false}"#),
        ("non-object JSON is not exhaustion", r#"["preview_budget_exhausted"]"#),
    ];
    for (name, content) in cases {
        assert!(
            !state.observe_upstream_preview_budget_exhaustion("call", content, TURN_PREVIEW_BUDGET_BYTES),
            "{name} must not latch exhaustion"
        );
    }
    assert!(!state.model_visible_preview_budget_exhausted());
    assert_eq!(state.suppressed_tool_previews, 0);
}
#[test]
fn per_result_limits_differ_by_mode_without_exhausting_tools() {
    let payload = "a".repeat(80 * 1024);
    for (limit, truncated) in [
        (TURN_PREVIEW_BUDGET_BYTES, true),
        (TURN_PREVIEW_BUDGET_BYTES_PLANNING, false),
    ] {
        let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 10, 1);
        let visible = state.bound_model_visible_tool_preview_with_budget(Some("read_file"), payload.clone(), limit);
        assert_eq!(visible != payload, truncated);
        assert!(visible.len() <= limit);
        assert!(!state.model_visible_preview_budget_exhausted());
    }
}
#[test]
fn budget_exhausted_record_carries_exact_ceiling_values() {
    use super::super::{BudgetExhaustedMetrics, budget_exhausted_record, budget_kind};

    let value = serde_json::to_value(budget_exhausted_record(BudgetExhaustedMetrics {
        budget: budget_kind::WALL_CLOCK,
        used: 600,
        max: 600,
        step_count: None,
        planning_active: false,
        tool_calls: 41,
    }))
    .unwrap();
    assert_eq!(value["kind"], "budget_exhausted");
    assert_eq!(value["budget"], "wall_clock_secs");
    assert_eq!(value["used"], 600);
    assert_eq!(value["max"], 600);
    assert!(value.get("step_count").is_none());
    assert_eq!(value["planning_active"], false);
    assert_eq!(value["tool_calls"], 41);
    assert!(value["ts"].is_number());

    // Asymmetric counterpart: a planning tool-loop record differs in
    // every mode-dependent field, so consumers can tell ceilings apart.
    let planned = serde_json::to_value(budget_exhausted_record(BudgetExhaustedMetrics {
        budget: budget_kind::TOOL_LOOP,
        used: 20,
        max: 20,
        step_count: Some(20),
        planning_active: true,
        tool_calls: 41,
    }))
    .unwrap();
    assert_eq!(planned["budget"], "tool_loop");
    assert_eq!(planned["step_count"], 20);
    assert_eq!(planned["planning_active"], true);
    assert_ne!(planned["budget"], value["budget"]);
}
#[test]
fn oversized_suppressed_preview_skips_unbounded_json_parse() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);
    state.bound_model_visible_tool_preview(Some("exec_command"), "a".repeat(TURN_PREVIEW_BUDGET_BYTES));

    let content = format!(
        "{{\"error_summary\":\"should-not-be-parsed\",\"output\":\"{}\"}}",
        "b".repeat(TOOL_PREVIEW_METADATA_PARSE_LIMIT_BYTES)
    );
    let metadata = state.bound_model_visible_tool_preview(Some("exec_command"), content);

    assert!(metadata.contains("\"preview_truncated\":true"));
    let parsed: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert!(parsed.get("error_summary").is_none());
    assert!(metadata.contains("\"byte_count\":"));
}
#[test]
fn harness_state_tracks_phase_transitions() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    // Verify that run_id and turn_id are accessible
    assert_eq!(state.run_id.0, "run-1");
    assert_eq!(state.turn_id.0, "turn-1");

    assert_eq!(state.phase, TurnPhase::Preparing);
    state.set_phase(TurnPhase::Requesting);
    assert_eq!(state.phase, TurnPhase::Requesting);
    state.set_phase(TurnPhase::ExecutingTools);
    assert_eq!(state.phase, TurnPhase::ExecutingTools);
    state.set_phase(TurnPhase::Finalizing);
    assert_eq!(state.phase, TurnPhase::Finalizing);
}
#[test]
fn harness_state_accounts_for_out_of_band_tool_calls() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    state.record_out_of_band_tool_call();

    let diagnostics = state.snapshot_turn_diagnostics(Default::default(), 0);
    assert_eq!(diagnostics.requested_tool_calls, 1);
    assert_eq!(diagnostics.admitted_tool_calls, 1);
    assert_eq!(diagnostics.unadmitted_tool_calls, 0);
    assert!(state.has_out_of_band_tool_progress());
}
#[test]
fn harness_state_tracks_spool_chunk_read_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    assert_eq!(state.record_spool_chunk_read(), 1);
    assert_eq!(state.record_spool_chunk_read(), 2);
    state.reset_spool_chunk_read_streak();
    assert_eq!(state.record_spool_chunk_read(), 1);
}
#[test]
fn harness_state_counts_tool_calls_without_warning_thresholds() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);

    for _ in 0..10 {
        state.record_tool_call();
    }

    assert_eq!(state.tool_calls, 10);
}
