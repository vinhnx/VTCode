use std::time::{Duration, Instant};

use hashbrown::HashSet;

use super::{
    CrossTurnTracker, DiagnosisMemoEntry, DiagnosisMemoKey, HarnessTurnState, RecoveryMode,
    SESSION_LIMIT_AUTO_GRANT_INCREMENT, TOOL_BUDGET_WARNING_THRESHOLD, TOOL_PREVIEW_METADATA_PARSE_LIMIT_BYTES,
    ToolBudgetExhaustion, ToolBudgetExhaustionNotice, ToolBudgetWarning, ToolWallClockExhaustion,
    ToolWallClockExhaustionNotice, TurnId, TurnPhase, TurnRunId, full_auto_loop_grants_enabled,
};
use vtcode_config::constants::output_limits::{TURN_PREVIEW_BUDGET_BYTES, TURN_PREVIEW_BUDGET_BYTES_PLANNING};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::types::CompactStr;

#[test]
fn repeated_tool_previews_keep_fresh_evidence_visible() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 32, 10, 1);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 32, 10, 1);
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
        let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);
        let visible = state.bound_model_visible_tool_preview_with_budget(Some("read_file"), payload.clone(), limit);
        assert_eq!(visible != payload, truncated);
        assert!(visible.len() <= limit);
        assert!(!state.model_visible_preview_budget_exhausted());
    }
}

#[test]
fn budget_exhausted_record_carries_exact_ceiling_values() {
    use super::{BudgetExhaustedMetrics, budget_exhausted_record, budget_kind};

    let value = serde_json::to_value(budget_exhausted_record(BudgetExhaustedMetrics {
        budget: budget_kind::TOOL_CALLS,
        used: 32,
        max: 32,
        step_count: None,
        planning_active: false,
        tool_calls: 32,
    }))
    .unwrap();
    assert_eq!(value["kind"], "budget_exhausted");
    assert_eq!(value["budget"], "tool_calls");
    assert_eq!(value["used"], 32);
    assert_eq!(value["max"], 32);
    assert!(value.get("step_count").is_none());
    assert_eq!(value["planning_active"], false);
    assert_eq!(value["tool_calls"], 32);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);
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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

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
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

    state.record_out_of_band_tool_call();

    let diagnostics = state.snapshot_turn_diagnostics(Default::default(), 0);
    assert_eq!(diagnostics.requested_tool_calls, 1);
    assert_eq!(diagnostics.admitted_tool_calls, 1);
    assert_eq!(diagnostics.unadmitted_tool_calls, 0);
    assert!(state.has_out_of_band_tool_progress());
}

#[test]
fn harness_state_tracks_spool_chunk_read_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

    assert_eq!(state.record_spool_chunk_read(), 1);
    assert_eq!(state.record_spool_chunk_read(), 2);
    state.reset_spool_chunk_read_streak();
    assert_eq!(state.record_spool_chunk_read(), 1);
}

#[test]
fn harness_state_tracks_budget_warning_threshold_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    state.record_tool_call(); // 1/4
    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    state.record_tool_call(); // 2/4
    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    state.record_tool_call(); // 3/4 => 75%
    assert!(state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    state.mark_tool_budget_warning_emitted();
    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    assert_eq!(state.remaining_tool_calls(), 1);
}

#[test]
fn harness_state_records_budget_warning_once_via_helper() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.record_tool_call_with_default_warning(), None);
    assert_eq!(state.record_tool_call_with_default_warning(), None);
    assert_eq!(
        state.record_tool_call_with_default_warning(),
        Some(ToolBudgetWarning { used: 3, max: 4, remaining: 1 })
    );
    assert_eq!(state.record_tool_call_with_default_warning(), None);
}

#[test]
fn tool_budget_warning_system_message_matches_contract() {
    assert_eq!(
        ToolBudgetWarning { used: 3, max: 4, remaining: 1 }.system_message(),
        "Tool-call budget warning: 3/4 used; 1 remaining for this turn. Use targeted extraction/batching before additional tool calls."
    );
}

