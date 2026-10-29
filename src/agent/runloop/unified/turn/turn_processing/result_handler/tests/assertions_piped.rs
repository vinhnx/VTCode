use super::*;

#[tokio::test]
async fn incorrect_generated_assertion_requires_explicit_correction_and_fresh_execution() {
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let readme = backing.workspace_path().join("README.md");
    let content = "<summary>View all contributors (<!-- CONTRIBUTORS-COUNT:START -->28<!-- CONTRIBUTORS-COUNT:END -->)</summary>\n";
    std::fs::write(&readme, content).unwrap();
    let wrong = "rg -o 'View all contributors \\([0-9]+\\)' README.md";
    let corrected = "rg -q 'View all contributors \\(<!-- CONTRIBUTORS-COUNT:START -->[0-9]+<!-- CONTRIBUTORS-COUNT:END -->\\)' README.md";
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        let mut commands = ctx.tool_registry.commands_config();
        commands.allow_list.push("rg".into());
        ctx.tool_registry.apply_commands_config(&commands);
        let result = ctx.tool_registry.execute_tool("task_tracker", serde_json::json!({
            "action":"create", "items":[{"description":"Display contributor count", "status":"completed", "verify":[wrong]}]
        })).await.unwrap();
        assert!(result.get("error").is_none(), "{result}");
        ctx.tool_registry.restore_completion_verification_debt();
    }
    tracker.mark_verification_pending();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "exec_command",
        &serde_json::json!({"cmd":wrong}).to_string(),
    )
    .await;
    {
        let ctx = backing.turn_processing_context();
        assert!(ctx.tool_registry.completion_verification_pending());
        assert!(!ctx.tool_registry.completion_verifier_passed(wrong));
        assert!(ctx.tool_registry.unverified_completion_reason().contains(wrong));
        assert!(
            ctx.working_history
                .iter()
                .any(|message| message.content.as_text().contains("original request and observed artifact")),
            "history: {:?}",
            ctx.working_history
        );
    }
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "task_tracker",
        &serde_json::json!({"action":"update", "index":1, "status":"completed", "verify":[corrected]}).to_string(),
    )
    .await;
    {
        let ctx = backing.turn_processing_context();
        assert!(ctx.tool_registry.completion_verification_pending());
        assert!(ctx.tool_registry.unverified_completion_reason().contains(corrected));
    }
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "exec_command",
        &serde_json::json!({"cmd":corrected}).to_string(),
    )
    .await;
    assert!(
        !backing
            .turn_processing_context()
            .tool_registry
            .completion_verification_pending()
    );
    assert_eq!(std::fs::read_to_string(&readme).unwrap(), content);
    // The corrected assertion must still reject the missing-count boundary.
    std::fs::write(&readme, "<summary>View all contributors</summary>\n").unwrap();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "exec_command",
        &serde_json::json!({"cmd":corrected}).to_string(),
    )
    .await;
    assert!(
        backing
            .turn_processing_context()
            .tool_registry
            .completion_verification_pending()
    );
}
#[tokio::test]
async fn piped_verifier_executes_standalone_with_truthful_status() {
    // End-to-end proof of the root fix: the model types a piped verifier,
    // the kernel elides the truncator, and the gate follows the VERIFIER's
    // real exit status — not the tail's. `rustc --version | head -c 5`
    // would report exit 0 with 5 chars of output under pipeline semantics;
    // rewritten, the full version surfaces and the gate clears.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    assert!(repeated_tool_attempts.verification_is_pending());
    let mut turn_modified_files = BTreeSet::new();

    drive_model_tool_call(
        &mut backing,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
        "exec_command",
        r#"{"cmd": "rustc --version | head -c 5"}"#,
    )
    .await;

    assert!(
        !repeated_tool_attempts.verification_is_pending(),
        "truthful exit 0 from the elided verifier must clear the gate"
    );
    assert_eq!(repeated_tool_attempts.consecutive_mutations, 0);
}
#[tokio::test]
async fn piped_verifier_failure_surfaces_instead_of_tail_success() {
    // Converse proof: `rustc --invalid-flag-xyz | tail -5` exits 0 as a
    // pipeline (tail's status) while the verifier fails. Rewritten, the
    // failure surfaces: gate stays pending with the fix window granted.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    drive_model_tool_call(
        &mut backing,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
        "exec_command",
        r#"{"cmd": "rustc --invalid-flag-xyz | tail -5"}"#,
    )
    .await;

    assert!(
        repeated_tool_attempts.verification_is_pending(),
        "truthful verifier failure must keep the gate pending"
    );
}
#[test]
fn auto_verification_attribution_ignores_prior_turn_responses() {
    use super::super::last_tool_response_text;

    let history = vec![
        uni::Message::user("do the work".to_string()),
        uni::Message::tool_response("harness-auto-verify".to_string(), "stale failure".to_string()),
        uni::Message::assistant("commentary".to_string()),
    ];
    // Window starting after the stale response: nothing attributable, so
    // a dispatch that broke before executing records no phantom failure.
    assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 3), None);
    // Out-of-range starts are fail-safe, never panics.
    assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 99), None);
    // Window covering it: attributed normally.
    assert_eq!(last_tool_response_text(&history, "harness-auto-verify", 0).as_deref(), Some("stale failure"));
    // Wrong call id: never attributed.
    assert_eq!(last_tool_response_text(&history, "call-other", 0), None);
}
#[tokio::test]
async fn completion_claim_jumps_straight_to_harness_verification() {
    // A "done" assertion on the FIRST pending text must not spend
    // directive rounds: the harness verifies immediately.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --version");
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    let mut ctx = backing.turn_processing_context();
    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: "The change is complete.".to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 8,
        tool_repeat_limit: 4,
    })
    .await
    .expect("completion claim should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!repeated_tool_attempts.verification_is_pending(), "executed `rustc --version` must clear the gate");
    assert_eq!(
        repeated_tool_attempts.verification_auto_recovery_attempts(),
        0,
        "fast path must not consume directive budget"
    );
    assert!(!repeated_tool_attempts.auto_verification_executed, "success clears the one-shot flag with the gate");
}
#[tokio::test]
async fn harness_auto_execute_disabled_falls_back_to_block() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
    vt_cfg.agent.harness.verification.auto_execute = false;
    vt_cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".to_string());
    backing.set_vt_cfg_for_test(vt_cfg);
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
        "disabled auto-execute must preserve the manual blocked handoff"
    );
    assert!(!repeated_tool_attempts.auto_verification_executed);
    assert!(repeated_tool_attempts.verification_is_pending());
}
#[tokio::test]
async fn harness_auto_verification_escalated_suite_blocks_without_executing() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --version");
    {
        let ctx = backing.turn_processing_context();
        for _ in 0..3 {
            ctx.session_stats
                .record_verification_auto_failure("rustc --version".to_string(), "boom");
        }
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 3);
    }
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })),
        "an escalated never-passing suite must block instead of executing again"
    );
    assert!(!repeated_tool_attempts.auto_verification_executed);
}
