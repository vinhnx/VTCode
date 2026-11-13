use super::*;

#[tokio::test]
async fn many_productive_planning_previews_do_not_force_recovery() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    for index in 0..24 {
        ctx.push_tool_response(
            format!("call-{index}"),
            Some("exec_command"),
            format!("evidence-{index} {}", "x".repeat(8_000)),
        );
    }
    let mut tracker = LoopTracker::new();
    let result = super::super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, usize::MAX).await;
    assert!(matches!(result, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
    assert!(!tracker.planning_low_signal_synthesis_triggered);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("evidence-23"))
    );
    assert!(
        !ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("preview_budget_exhausted"))
    );
}
#[tokio::test]
async fn planning_one_repeated_successful_inspection_remains_productive() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout":"useful source"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let calls = [
        ("ls -la src", None),
        ("cat src/main.rs", None),
        ("sed -n '80,140p' src/lib.rs", None),
        ("cat src/startup.rs", None),
        ("ls -la src/startup", None),
        ("find src -name 'startup*' -o -name 'main_helpers*'", None),
        ("rg -n 'resolve_startup_context' src/", None),
        ("sed -n '100,220p' src/main_helpers/bootstrap.rs", None),
        ("rg -n 'from_cli_args' src/startup/mod.rs", None),
        ("sed -n '278,420p' src/startup/mod.rs", None),
        ("sed -n '278,340p' src/startup/mod.rs", None),
        ("sed -n '278,420p' src/startup/mod.rs", Some(8000)),
    ];

    for (command, max_output_tokens) in calls {
        let mut args = json!({"cmd": command, "command": command, "action": "run"});
        if let Some(max_output_tokens) = max_output_tokens {
            args["max_output_tokens"] = json!(max_output_tokens);
        }
        update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &args);
    }

    assert_eq!(tracker.consecutive_navigations, calls.len());
    assert_eq!(tracker.repeated_navigation_count(), 1);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, 3).await;

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_navigations, calls.len());
    assert!(!tracker.planning_low_signal_synthesis_triggered);
}
#[tokio::test]
async fn planning_diverse_inspection_remains_productive() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout":"useful source"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let commands = [
        "ls -la src",
        "cat src/main.rs",
        "sed -n '80,140p' src/lib.rs",
        "cat src/startup.rs",
        "ls -la src/startup",
        "find src -name 'startup*' -o -name 'main_helpers*'",
        "rg -n 'resolve_startup_context' src/",
        "sed -n '100,220p' src/main_helpers/bootstrap.rs",
        "rg -n 'from_cli_args' src/startup/mod.rs",
        "sed -n '278,420p' src/startup/mod.rs",
        "sed -n '278,340p' src/startup/mod.rs",
        "cat src/startup/mod.rs",
    ];

    for command in commands {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": command, "command": command, "action": "run"}),
        );
    }

    assert_eq!(tracker.consecutive_navigations, commands.len());
    assert_eq!(tracker.repeated_navigation_count(), 0);

    let outcome = super::super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, 3).await;

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_navigations, commands.len());
}
#[tokio::test]
async fn planning_genuine_repeated_navigation_still_recovers() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout":"useful source"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for index in 0..NAVIGATION_LOOP_THRESHOLD {
        let command = format!("cat src/{}.rs", index % 4);
        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": command, "command": command, "action": "run"}),
        );
    }
    assert!(tracker.repeated_navigation_count() >= 3);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);

    let outcome = super::super::handle_turn_balancer(&mut ctx, NAVIGATION_LOOP_THRESHOLD, &mut tracker, 120, 3).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.is_recovery_active());
    assert_eq!(tracker.consecutive_navigations, 0);
}