#[test]
fn harness_state_records_budget_exhaustion_notice_once_via_helper() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 10, 1);

    assert!(!state.tool_budget_exhausted());
    assert!(!state.tool_budget_exhausted_emitted);
    state.record_tool_call();
    assert!(state.tool_budget_exhausted());
    assert_eq!(
        state.record_tool_budget_exhaustion_notice(),
        Some(ToolBudgetExhaustionNotice {
            exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
            first_notice: true,
        })
    );
    assert!(state.tool_budget_exhausted_emitted);
    assert_eq!(
        state.record_tool_budget_exhaustion_notice(),
        Some(ToolBudgetExhaustionNotice {
            exhaustion: ToolBudgetExhaustion { used: 1, max: 1, remaining: 0 },
            first_notice: false,
        })
    );
}

#[test]
fn tool_budget_exhaustion_synthesis_directive_matches_contract() {
    assert_eq!(
        ToolBudgetExhaustion { used: 4, max: 4, remaining: 0 }.synthesis_directive_message(),
        "Tool-call budget exhausted for this turn (4/4). Tools are disabled for the rest of this turn, so further tool calls are skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation."
    );
}

#[test]
fn tool_budget_exhaustion_notice_arms_synthesis_directive_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 600, 3);
    state.record_tool_call();
    assert!(state.record_tool_budget_exhaustion_notice().is_some());
    assert!(state.take_tool_budget_directive_pending());
    // Second rejected call in the same turn must not re-arm the directive.
    assert!(state.record_tool_budget_exhaustion_notice().is_some());
    assert!(!state.take_tool_budget_directive_pending());
}

#[test]
fn tool_budget_rejection_is_separate_from_permission_denial() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 1, 10, 1);

    state.record_tool_budget_rejection();
    assert!(state.take_tool_budget_rejection());
    assert!(!state.take_tool_budget_rejection());
    assert_eq!(state.snapshot_turn_diagnostics(Default::default(), 0).denied_tool_calls, 0);

    state.record_denied_tool_call();
    assert_eq!(state.snapshot_turn_diagnostics(Default::default(), 0).denied_tool_calls, 1);
}

#[test]
fn auto_permission_probe_warning_is_queued_once_until_flushed() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.queue_auto_permission_probe_warning("trusted warning".to_string()));
    assert!(!state.queue_auto_permission_probe_warning("duplicate warning".to_string()));
    assert_eq!(state.take_auto_permission_probe_warning().as_deref(), Some("trusted warning"));
    assert!(state.take_auto_permission_probe_warning().is_none());
}

#[test]
fn tool_wall_clock_exhaustion_policy_violation_message_matches_contract() {
    assert_eq!(
        ToolWallClockExhaustion { max_secs: 600 }.policy_violation_message(),
        "Policy violation: exceeded tool wall clock budget (600s)"
    );
}

#[test]
fn harness_state_treats_zero_tool_budget_as_unlimited() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 0, 10, 1);

    for _ in 0..8 {
        state.record_tool_call();
    }

    assert!(!state.has_tool_call_budget());
    assert!(!state.tool_budget_exhausted());
    assert_eq!(state.tool_budget_exhaustion(), None);
    assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
}

#[test]
fn harness_state_reports_wall_clock_budget_exhaustion() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.wall_clock_budget_exhaustion(), None);
    state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();
    assert_eq!(state.wall_clock_budget_exhaustion(), Some(ToolWallClockExhaustion { max_secs: 10 }));
}

#[test]
fn harness_state_excludes_active_external_wait_from_wall_clock_budget() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);
    let wait_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();
    state.turn_started_at = wait_started_at;
    state.wait_started_at = Some(wait_started_at);

    assert_eq!(state.wall_clock_budget_exhaustion(), None);

    state.end_budget_excluded_wait();
    assert_eq!(state.wall_clock_budget_exhaustion(), None);
}

#[test]
fn harness_state_records_wall_clock_exhaustion_notice_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Not exhausted yet: no notice, no pending directive.
    assert_eq!(state.record_wall_clock_exhaustion_notice(), None);
    assert!(!state.take_wall_clock_directive_pending());

    // Simulate the wall-clock budget elapsing.
    state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();

    // First rejection: first_notice=true and arms the directive.
    assert_eq!(
        state.record_wall_clock_exhaustion_notice(),
        Some(ToolWallClockExhaustionNotice {
            exhaustion: ToolWallClockExhaustion { max_secs: 10 },
            first_notice: true,
        })
    );
    assert!(state.wall_clock_exhausted_emitted);

    // Subsequent rejections in the same batch: first_notice=false.
    assert_eq!(
        state.record_wall_clock_exhaustion_notice(),
        Some(ToolWallClockExhaustionNotice {
            exhaustion: ToolWallClockExhaustion { max_secs: 10 },
            first_notice: false,
        })
    );

    // The directive is consumed exactly once.
    assert!(state.take_wall_clock_directive_pending());
    assert!(!state.take_wall_clock_directive_pending());
}

