use super::*;

#[tokio::test]
async fn oversized_preview_does_not_trigger_synthesis_outside_planning() {
    use vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES;

    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    ctx.push_tool_response("call-blind", Some("exec_command"), "x".repeat(TURN_PREVIEW_BUDGET_BYTES + 1));
    assert!(!ctx.harness_state.model_visible_preview_budget_exhausted());

    let mut tracker = LoopTracker::new();
    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn build_listing_loop_converges_on_third_same_base_scan() {
    // Rehearsal of the observed build-mode listing loop (`ls src/`,
    // `find …`, `ls src`, `ls -1 src`, …): argument variations keep every
    // exact-request counter quiet, so the third same-base listing must
    // schedule recovery instead of burning the turn tool-call budget.
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout": "src\nsrc-tauri"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for command in ["ls src/", "find src -maxdepth 1 -type d", "ls src"] {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": command, "command": command, "action": "run"}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 2);
    assert_eq!(tracker.repeated_navigation_count(), 0);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());

    update_repetition_tracker(
        &mut tracker,
        &success,
        tool_names::EXEC_COMMAND,
        &json!({"cmd": "ls -1 src", "command": "ls -1 src", "action": "run"}),
    );
    assert_eq!(tracker.max_coarse_listing_count(), 3);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("summarize only from collected evidence") })
    );
}
#[tokio::test]
async fn build_search_triplets_do_not_trip_listing_convergence() {
    // Distinct `rg` queries are semantically distinct questions, so three
    // of them must not schedule recovery. They carry no coarse inspection
    // family (`rg`/`grep` are excluded from coarse grouping because their
    // first positional is the search pattern, not a path root).
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout": "src/lib.rs:1:hit"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for query in ["rg -n 'alpha' src/", "rg -n 'beta' src/", "rg -n 'gamma' src/"] {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": query, "command": query, "action": "run"}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 0);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn same_pattern_search_repeats_do_not_trip_early_recovery() {
    // Regression for turn_1303/turn_1304 (`exec::inspection::grep::enum ×5`)
    // and turn_1291 (`exec::inspection::rg::pub ×5`): five distinct
    // successful searches sharing one pattern across different files/flags
    // are legitimate research. With `rg`/`grep` excluded from coarse
    // grouping they must not enter the low-signal ledger and must not
    // schedule early recovery.
    let mut backing = TestTurnProcessingBacking::new(120).await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout": "src/lib.rs:1:hit"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for query in [
        "rg -n 'pub enum Commands' src/ -A 40",
        "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 50",
        "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 600",
        "rg -n 'pub enum Provider|Gemini|OpenAI' crates/codegen/vtcode-llm/src",
        "rg -n 'pub enum SecretCommand|Add|List' crates/codegen/vtcode-core/src/cli/args/secret.rs",
    ] {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": query, "command": query, "action": "run"}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 0);
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.dominant_churn(), None);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
}