#[test]
fn tool_wall_clock_exhaustion_directive_messages_match_contract() {
    let exhaustion = ToolWallClockExhaustion { max_secs: 600 };
    assert_eq!(exhaustion.skipped_call_message(), "Tool wall-clock budget exhausted for this turn; call skipped.");
    assert_eq!(
        exhaustion.synthesis_directive_message(),
        "Tool wall-clock budget exhausted for this turn (600s). Tools are disabled for the rest of this turn, so further tool calls are skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation."
    );
}

#[test]
fn harness_state_tracks_blocked_call_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.blocked_tool_calls, 0);
    assert_eq!(state.record_blocked_tool_call(), 1);
    assert_eq!(state.record_blocked_tool_call(), 2);
    assert_eq!(state.blocked_tool_calls, 2);
    state.reset_blocked_tool_call_streak();
    assert_eq!(state.consecutive_blocked_tool_calls, 0);
}

#[test]
fn harness_state_tracks_and_resets_preflight_failure_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.record_preflight_failure(), 1);
    assert_eq!(state.record_preflight_failure(), 2);
    state.reset_preflight_failure_streak();
    assert_eq!(state.consecutive_preflight_failures, 0);
}

#[test]
fn harness_state_tracks_recovery_state() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.is_recovery_active());
    assert!(!state.recovery_pass_used());

    state.activate_recovery("loop detector");
    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_reason(), Some("loop detector"));
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.recovery_is_tool_free());

    assert!(state.consume_recovery_pass());
    assert!(state.recovery_pass_used());
    assert!(state.finish_recovery_pass());
    assert!(!state.is_recovery_active());
}

#[test]
fn harness_state_consumes_recovery_pass_once() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.consume_recovery_pass());

    state.activate_recovery("loop detector");
    assert!(state.consume_recovery_pass());
    assert!(!state.consume_recovery_pass());
    assert!(state.recovery_pass_used());
    assert!(state.finish_recovery_pass());
    assert!(!state.finish_recovery_pass());
}

#[test]
fn harness_state_supports_tool_enabled_recovery_retries() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);

    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
    assert!(!state.recovery_is_tool_free());
    assert!(state.consume_recovery_pass());
    assert!(state.finish_recovery_pass());
}

#[test]
fn harness_state_arms_one_post_tool_compaction_retry() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.arm_post_tool_tool_enabled_retry("transient post-tool failure", false));
    assert!(state.is_recovery_active());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
    assert!(!state.recovery_is_tool_free());
    assert!(state.post_tool_compaction_pending());
    assert!(state.post_tool_tool_enabled_retry_used());
    assert!(!state.post_tool_context_capacity_failure());
    assert!(state.take_post_tool_compaction_pending());
    assert!(!state.post_tool_compaction_pending());
    assert!(!state.arm_post_tool_tool_enabled_retry("must remain bounded", false));
}

#[test]
fn harness_state_marks_context_capacity_recovery() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(state.arm_post_tool_tool_enabled_retry("context limit", true));
    assert!(state.post_tool_context_capacity_failure());
    assert!(!state.post_tool_context_compaction_failed());

    state.mark_post_tool_context_compaction_failed();
    assert!(state.post_tool_context_compaction_failed());
}

#[test]
fn harness_state_switch_to_tool_free_recovery_from_inactive() {
    // Regression guard for the post-tool follow-up infinite loop:
    // `switch_to_tool_free_recovery` must transition `Inactive -> Pending`
    // (not just `InPass` or `Completed` to `Pending`). When a normal
    // (non-recovery)
    // turn's follow-up LLM phase fails, the phase is `Inactive`; if the
    // switch left it there, `consume_recovery_pass()` would return false,
    // `tool_free_recovery` would evaluate to false, and tools would never
    // be disabled at the API level.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Fresh state: recovery is inactive.
    assert!(!state.is_recovery_active());
    assert!(!state.recovery_is_tool_free());

    // Switching from Inactive must engage a tool-free recovery pass.
    assert!(state.switch_to_tool_free_recovery(), "switch from Inactive must report a phase change");
    assert!(state.is_recovery_active(), "phase must be Pending");
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.recovery_is_tool_free());

    // The pass must be consumable. This is what the turn loop checks to
    // decide `tool_free_recovery = true` and disable tools at the API level.
    assert!(state.consume_recovery_pass(), "consume_recovery_pass must succeed after switch from Inactive");

    // A default recovery reason must be seeded so the [Recovery Mode]
    // request block reports why recovery was engaged.
    assert!(state.recovery_reason().is_some(), "recovery_reason must be seeded when switching from Inactive");
}

#[test]
fn harness_state_switch_to_tool_free_recovery_from_in_pass_keeps_consumable() {
    // Switching from InPass (a pass already in flight) must still reset to
    // Pending so the next loop iteration can consume it.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(state.consume_recovery_pass()); // -> InPass
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));

    assert!(state.switch_to_tool_free_recovery());
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.consume_recovery_pass(), "pass must be consumable again after switching from InPass");
}

#[test]
fn harness_state_switch_to_tool_free_recovery_from_completed_keeps_consumable() {
    // No-regression guard: switching from Completed must reset to Pending.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(state.consume_recovery_pass()); // -> InPass
    assert!(state.finish_recovery_pass()); // -> Completed
    assert!(!state.is_recovery_active());

    assert!(state.switch_to_tool_free_recovery());
    assert!(state.is_recovery_active());
    assert!(state.consume_recovery_pass());
}

#[test]
fn harness_state_switch_to_tool_free_recovery_idempotent_when_pending() {
    // When already Pending, switching reports no phase change but still
    // forces the mode to ToolFreeSynthesis.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.activate_recovery("loop detector");
    assert!(state.is_recovery_active()); // Pending

    assert!(!state.switch_to_tool_free_recovery(), "switch from Pending must report no phase change");
    assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
    assert!(state.consume_recovery_pass());
}

#[test]
fn harness_state_switch_to_tool_free_recovery_resets_retry_count_from_inactive() {
    // Switching from Inactive resets the retry counter (mirrors
    // activate_recovery_with_mode) so any stale count does not
    // prematurely exhaust the in-pass retry budget on the new pass.
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    // Fresh state: retry_count is 0, phase is Inactive.
    assert_eq!(state.recovery_retry_count(), 0);

    // Switch from Inactive to Pending: retry count stays 0.
    state.switch_to_tool_free_recovery();
    assert_eq!(state.recovery_retry_count(), 0);

    // Complete this pass and start a second cycle.
    assert!(state.consume_recovery_pass());
    assert!(state.retry_recovery_pass()); // retry_count becomes 1
    assert_eq!(state.recovery_retry_count(), 1);

    // Switch from Completed to Pending: retry count is not reset
    // (only Inactive triggers the reset).
    assert!(state.consume_recovery_pass());
    assert!(state.finish_recovery_pass());
    state.switch_to_tool_free_recovery();
    assert_eq!(state.recovery_retry_count(), 1, "retry count must NOT be reset when switching from Completed");

    // Consume and retry: the budget should tick up to 2, not start over.
    assert!(state.consume_recovery_pass());
    assert!(state.retry_recovery_pass());
    assert_eq!(state.recovery_retry_count(), 2);
}

#[test]
fn harness_state_tracks_post_tool_recovery_cycles() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.post_tool_recovery_cycles(), 0);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 1);
    assert_eq!(state.post_tool_recovery_cycles(), 1);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 2);
    assert_eq!(state.post_tool_recovery_cycles(), 2);
    assert_eq!(state.increment_post_tool_recovery_cycle(), 3);
    assert_eq!(state.post_tool_recovery_cycles(), 3);
}

#[test]
fn harness_state_tracks_task_tracker_create_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(
        state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        )
    );
    assert!(
        !state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        )
    );
    assert!(
        state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"y\"]}".to_string()
        )
    );
}

#[test]
fn harness_state_tracks_successful_readonly_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert!(!state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
    assert!(state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
    assert!(state.has_successful_readonly_signature("file_operation:ro:len10-fnv1234"));
    assert!(!state.record_successful_readonly_signature("file_operation:ro:len10-fnv1234".to_string()));
}

#[test]
fn harness_state_tracks_identical_shell_command_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 1);
    assert_eq!(state.record_shell_command_run("exec_command::cargo check".to_string()), 2);
    assert_eq!(state.record_shell_command_run("exec_command::cargo test".to_string()), 1);
    assert_eq!(state.last_shell_command_signature.as_deref(), Some("exec_command::cargo test"));
}

#[test]
fn harness_state_resets_shell_command_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    state.record_shell_command_run("exec_command::cargo check".to_string());
    state.reset_shell_command_run_streak();
    assert_eq!(state.consecutive_same_shell_command_runs, 0);
    assert!(state.last_shell_command_signature.is_none());
}

#[test]
fn harness_state_tracks_file_read_family_streak() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);

    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 1);
    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/lib.rs".to_string()), 2);
    assert_eq!(state.record_file_read_family_call("apply_patch::read::src/main.rs".to_string()), 1);

    state.reset_file_read_family_streak();
    assert_eq!(state.consecutive_same_file_read_family_calls, 0);
}

#[test]
fn file_read_path_counts_track_per_path_regardless_of_slice() {
    let mut state =
        HarnessTurnState::new(TurnRunId("run-path".to_string()), TurnId("turn-path".to_string()), 20, 600, 3);

    // Same path, different offsets — each increments the path counter.
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 2);
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 3);
    // Different path gets its own counter.
    assert_eq!(state.record_file_read_path_call("src/main.rs".to_string()), 1);
    // Original path continues counting.
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 4);

    state.reset_file_read_path_counts();
    assert_eq!(state.record_file_read_path_call("src/lib.rs".to_string()), 1);
}

#[test]
fn session_limit_grant_is_recorded_once_and_exposed_for_cleanup() {
    let mut state =
        HarnessTurnState::new(TurnRunId("run-limit".to_string()), TurnId("turn-limit".to_string()), 20, 600, 3);

    assert!(!state.has_session_limit_grant());
    assert!(!state.take_session_limit_grant_directive_pending());

    state.record_session_limit_grant();

    assert!(state.has_session_limit_grant());
    assert!(state.take_session_limit_grant_directive_pending());
    assert!(!state.take_session_limit_grant_directive_pending());
}

#[test]
fn harness_state_builds_execution_snapshot() {
    let mut state = HarnessTurnState::new(TurnRunId("run-9".to_string()), TurnId("turn-3".to_string()), 6, 120, 2);
    state.set_phase(TurnPhase::ExecutingTools);

    let snapshot = state.execution_snapshot();
    assert_eq!(snapshot.run_id, "run-9");
    assert_eq!(snapshot.turn_id, "turn-3");
    assert_eq!(snapshot.phase, TurnPhase::ExecutingTools);
    assert_eq!(snapshot.max_tool_calls, 6);
    assert_eq!(snapshot.max_tool_wall_clock_secs, 120);
    assert_eq!(snapshot.max_tool_retries, 2);
}

#[test]
fn turn_diagnostic_counters_saturate_and_count_recovery_once() {
    let mut state = HarnessTurnState::new(
        TurnRunId("run-diagnostics".to_string()),
        TurnId("turn-diagnostics".to_string()),
        4,
        120,
        2,
    );
    state.requested_tool_calls = u32::MAX - 1;
    state.record_requested_tool_calls(usize::MAX);
    state.raw_spooled_bytes = u64::MAX - 2;
    state.model_visible_output_bytes = u64::MAX - 2;
    state.record_tool_output_metrics(true, true, 10, usize::MAX);
    state.record_reused_result();
    state.activate_recovery("adaptive planning synthesis");
    state.activate_recovery("duplicate activation");

    let diagnostics = state.snapshot_turn_diagnostics(Default::default(), u32::MAX);
    assert_eq!(diagnostics.requested_tool_calls, u32::MAX);
    assert_eq!(diagnostics.unadmitted_tool_calls, u32::MAX);
    assert_eq!(diagnostics.reused_results, 2);
    assert_eq!(diagnostics.spooled_results, 1);
    assert_eq!(diagnostics.raw_spooled_bytes, u64::MAX);
    assert_eq!(diagnostics.model_visible_output_bytes, u64::MAX);
    assert_eq!(diagnostics.low_signal_tool_calls, u32::MAX);
    assert_eq!(diagnostics.recovery_activations, 1);
}

// --- CrossTurnTracker tests ---

#[test]
fn cross_turn_tracker_no_warning_on_first_turn() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
    let written = HashSet::new();
    assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());
}

#[test]
fn cross_turn_tracker_detects_repeated_turn() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
    let written = HashSet::new();

    // First turn: no warning
    assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());

    // Second turn with same signatures: cross-turn loop detected
    let warning = tracker.seal_turn(&read_sigs, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Cross-turn loop detected"));
}

#[test]
fn cross_turn_tracker_no_false_positive_different_turns() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs_1 = vec!["apply_patch::read::src/main.rs".to_string()];
    let read_sigs_2 = vec!["apply_patch::read::src/lib.rs".to_string()];
    let written = HashSet::new();

    assert!(tracker.seal_turn(&read_sigs_1, &written, None, false).is_none());
    assert!(tracker.seal_turn(&read_sigs_2, &written, None, false).is_none());
}

#[test]
fn cross_turn_tracker_stuck_no_progress() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    // Use different signatures each turn to avoid cross-turn loop detection
    // and isolate the stuck (zero-mutation) detection.
    let sigs_1 = vec!["apply_patch::read::src/a.rs".to_string()];
    let sigs_2 = vec!["apply_patch::read::src/b.rs".to_string()];
    let sigs_3 = vec!["apply_patch::read::src/c.rs".to_string()];

    assert!(tracker.seal_turn(&sigs_1, &written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_2, &written, None, false).is_none());

    // Third consecutive read-only turn: stuck warning
    let warning = tracker.seal_turn(&sigs_3, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("No progress detected"));
}

#[test]
fn cross_turn_tracker_mutation_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let empty_written = HashSet::new();

    // Two read-only turns with different signatures (avoid cross-turn loop)
    let sigs_a = vec!["apply_patch::read::src/a.rs".to_string()];
    let sigs_b = vec!["apply_patch::read::src/b.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_a, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_b, &empty_written, None, false).is_none());

    // A mutating turn resets the counter
    let mut written = HashSet::new();
    written.insert("src/main.rs".to_string());
    let sigs_c = vec!["apply_patch::read::src/c.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_c, &written, None, false).is_none());

    // Two more read-only turns: no stuck warning (counter was reset)
    let sigs_d = vec!["apply_patch::read::src/d.rs".to_string()];
    let sigs_e = vec!["apply_patch::read::src/e.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_d, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_e, &empty_written, None, false).is_none());
}

#[test]
fn cross_turn_tracker_command_execution_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    assert!(tracker.seal_turn(&["read::a".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::b".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&[], &written, Some("exec::cargo-check"), false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 0);

    assert!(tracker.seal_turn(&["read::c".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::d".to_string()], &written, None, false).is_none());
}

#[test]
fn cross_turn_tracker_out_of_band_progress_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    assert!(tracker.seal_turn(&["read::a".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::b".to_string()], &written, None, false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 2);

    assert!(
        tracker
            .seal_turn_with_progress(&[], &written, None, None, true, false)
            .is_none()
    );
    assert_eq!(tracker.zero_mutation_turns(), 0);

    assert!(tracker.seal_turn(&["read::c".to_string()], &written, None, false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 1);
}

#[test]
fn harness_state_separates_guarded_and_admitted_shell_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

    state.record_shell_command_run("exec_command::blocked".to_string());
    assert_eq!(state.last_admitted_shell_command_signature, None);

    state.record_admitted_shell_command("exec_command::admitted".to_string());
    assert_eq!(state.last_admitted_shell_command_signature.as_deref(), Some("exec_command::admitted"));
}

#[test]
fn harness_state_records_failed_shell_key() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);
    assert_eq!(state.last_failed_shell_key(), None);

    state.record_failed_shell_command("command_session::cargo check".to_string(), "error: bad manifest".to_string());
    assert_eq!(state.last_failed_shell_key(), Some("command_session::cargo check::err::error: bad manifest"));
}

#[test]
fn cross_turn_tracker_empty_turn_no_warning() {
    let mut tracker = CrossTurnTracker::new();
    let empty_sigs: Vec<String> = Vec::new();
    let empty_written = HashSet::new();

    // Empty turns should not trigger warnings or corrupt state
    assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
}

#[test]
fn cross_turn_tracker_identical_shell_failure_warns_on_third_turn() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();
    let key = Some("command_session::cargo check::err::error: failed to load manifest");

    // Vary the read sets so the fingerprint loop detector stays quiet;
    // the identical-failure streak must fire regardless.
    let sigs_a = vec!["code_search::alpha".to_string()];
    let sigs_b = vec!["code_search::beta".to_string()];
    let sigs_c = vec!["code_search::gamma".to_string()];
    assert!(
        tracker
            .seal_turn_with_progress(&sigs_a, &written, None, key, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&sigs_b, &written, None, key, false, false)
            .is_none()
    );
    let warning = tracker.seal_turn_with_progress(&sigs_c, &written, None, key, false, false);
    assert!(warning.is_some());
    let warning = warning.unwrap();
    assert!(warning.contains("Identical shell failure"), "warning names the pattern: {warning}");
    assert!(warning.contains("cargo check"), "warning names the command: {warning}");
}

#[test]
fn cross_turn_tracker_failed_shell_streak_resets_on_change_or_success() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();
    let empty: Vec<String> = Vec::new();
    let key_a = Some("command_session::cargo check::err::error: bad manifest");
    let key_b = Some("command_session::cargo check::err::error: missing crate");

    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_a, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_a, false, false)
            .is_none()
    );
    // Changed error text restarts the streak: no warning on what would
    // otherwise be the third consecutive failure turn.
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    // A clean turn clears the streak entirely.
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, None, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    let warning = tracker.seal_turn_with_progress(&empty, &written, None, key_b, false, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Identical shell failure"));
}

#[test]
fn cross_turn_tracker_order_independent_fingerprint() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    // Same signatures in different order should produce same fingerprint
    let sigs_a = vec![
        "apply_patch::read::src/main.rs".to_string(),
        "code_search::grep::fn".to_string(),
    ];
    let sigs_b = vec![
        "code_search::grep::fn".to_string(),
        "apply_patch::read::src/main.rs".to_string(),
    ];

    assert!(tracker.seal_turn(&sigs_a, &written, None, false).is_none());
    let warning = tracker.seal_turn(&sigs_b, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Cross-turn loop detected"));
}

#[test]
fn full_auto_loop_grants_require_runtime_and_config_and_opt_in() {
    assert_eq!(SESSION_LIMIT_AUTO_GRANT_INCREMENT, 100);

    // No config at all: interactive sessions keep prompting.
    assert!(!full_auto_loop_grants_enabled(true, None));
    assert!(!full_auto_loop_grants_enabled(false, None));

    let mut cfg = VTCodeConfig::default();
    // Default config has full-auto disabled: no auto-grant either way.
    assert!(!full_auto_loop_grants_enabled(true, Some(&cfg)));
    assert!(!full_auto_loop_grants_enabled(false, Some(&cfg)));

    // Full-auto runtime + enabled config grants by default (opt-out flag on).
    cfg.automation.full_auto.enabled = true;
    assert!(full_auto_loop_grants_enabled(true, Some(&cfg)));
    // Ordinary runtime never grants, even with full-auto configured.
    assert!(!full_auto_loop_grants_enabled(false, Some(&cfg)));

    // Explicit opt-out restores prompting in full-auto runs.
    cfg.automation.full_auto.auto_grant_tool_limits = false;
    assert!(!full_auto_loop_grants_enabled(true, Some(&cfg)));
}

#[test]
fn failure_diagnosis_memo_round_trips_and_caps_model_calls() {
    let mut state = HarnessTurnState::new(TurnRunId("r".into()), TurnId("t".into()), 4, 60, 1);
    let key = DiagnosisMemoKey {
        tool: CompactStr::from("exec_command"),
        evidence: "exit 1".to_string(),
    };
    assert!(state.failure_diagnosis_memo_get(&key).is_none());
    state.failure_diagnosis_memo_put(
        key.clone(),
        DiagnosisMemoEntry {
            observed: CompactStr::from("obs"),
            likely_cause: CompactStr::from("cause"),
            next_action: CompactStr::from("act"),
        },
    );
    let hit = state.failure_diagnosis_memo_get(&key).expect("memo hit");
    assert_eq!(hit.observed.as_str(), "obs");
    assert_eq!(hit.likely_cause.as_str(), "cause");
    assert_eq!(hit.next_action.as_str(), "act");

    assert!(state.can_spend_failure_diagnosis_model_call());
    for _ in 0..3 {
        state.record_failure_diagnosis_model_call();
    }
    assert!(!state.can_spend_failure_diagnosis_model_call(), "per-turn model diagnosis budget must stop at 3");
}

#[test]
fn auto_permission_probe_budget_stops_model_calls_per_turn() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);

    assert!(state.can_spend_auto_permission_probe_model_call());
    for _ in 0..3 {
        state.record_auto_permission_probe_model_call();
    }
    assert!(
        !state.can_spend_auto_permission_probe_model_call(),
        "per-turn prompt-injection probe budget must stop at 3"
    );
}
