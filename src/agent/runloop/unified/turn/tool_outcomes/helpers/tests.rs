use serde_json::json;
use vtcode_core::config::constants::tools;

mod verification;
mod verification_recovery;
mod verifier_sessions;

#[test]
fn verifier_wait_window_grows_and_resets_with_the_session() {
    let mut tracker = LoopTracker::new();

    // The window doubles up to the cap, so a long verifier needs a handful of
    // polls instead of one per second.
    let windows: Vec<u64> = (0..6).map(|_| tracker.next_verifier_wait_timeout_secs()).collect();
    assert_eq!(windows, vec![1, 2, 4, 8, 8, 8]);
    assert!(windows.windows(2).all(|pair| pair[1] >= pair[0]), "the wait window must never shrink: {windows:?}");

    // A new verifier session starts a fresh window.
    tracker.reset_verifier_wait_polls();
    assert_eq!(tracker.next_verifier_wait_timeout_secs(), 1);
}

#[test]
fn unrelated_successful_checks_cannot_refill_required_verifier_progress() {
    let mut tracker = LoopTracker::new();
    for attempt in 1..=BLIND_EDITING_THRESHOLD {
        tracker.mark_verification_complete();
        assert_eq!(
            tracker.record_required_verifier_progress(Some("cargo clippy".into())),
            attempt == BLIND_EDITING_THRESHOLD
        );
    }
    assert!(
        !tracker.record_required_verifier_progress(Some("cargo check".into())),
        "advancing to another check starts a new window"
    );
    tracker.record_successful_mutation();
    assert!(
        !tracker.record_required_verifier_progress(Some("cargo check".into())),
        "real edits start a new verification generation"
    );
}

#[test]
fn tracker_observations_cannot_reset_navigation_or_verification_progress() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 1));
    let output = json!({"status":"ok", "checklist":{"items":[{"index":3,"status":"in_progress"}]}});
    let observed = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for budget in [1000, 2000] {
        update_repetition_tracker(
            &mut tracker,
            &observed,
            tools::TASK_TRACKER,
            &json!({"action":"list","max_output_tokens":budget}),
        );
    }
    assert!(tracker.tracker_observations_stalled());
    assert_eq!(tracker.consecutive_navigations, 2);
    assert_eq!(tracker.verification_snapshot(), (true, 0));
    let progress = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"status":"updated"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(
        &mut tracker,
        &progress,
        tools::TASK_TRACKER,
        &json!({"action":"update","index":3,"status":"completed"}),
    );
    assert!(!tracker.tracker_observations_stalled());
    assert_eq!(tracker.verification_snapshot(), (true, 0));
}

#[test]
fn rejected_verifier_does_not_grant_repair_edits() {
    for kind in [
        vtcode_core::tools::registry::ToolErrorType::InvalidParameters,
        vtcode_core::tools::registry::ToolErrorType::PermissionDenied,
        vtcode_core::tools::registry::ToolErrorType::PolicyViolation,
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                tools::EXEC_COMMAND,
                kind,
                "verifier rejected",
            ),
        });
        assert!(!update_repetition_tracker(
            &mut tracker,
            &outcome,
            tools::EXEC_COMMAND,
            &json!({"cmd":"cargo check --locked"})
        ));
        assert!(tracker.verification_is_pending());

        assert!(!tracker.take_verification_result_lost_notice());
        assert!(!tracker.take_verifier_failed_notice());
    }
}

#[test]
fn running_verifier_requires_its_own_terminal_session_result() {
    for exit_code in [0, 1] {
        let mut tracker = LoopTracker::new();
        tracker.mark_verification_pending();
        let running = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"session_id": "run-verifier", "lifecycle_state": "running"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &running, tools::EXEC_COMMAND, &json!({"cmd": "cargo check --locked"}));
        assert!(tracker.verification_is_pending());
        assert_eq!(tracker.pending_verifier_session_id.as_deref(), Some("run-verifier"));

        assert!(!tracker.take_verifier_failed_notice(), "running is not failed");

        let terminal = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code": exit_code}),
            stdout: None,
            modified_files: vec![],
            command_success: exit_code == 0,
        });
        update_repetition_tracker(
            &mut tracker,
            &terminal,
            tools::WRITE_STDIN,
            &json!({"session_id": "run-unrelated", "action": "wait"}),
        );
        assert!(tracker.verification_is_pending(), "unrelated completion cannot verify changes");
        assert!(!tracker.take_verifier_failed_notice(), "unrelated completion is not the verifier verdict");
        update_repetition_tracker(
            &mut tracker,
            &terminal,
            tools::WRITE_STDIN,
            &json!({"session_id": " run-verifier ", "action": "wait"}),
        );
        assert_eq!(tracker.verification_is_pending(), exit_code != 0);
        assert!(tracker.pending_verifier_session_id.is_none());
        assert_eq!(tracker.take_verifier_failed_notice(), exit_code != 0);
        assert!(!tracker.take_verifier_failed_notice(), "terminal feedback is consumed once");
    }
}

#[test]
fn session_cleanup_remains_available_while_verification_is_pending() {
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    for action in ["terminate", "close"] {
        let args = json!({"session_id": "run-cleanup", "action": action});

        assert!(vtcode_core::tools::tool_intent::is_turn_budget_exempt_call(tools::WRITE_STDIN, &args));
    }
}

use super::*;

#[test]
fn push_tool_response_replaces_existing_tool_call_entry() {
    let mut history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        "{\"output\":\"first\"}".to_string(),
    )];

    let update = push_tool_response(&mut history, "call_1".to_string(), None, "{\"output\":\"latest\"}".to_string());

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text_borrowed(), Some("{\"output\":\"latest\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Replaced { previous_text_len: "{\"output\":\"first\"}".len() });
}

#[test]
fn push_tool_response_sets_origin_tool_when_provided() {
    let mut history = Vec::new();

    let update =
        push_tool_response(&mut history, "call_1".to_string(), Some("read_file"), "{\"output\":\"first\"}".to_string());

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].origin_tool.as_deref(), Some("read_file"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}

#[test]
fn push_tool_response_refreshes_origin_tool_when_replacing_same_call() {
    let mut history = vec![uni::Message::tool_response("call_1".to_string(), "old".to_string())];

    let update = push_tool_response(&mut history, "call_1".to_string(), Some("exec_command"), "new".to_string());

    assert_eq!(update, ToolResponseHistoryUpdate::Replaced { previous_text_len: 3 });
    assert_eq!(history[0].origin_tool.as_deref(), Some("exec_command"));
}

#[test]
fn push_tool_response_appends_when_id_reused_across_assistant_boundary() {
    // Fabricated ids can collide across turns (e.g. index-based fallbacks).
    // A later assistant message re-declaring the same id must not cause a
    // new result to clobber the earlier, unrelated Tool response.
    let mut history = vec![
        uni::Message::assistant_with_tools(
            "first".into(),
            vec![uni::ToolCall::function(
                "call_1".into(),
                "file_operation".into(),
                "{}".into(),
            )],
        ),
        uni::Message::tool_response("call_1".to_string(), "{\"output\":\"first\"}".into()),
        uni::Message::assistant_with_tools(
            "second".into(),
            vec![uni::ToolCall::function(
                "call_1".into(),
                tools::CODE_SEARCH.into(),
                "{}".into(),
            )],
        ),
    ];

    let update = push_tool_response(
        &mut history,
        "call_1".to_string(),
        Some(tools::CODE_SEARCH),
        "{\"output\":\"second\"}".to_string(),
    );

    let tool_messages: Vec<&uni::Message> = history
        .iter()
        .filter(|message| matches!(message.role, uni::MessageRole::Tool))
        .collect();
    assert_eq!(tool_messages.len(), 2, "must append, not overwrite");
    assert_eq!(
        tool_messages[0].content.as_text_borrowed(),
        Some("{\"output\":\"first\"}"),
        "earlier unrelated Tool result must remain intact"
    );
    assert_eq!(tool_messages[1].content.as_text_borrowed(), Some("{\"output\":\"second\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}

#[test]
fn push_tool_response_appends_when_assistant_has_no_tool_calls() {
    // When an Assistant message has no tool_calls (e.g. commentary-only
    // message between tool calls), the boundary must STILL stop the scan.
    // Otherwise a later Tool response with a colliding fabricated id would
    // overwrite an earlier, unrelated Tool result.
    let mut history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_0".into(),
                "file_operation".into(),
                "{}".into(),
            )],
        ),
        uni::Message::tool_response("call_0".to_string(), "{\"output\":\"file content\"}".into()),
        // Commentary Assistant with no tool_calls — must act as boundary
        uni::Message::assistant("I need to retry.".into()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_0".into(),
                "apply_patch".into(),
                "{}".into(),
            )],
        ),
    ];

    let update = push_tool_response(
        &mut history,
        "call_0".to_string(),
        Some("apply_patch"),
        "{\"output\":\"patch result\"}".to_string(),
    );

    let tool_messages: Vec<&uni::Message> = history
        .iter()
        .filter(|message| matches!(message.role, uni::MessageRole::Tool))
        .collect();
    assert_eq!(tool_messages.len(), 2, "must append, not overwrite the earlier file read");
    assert_eq!(
        tool_messages[0].content.as_text_borrowed(),
        Some("{\"output\":\"file content\"}"),
        "earlier file read result must remain intact"
    );
    assert_eq!(tool_messages[1].content.as_text_borrowed(), Some("{\"output\":\"patch result\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}

#[test]
fn repetition_tracker_counts_failures() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            "edit_file".to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "boom".to_string(),
        ),
    });

    update_repetition_tracker(&mut tracker, &outcome, "edit_file", &json!({"path":"src/main.rs"}));

    assert_eq!(tracker.max_count_filtered(|_| false), 1);
}

#[test]
fn failed_file_mutations_do_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            "apply_patch".to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "invalid patch path".to_string(),
        ),
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::APPLY_PATCH,
        &json!({"input":"*** Begin Patch\n*** Update File: /absolute/path\n*** End Patch"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}

#[test]
fn no_op_write_does_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({
            "success": true,
            "path": "README.md",
            "diff_preview": {
                "content": "",
                "truncated": false,
                "omitted_line_count": 0,
                "skipped": false,
                "is_empty": true
            },
            "diff": [{
                "path": "README.md",
                "content": "",
                "truncated": false,
                "omitted_line_count": 0,
                "skipped": false,
                "is_empty": true
            }]
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::WRITE_FILE,
        &json!({"path":"README.md","content":"same\n","mode":"overwrite"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}

#[test]
fn skipped_write_does_not_trigger_verification_pressure() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({
            "success": true,
            "skipped": true,
            "reason": "File already exists"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::WRITE_FILE,
        &json!({"path":"README.md","content":"same\n","mode":"skip_if_exists"}),
    );

    assert_eq!(tracker.consecutive_mutations, 0);
}

#[test]
fn logged_compound_inspections_do_not_trigger_anti_blind_pressure() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for command in [
        "cat README.md && printf '\\n--- git status ---\\n' && git status --short",
        "wc -l README.md; rg -n '^#' README.md",
        "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
    ] {
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));
    }

    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_navigations, 3);
}

#[test]
fn awk_range_inspections_do_not_trigger_anti_blind_pressure() {
    // Regression for session-vtcode-20260921T023834Z: six consecutive
    // read-only `awk` page reads tripped the blind-editing gate because
    // `awk` was missing from the read-only allow-list.
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::EXEC_COMMAND,
            &json!({"cmd": "awk 'NR>=297 && NR<=312' README.md"}),
        );
    }

    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
}

#[test]
fn awk_write_primitives_still_trigger_anti_blind_pressure() {
    // Asymmetric counterpart: real writes (including gawk `@` indirect
    // calls) must still count as mutations so the fix cannot
    // over-correct into a fail-open.
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::EXEC_COMMAND,
            &json!({"cmd": "awk -v f=system 'BEGIN{@f(\"id\")}' README.md"}),
        );
    }

    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    assert!(!tracker.verification_is_pending(), "mutation debt belongs to the registry");
}

#[cfg(unix)]
#[test]
fn logged_compound_inspection_with_unix_stderr_suppression_does_not_trigger_pressure() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let command = r###"git diff --stat; find docs -maxdepth 2 -type f | sort | head -40; rg -n "vtcode init|vtcode models|full-auto|run-debug|cargo install" docs/user-guide docs/installation docs/development 2>/dev/null | head -50"###;
    update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));

    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 1);
}

#[test]
fn only_a_completed_verification_clears_pending_mutation_pressure() {
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let edit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(&mut tracker, &edit, tools::EDIT_FILE, &json!({"path":"src/lib.rs"}));
    }
    assert!(tracker.verification_is_pending());

    let failed_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::EXEC_COMMAND.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "check could not start".to_string(),
        ),
    });
    update_repetition_tracker(&mut tracker, &failed_check, tools::EXEC_COMMAND, &json!({"cmd":"cargo nextest run"}));
    assert!(tracker.verification_is_pending());

    update_repetition_tracker(&mut tracker, &edit, tools::EXEC_COMMAND, &json!({"cmd":"cargo nextest run"}));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_mutations, 0);
}

#[test]
fn carried_verification_checkpoint_clears_after_successful_check() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    let successful_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &successful_check,
        tools::EXEC_COMMAND,
        &json!({"cmd":"cargo check --locked"}),
    );

    assert!(!tracker.verification_is_pending());
}

#[test]
fn verification_snapshot_round_trips_through_session_state() {
    let tracker = LoopTracker::with_verification_snapshot((true, 0));
    assert_eq!(tracker.verification_snapshot(), (true, 0));
    assert_eq!(LoopTracker::new().verification_snapshot(), (false, 0));
}

#[test]
fn repetition_tracker_ignores_cancellations() {
    let mut tracker = LoopTracker::new();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);

    update_repetition_tracker(&mut tracker, &outcome, "edit_file", &json!({"path":"src/main.rs"}));

    assert_eq!(tracker.max_count_filtered(|_| false), 0);
}

#[test]
fn reset_after_balancer_recovery_preserves_anti_blind_state() {
    let mut tracker = LoopTracker::new();
    tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
    tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
    tracker.consecutive_mutations = 2;
    tracker.verification_pending = true;
    tracker.verification_warning_emitted = true;
    tracker.verification_block_notice_emitted = true;
    tracker.verification_result_lost_notice_pending = true;
    tracker.piped_verification_notice_pending = true;
    tracker.consecutive_navigations = 4;
    tracker.consecutive_low_signal_navigations = 3;
    tracker.total_low_signal_navigations = 7;
    tracker.record_low_signal("code_search::Widget::src".to_string());
    tracker.navigation_loop_recoveries = 3;

    tracker.reset_after_balancer_recovery();

    assert_eq!(tracker.max_count_filtered(|_| false), 0);
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.consecutive_mutations, 2);
    assert!(tracker.verification_pending);
    assert!(tracker.verification_is_pending());

    assert!(tracker.verification_warning_emitted);
    assert!(tracker.verification_block_notice_emitted);
    assert!(tracker.verification_result_lost_notice_pending);
    assert!(tracker.piped_verification_notice_pending);
    assert_eq!(tracker.consecutive_navigations, 0);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
    assert_eq!(tracker.navigation_loop_recoveries, 3);
}

#[test]
fn balancer_recovery_preserves_mutation_counts_without_arming_verification() {
    let mut tracker = LoopTracker::new();
    for _ in 0..(BLIND_EDITING_THRESHOLD - 1) {
        tracker.record_successful_mutation();
    }
    assert!(!tracker.verification_is_pending());

    tracker.reset_after_balancer_recovery();
    tracker.record_successful_mutation();

    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    assert!(!tracker.verification_is_pending(), "the registry owns completion debt");
}

#[test]
fn shell_activity_distinguishes_inspection_verification_and_mutation() {
    for command in [
        "rg -n 'LoopTracker' src",
        "find src -name '*.rs'",
        "cat Cargo.toml",
        "sed -n '1,80p' src/main.rs",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":command})),
            ShellActivity::Inspection,
            "{command}"
        );
    }

    for command in [
        "cargo check --locked",
        "cargo nextest run -p vtcode",
        "cargo clippy --all-targets",
        "cargo build --release",
        "./scripts/check-dev.sh --changed",
        "cargo check --locked > build.log",
        "cargo check &> build.log",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":command})),
            ShellActivity::Verification,
            "{command}"
        );
    }

    for command in [
        "cargo nextest run -p vtcode 2>&1 | head -c 4000",
        "cargo check | head -40",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":command})),
            ShellActivity::Mutation,
            "verification pipelines require reliable aggregate status: {command}"
        );
    }

    assert_eq!(
        classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":"sed -i '' 's/a/b/' src/lib.rs"})),
        ShellActivity::Mutation
    );
    assert_eq!(
        classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":"rm output && cargo check"})),
        ShellActivity::Mutation
    );
}

#[test]
fn inspection_commands_increment_navigation_instead_of_resetting_it() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for command in [
        "rg LoopTracker src",
        "find src -name '*.rs'",
        "cat Cargo.toml",
        "sed -n '1,20p' src/main.rs",
    ] {
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));
    }

    assert_eq!(tracker.consecutive_navigations, 4);
}

#[test]
fn navigation_tracking_ignores_nonsemantic_preview_controls() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"stdout":"useful source"}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let command = "sed -n '278,420p' src/startup/mod.rs";

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::EXEC_COMMAND,
        &json!({"cmd": command, "command": command, "action": "run"}),
    );
    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::EXEC_COMMAND,
        &json!({
            "cmd": command,
            "command": command,
            "max_output_tokens": 8000,
            "action": "run"
        }),
    );

    assert_eq!(tracker.consecutive_navigations, 2);
    assert_eq!(tracker.repeated_navigation_count(), 1);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
}

#[test]
fn productive_navigation_resets_only_consecutive_low_signal_count() {
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let hit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"results":[{"path":"src/lib.rs"}]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    for query in ["missing-a", "missing-b"] {
        update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &json!({"query":query, "path":"src"}));
    }
    update_repetition_tracker(&mut tracker, &hit, tools::CODE_SEARCH, &json!({"query":"LoopTracker", "path":"src"}));

    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 2);
}

#[test]
fn verification_resets_all_low_signal_navigation_counts() {
    let mut tracker = LoopTracker::new();
    tracker.consecutive_low_signal_navigations = 6;
    tracker.total_low_signal_navigations = 10;
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_EXEC,
        &json!({"action":"run", "command":"cargo check --locked"}),
    );

    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
}

#[test]
fn consecutive_mutations_increments_on_edit() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // edit_file is classified as mutating
    update_repetition_tracker(
        &mut tracker,
        &success,
        "edit_file",
        &json!({"path":"src/lib.rs","old_str":"a","new_str":"b"}),
    );
    assert_eq!(tracker.consecutive_mutations, 1);
    assert_eq!(tracker.consecutive_navigations, 0);

    update_repetition_tracker(&mut tracker, &success, "write_to_file", &json!({"path":"src/lib.rs","content":"x"}));
    assert_eq!(tracker.consecutive_mutations, 2);
}

#[test]
fn dynamic_readonly_inspections_do_not_arm_verification_gate() {
    let mut tracker = LoopTracker::new();
    // Session-vtcode-20261015T142625Z: dynamic `for` loops over grep results
    // with `test -f` exited 0 with no writes but were counted as blind edits,
    // bumping completion generation and looping harness auto-verification.
    for command in [
        "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](//' | sort -u); do test -f \"$p\" || echo \"missing: $p\"; done",
        "rg -n 'vinhnx/(VTCode|vtcode)' README.md",
        "grep -n '^## ' README.md",
        "git status --short",
        "rg -n \"MD013|line_length\" docs/development/*.md | head",
    ] {
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &outcome, "exec_command", &json!({"cmd": command}));
    }
    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());
}

#[test]
fn execution_tool_resets_mutation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // Two mutations
    update_repetition_tracker(&mut tracker, &success, "edit_file", &json!({"path":"a","old_str":"x","new_str":"y"}));
    update_repetition_tracker(&mut tracker, &success, "edit_file", &json!({"path":"b","old_str":"x","new_str":"y"}));
    assert_eq!(tracker.consecutive_mutations, 2);

    // Execution tool resets
    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_EXEC,
        &json!({"action":"run","command":"cargo check"}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}

#[test]
fn reads_increment_navigation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(&mut tracker, &success, tools::READ_FILE, &json!({"path":"src/main.rs"}));
    assert_eq!(tracker.consecutive_navigations, 1);
    assert_eq!(tracker.consecutive_mutations, 0);

    update_repetition_tracker(&mut tracker, &success, tools::GREP_FILE, &json!({"pattern":"foo","path":"src/"}));
    assert_eq!(tracker.consecutive_navigations, 2);
}

#[test]
fn mutation_resets_navigation_counter() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // Several reads
    for _ in 0..5 {
        update_repetition_tracker(&mut tracker, &success, tools::READ_FILE, &json!({"path":"src/main.rs"}));
    }
    assert_eq!(tracker.consecutive_navigations, 5);

    // A mutation resets navigation counter
    update_repetition_tracker(
        &mut tracker,
        &success,
        "edit_file",
        &json!({"path":"src/lib.rs","old_str":"a","new_str":"b"}),
    );
    assert_eq!(tracker.consecutive_navigations, 0);
    assert_eq!(tracker.consecutive_mutations, 1);
}

#[test]
fn task_tracker_does_not_increment_mutations_in_planning() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::TASK_TRACKER,
        &json!({"action":"create","items":["step"]}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}

#[test]
fn task_tracker_does_not_increment_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::TASK_TRACKER,
        &json!({"action":"create","items":["step"]}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}

#[test]
fn plan_file_write_does_not_increment_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":".vtcode/plans/my-plan.md","content":"text"}),
    );
    assert_eq!(tracker.consecutive_mutations, 0);
    assert_eq!(tracker.consecutive_navigations, 0);
}

#[test]
fn non_plan_file_write_still_increments_mutations() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &success,
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"text"}),
    );
    assert_eq!(tracker.consecutive_mutations, 1);
    assert_eq!(tracker.consecutive_navigations, 0);
}

#[test]
fn argument_error_detection_includes_required_update_fields() {
    assert!(check_is_argument_error("Tool execution failed: 'index' is required for 'update' (1-indexed)"));
}

#[test]
fn low_signal_tracker_groups_empty_search_results_by_family() {
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    // Different queries produce separate family keys, so each counts as its
    // own family while the agent explores one path.
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::CODE_SEARCH,
        &json!({"query":"Widget", "path":"src", "result_types":["definition"]}),
    );
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::CODE_SEARCH,
        &json!({"query":"Result", "path":"src", "result_types":["usage"]}),
    );
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::CODE_SEARCH,
        &json!({"query":"Result<", "path":"src", "result_types":["text"]}),
    );

    assert_eq!(tracker.max_low_signal_count(), 1);
}

#[test]
fn low_signal_tracker_groups_identical_searches_in_same_family() {
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"results":[]}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let args = json!({"query":"TODO","path":"src","file_types":["rust"]});
    update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);
    update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);
    update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);

    assert_eq!(tracker.max_low_signal_count(), 3);
}

#[test]
fn low_signal_tracker_ignores_empty_search_results_with_recovery_guidance() {
    let mut tracker = LoopTracker::new();
    let guided = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "results": [],
            "hint": "Try narrowing the path.",
            "is_recoverable": true,
            "next_action": "Retry with narrower filters."
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &guided,
        tools::CODE_SEARCH,
        &json!({"query":"run", "path":"src/agent", "result_types":["definition"]}),
    );

    assert_eq!(tracker.max_low_signal_count(), 0);
}

#[test]
fn low_signal_tracker_does_not_hide_structured_search_errors_as_empty_results() {
    let mut tracker = LoopTracker::new();
    let failure_like = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "results": [],
            "error": "permission denied while searching the workspace"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    update_repetition_tracker(
        &mut tracker,
        &failure_like,
        tools::CODE_SEARCH,
        &json!({"query":"secret", "path":"src"}),
    );

    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
}

#[test]
fn low_signal_tracker_counts_missing_read_failures() {
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_FILE.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
            "Resource not found: vtcode-tui/src/main.rs".to_string(),
        ),
    });

    // Two reads of the same path with different offsets are *different*
    // slices (paginated exploration), not a retry loop. The slice-aware
    // family key keeps them as distinct families, each with count 1.
    // Regression: previously both collapsed into one family with count 2,
    // which falsely tripped the family cap when the model paginated a
    // missing file (checkpoint turn_613 pattern).
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"vtcode-tui/src/main.rs"}),
    );
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"vtcode-tui/src/main.rs","offset":40}),
    );

    assert_eq!(
        tracker.max_low_signal_count(),
        1,
        "paginated reads (different offset) must be distinct families, not one family with count 2"
    );
}

#[test]
fn low_signal_tracker_counts_identical_missing_read_failures() {
    // True retry loop: same path + same slice, repeated. The low-signal
    // count must accumulate so the turn balancer can stop the churn.
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_FILE.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
            "Resource not found: vtcode-tui/src/main.rs".to_string(),
        ),
    });

    let identical_args = json!({"action":"read","path":"vtcode-tui/src/main.rs"});
    update_repetition_tracker(&mut tracker, &miss, tools::UNIFIED_FILE, &identical_args);
    update_repetition_tracker(&mut tracker, &miss, tools::UNIFIED_FILE, &identical_args);

    assert_eq!(
        tracker.max_low_signal_count(),
        2,
        "identical retry reads must accumulate into one family with count 2"
    );
}

#[test]
fn low_signal_tracker_counts_grep_style_shell_misses() {
    let mut tracker = LoopTracker::new();
    let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "command": "grep -n 'missing' vtcode-tui/src/main.rs",
            "exit_code": 1,
            "output": ""
        }),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::EXEC_COMMAND,
        &json!({"cmd":"grep -n 'missing' vtcode-tui/src/main.rs"}),
    );
    update_repetition_tracker(
        &mut tracker,
        &miss,
        tools::EXEC_COMMAND,
        &json!({"cmd":"grep -n \"missing\" vtcode-tui/src/main.rs"}),
    );

    assert_eq!(tracker.max_low_signal_count(), 2);
    assert_eq!(tracker.consecutive_low_signal_navigations, 2);
    assert_eq!(tracker.total_low_signal_navigations, 2);
}

#[test]
fn low_signal_tracker_counts_empty_successful_search_pipelines() {
    let mut tracker = LoopTracker::new();
    let empty = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"exit_code": 0, "output": "", "total_output_bytes": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for command in [
        "rg --files --hidden | rg -i 'markdownlint' | sed -n '1,15p'",
        "git ls-files | grep markdownlint | head -10",
        "git status --short --ignored | rg -i 'markdownlint' | sed -n '1,10p'",
        "rg -e -query src | head -10",
        "rg --field-match-separator -q missing src | head -10",
    ] {
        update_repetition_tracker(&mut tracker, &empty, tools::EXEC_COMMAND, &json!({"cmd": command}));
    }
    assert_eq!(tracker.consecutive_navigations, 5);
    assert_eq!(tracker.total_low_signal_navigations, 5);
    assert_eq!(tracker.low_signal_tool_calls, 5);
    assert_eq!(tracker.max_low_signal_count(), 1, "distinct searches retain distinct families");
    assert!(!tracker.verification_is_pending());
}

#[test]
fn low_signal_tracker_preserves_productive_hidden_and_quiet_searches() {
    let args = json!({"cmd": "rg missing src | head -10"});
    for output in [
        json!({"exit_code": 0, "output": "src/lib.rs:7:missing"}),
        json!({"exit_code": 0, "output": "", "total_output_bytes": 42}),
        json!({"exit_code": 0, "output": "", "output_truncated": true}),
        json!({"exit_code": 0, "output": "", "spool_path": ".vtcode/context/tool_outputs/search.txt"}),
        json!({"exit_code": 0, "output": "", "stderr": "permission denied"}),
        json!({"output": "", "lifecycle_state": "running"}),
        json!({"exit_code": 2, "output": ""}),
    ] {
        let mut tracker = LoopTracker::new();
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output,
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        tracker.mark_verification_pending();
        update_repetition_tracker(&mut tracker, &outcome, tools::EXEC_COMMAND, &args);
        assert_eq!(tracker.low_signal_tool_calls, 0, "{:?}", outcome.status);
        assert!(tracker.verification_is_pending(), "inspection never clears verification");
    }
    let empty = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"exit_code": 0, "output": ""}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let mut tracker = LoopTracker::new();
    update_repetition_tracker(&mut tracker, &empty, tools::EXEC_COMMAND, &json!({"cmd": "rg -q hit src"}));
    update_repetition_tracker(&mut tracker, &empty, tools::EXEC_COMMAND, &json!({"cmd": "cat empty.txt"}));
    assert_eq!(tracker.low_signal_tool_calls, 0);
}

#[test]
fn low_signal_tracker_does_not_count_grep_style_errors_as_no_match() {
    let mut tracker = LoopTracker::new();
    let error = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "command": "rg missing restricted",
            "exit_code": 2,
            "output": ""
        }),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });

    update_repetition_tracker(&mut tracker, &error, tools::EXEC_COMMAND, &json!({"cmd":"rg missing restricted"}));

    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
    assert_eq!(tracker.total_low_signal_navigations, 0);
}

#[test]
fn low_signal_tracker_does_not_hide_grep_errors_as_no_match() {
    let mut tracker = LoopTracker::new();
    let failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "command": "rg missing restricted",
            "exit_code": 1,
            "stdout": "",
            "stderr": "permission denied",
        }),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });

    update_repetition_tracker(&mut tracker, &failure, tools::EXEC_COMMAND, &json!({"cmd":"rg missing restricted"}));

    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.consecutive_low_signal_navigations, 0);
}

// --- read_normalized_signature_key tests ---

#[test]
fn read_normalized_signature_key_normalizes_file_operation_read_offset() {
    let args_a = json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100});
    let args_b = json!({"action": "read", "path": "src/lib.rs", "offset": 50, "limit": 200});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_eq!(key_a, key_b, "same file read with different offset/limit should produce the same normalized key");
}

#[test]
fn read_normalized_signature_key_preserves_encoding() {
    let utf8 = json!({"action": "read", "path": "src/lib.rs", "encoding": "utf8"});
    let base64 = json!({"action": "read", "path": "src/lib.rs", "encoding": "base64"});

    assert_ne!(
        read_normalized_signature_key("file_operation", &utf8),
        read_normalized_signature_key("file_operation", &base64),
        "different encodings produce different tool output and must not reuse one another"
    );
}

#[test]
fn read_normalized_signature_key_differentiates_different_paths() {
    let args_a = json!({"action": "read", "path": "src/lib.rs"});
    let args_b = json!({"action": "read", "path": "src/main.rs"});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_ne!(key_a, key_b, "different paths must produce different keys");
}

#[test]
fn read_normalized_signature_key_includes_code_search_limit_and_normalises_filter_order() {
    let args_a = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust", "typescript"],
        "result_types": ["text", "definition"],
        "max_results": 10
    });
    let args_b = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["typescript", "rs"],
        "result_types": ["definition", "text"],
        "max_results": 100
    });
    let key_a = read_normalized_signature_key(tools::CODE_SEARCH, &args_a);
    let key_b = read_normalized_signature_key(tools::CODE_SEARCH, &args_b);
    assert_ne!(key_a, key_b, "different effective limits must not share one code-search replay identity");

    let args_default = json!({
        "query": " Widget ",
        "path": "src",
        "file_types": ["rs", "typescript"],
        "result_types": ["definition", "text"]
    });
    let args_explicit_default = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["typescript", "rust"],
        "result_types": ["text", "definition"],
        "max_results": 20
    });
    assert_eq!(
        read_normalized_signature_key(tools::CODE_SEARCH, &args_default),
        read_normalized_signature_key(tools::CODE_SEARCH, &args_explicit_default),
        "omitted and explicit default limits must share replay identity"
    );
}

#[test]
fn read_normalized_signature_key_preserves_mutation_for_write() {
    let args_a = json!({"path": "src/lib.rs", "content": "old"});
    let args_b = json!({"path": "src/lib.rs", "content": "new"});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_ne!(key_a, key_b, "mutating writes must NOT be normalized away");
}

#[test]
fn find_duplicate_in_history_matches_normalized_read() {
    use vtcode_core::llm::provider as uni;

    // find_duplicate_in_history uses read_normalized_signature_key, which
    // strips offset/limit for file reads. A later unrelated Assistant batch
    // must not obscure the earlier matching call and result pair.

    // Verify normalization: same file + different offset/limit → same key
    let key_a = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
    );
    let key_b = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":50,"limit":500}),
    );
    assert_eq!(key_a, key_b, "same file read with different offset/limit should normalize to the same key");

    // Verify: different file → different key
    let key_c = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
    );
    assert_ne!(key_a, key_c, "different files must produce different normalized keys");

    // Verify: code-search result limits remain distinct while filter ordering normalises away.
    let s_key_a = read_normalized_signature_key(
        tools::CODE_SEARCH,
        &json!({"query":"Widget","path":"src","file_types":["rust","typescript"],"result_types":["text","definition"],"max_results":10}),
    );
    let s_key_b = read_normalized_signature_key(
        tools::CODE_SEARCH,
        &json!({"query":"Widget","path":"src","file_types":["typescript","rs"],"result_types":["definition","text"],"max_results":100}),
    );
    assert_ne!(s_key_a, s_key_b, "different effective limits must not share one code-search replay identity");

    // Verify: write NOT normalized
    let w_key_a = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"old"}),
    );
    let w_key_b = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"new"}),
    );
    assert_ne!(w_key_a, w_key_b, "writes must not be normalized away");

    // Verify: find_duplicate_in_history still works for EXACT match
    let mut history: Vec<uni::Message> = Vec::new();
    history.push(uni::Message::assistant_with_tools(
        "read".into(),
        vec![uni::ToolCall::function(
            "tc_exact".into(),
            tools::UNIFIED_FILE.into(),
            serde_json::to_string(&json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100})).unwrap(),
        )],
    ));
    history.push(uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("exact content".into()),
        tool_call_id: Some("tc_exact".into()),
        ..Default::default()
    });
    // Second pair (different file) so the scan finds A₀'s Tool after A₁:
    history.push(uni::Message::assistant_with_tools(
        "read other".into(),
        vec![uni::ToolCall::function(
            "tc_other".into(),
            tools::UNIFIED_FILE.into(),
            serde_json::to_string(&json!({"action":"read","path":"src/main.rs"})).unwrap(),
        )],
    ));
    history.push(uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("other content".into()),
        tool_call_id: Some("tc_other".into()),
        ..Default::default()
    });

    let result = find_duplicate_in_history(
        &history,
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":50}),
        Path::new("."),
    );
    assert_eq!(result.as_deref(), Some("exact content"));
}

#[test]
fn find_duplicate_in_history_respects_normalised_code_search_limit() {
    let original_args = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust", "typescript"],
        "result_types": ["text", "definition"],
        "max_results": 10
    });
    let history = vec![
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "tc_search".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&original_args).unwrap(),
            )],
        ),
        uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("{\"results\":[]}".into()),
            tool_call_id: Some("tc_search".into()),
            ..Default::default()
        },
    ];

    let different_limit = find_duplicate_in_history(
        &history,
        tools::CODE_SEARCH,
        &json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["typescript", "rs"],
            "result_types": ["definition", "text"],
            "max_results": 100
        }),
        Path::new("."),
    );

    assert_eq!(different_limit, None);

    let equivalent_default_history = vec![
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "tc_default".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&json!({
                    "query": "Widget",
                    "path": "src",
                    "max_results": 20
                }))
                .unwrap(),
            )],
        ),
        uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("{\"results\":[1]}".into()),
            tool_call_id: Some("tc_default".into()),
            ..Default::default()
        },
    ];
    let reused = find_duplicate_in_history(
        &equivalent_default_history,
        tools::CODE_SEARCH,
        &json!({"query": " Widget ", "path": "src"}),
        Path::new("."),
    );
    assert_eq!(reused.as_deref(), Some("{\"results\":[1]}"));
}

#[test]
fn working_history_code_search_replay_stops_at_in_scope_mutation() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let search_call = uni::Message::assistant_with_tools(
        "search".into(),
        vec![uni::ToolCall::function(
            "search_call".into(),
            tools::CODE_SEARCH.into(),
            serde_json::to_string(&search_args).unwrap(),
        )],
    );
    let search_result = uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("{\"results\":[\"cached\"]}".into()),
        tool_call_id: Some("search_call".into()),
        ..Default::default()
    };
    let mutation = |path: &str, result: serde_json::Value| {
        let patch = format!("*** Begin Patch\n*** Update File: {path}\n@@\n-Widget\n+Gadget\n*** End Patch\n");
        vec![
            uni::Message::assistant_with_tools(
                "edit".into(),
                vec![uni::ToolCall::function(
                    "edit_call".into(),
                    tools::APPLY_PATCH.into(),
                    serde_json::to_string(&json!({"patch": patch})).unwrap(),
                )],
            ),
            uni::Message::tool_response("edit_call".into(), result.to_string()),
        ]
    };

    let mut in_scope_history = vec![search_call.clone(), search_result.clone()];
    in_scope_history.extend(mutation("src/widget.rs", json!({"success": true})));
    assert!(
        find_duplicate_in_history(&in_scope_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "editing src/widget.rs after searching src must force a fresh search"
    );

    let mut status_success_history = vec![search_call.clone(), search_result.clone()];
    status_success_history.extend(mutation("src/widget.rs", json!({"status": "success", "output": "patch applied"})));
    assert!(
        find_duplicate_in_history(&status_success_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "the established successful status shape must invalidate replay"
    );

    let mut unrelated_history = vec![search_call.clone(), search_result.clone()];
    unrelated_history.extend(mutation("tests/widget.rs", json!({"success": true})));
    assert_eq!(
        find_duplicate_in_history(&unrelated_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"cached\"]}"),
        "an unrelated edit may reuse the prior scoped search"
    );

    for failure in [
        json!({"success": false, "error": "patch rejected"}),
        json!({"error": {"message": "execution denied by policy"}}),
        json!({"failure_kind": "timeout"}),
        json!({"status": "failed"}),
        json!({"status": "denied"}),
        json!({"success": null}),
        json!({"output": "patch output without an outcome"}),
        json!(["non-object mutation output"]),
    ] {
        let mut failed_history = vec![search_call.clone(), search_result.clone()];
        failed_history.extend(mutation("src/widget.rs", failure));
        assert_eq!(
            find_duplicate_in_history(&failed_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
            Some("{\"results\":[\"cached\"]}"),
            "a mutation without explicit positive success evidence must preserve reuse"
        );
    }

    let mut unexecuted_history = vec![search_call, search_result];
    let unexecuted_mutation = mutation("src/widget.rs", json!({"success": true}));
    unexecuted_history.push(unexecuted_mutation[0].clone());
    assert_eq!(
        find_duplicate_in_history(&unexecuted_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"cached\"]}"),
        "an unexecuted mutation call must preserve reuse"
    );
}

#[test]
fn mutation_tool_response_success_rejects_malformed_and_conflicting_shapes() {
    let response = |content: &str| uni::Message::tool_response("edit_call".into(), content.into());

    assert!(tool_response_is_success(&response(r#"{"success":true}"#)));
    assert!(tool_response_is_success(&response(r#"{"status":"success","output":"patch applied"}"#,)));

    for content in [
        "not json",
        "null",
        r#"{"success":null,"status":"success"}"#,
        r#"{"success":true,"status":"failed"}"#,
        r#"{"success":true,"failure_kind":"timeout"}"#,
        r#"{"success":true,"error":"execution denied"}"#,
    ] {
        assert!(
            !tool_response_is_success(&response(content)),
            "mutation outcome must not count as successful: {content}"
        );
    }
}

#[test]
fn duplicate_history_reuse_rejects_failed_results() {
    let args = json!({"query": "needle", "path": "src"});
    let call = || {
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "search_call".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&args).unwrap(),
            )],
        )
    };

    for failure in [
        r#"{"success":false,"output":"partial"}"#,
        r#"{"status":"timeout","output":"partial"}"#,
        r#"{"error":"permission denied"}"#,
        "Error: command failed",
        "timed out while reading",
        "failed to execute command",
        "denied by policy",
        "blocked until verification",
        "not executed",
    ] {
        let history = vec![
            call(),
            uni::Message::tool_response("search_call".into(), failure.into()),
        ];
        assert!(
            find_duplicate_in_history(&history, tools::CODE_SEARCH, &args, Path::new(".")).is_none(),
            "failed result must not be replayed: {failure}"
        );
    }

    for success in [r#"{"results":[]}"#, "[]", "plain successful output"] {
        let history = vec![
            call(),
            uni::Message::tool_response("search_call".into(), success.into()),
        ];
        assert_eq!(
            find_duplicate_in_history(&history, tools::CODE_SEARCH, &args, Path::new(".")).as_deref(),
            Some(success)
        );
    }
}

#[test]
fn working_history_code_search_replay_rejects_reused_patch_call_id() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let shared_call_id = "call_0";
    let search_call = uni::Message::assistant_with_tools(
        "search".into(),
        vec![uni::ToolCall::function(
            shared_call_id.into(),
            tools::CODE_SEARCH.into(),
            serde_json::to_string(&search_args).unwrap(),
        )],
    );
    let search_result =
        uni::Message::tool_response(shared_call_id.into(), "{\"results\":[\"genuine search output\"]}".into());
    let patch = "*** Begin Patch\n*** Update File: src/widget.rs\n@@\n-Widget\n+Gadget\n*** End Patch\n";
    let patch_call = uni::Message::assistant_with_tools(
        "edit".into(),
        vec![uni::ToolCall::function(
            shared_call_id.into(),
            tools::APPLY_PATCH.into(),
            serde_json::to_string(&json!({"patch": patch})).unwrap(),
        )],
    );

    let mut successful_history = vec![
        search_call.clone(),
        search_result.clone(),
        patch_call.clone(),
        uni::Message::tool_response(
            shared_call_id.into(),
            json!({"success": true, "output": "patch output"}).to_string(),
        ),
    ];
    assert!(
        find_duplicate_in_history(&successful_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "a successful in-scope patch must invalidate the genuine earlier search result"
    );

    successful_history.pop();
    successful_history.push(uni::Message::tool_response(
        shared_call_id.into(),
        json!({"success": false, "error": "patch rejected", "output": "patch output"}).to_string(),
    ));
    assert_eq!(
        find_duplicate_in_history(&successful_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"genuine search output\"]}"),
        "a failed patch must preserve the earlier search without returning patch output"
    );
}

#[test]
fn read_extent_covers_query_rejects_larger_limit() {
    // Cached limit=200 must NOT cover query limit=220
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
    ));

    // Cached limit=200 covers query limit=200 (same)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
    ));

    // Cached limit=200 covers query limit=100 (subset)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
    ));

    // Different offset must not match
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":50,"limit":200}),
    ));
}

#[test]
fn read_extent_covers_query_rejects_different_raw_mode() {
    // Non-raw cached must NOT cover raw=true query
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
    ));

    // Raw cached covers raw query
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
    ));

    // Raw cached must NOT cover non-raw query
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
    ));
}

#[test]
fn read_extent_covers_query_handles_missing_limit() {
    // Both missing limit → matches (same default read)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md"}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));

    // Cached has limit, query doesn't → mismatch
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));

    // Cached has no limit, query does → mismatch
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));
}

fn successful_exec_output() -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    })
}

#[test]
fn coarse_listing_count_groups_same_root_rescans() {
    let mut tracker = LoopTracker::new();
    assert_eq!(tracker.max_coarse_listing_count(), 0);
    // Same root across flag and quote variations: one coarse family.
    for command in ["ls src", "ls \"src\"", "ls -1 src/"] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 3);
    assert_eq!(tracker.dominant_churn(), Some(("exec::inspection::ls::src".to_string(), 3)));
}

#[test]
fn coarse_listing_count_separates_distinct_roots() {
    let mut tracker = LoopTracker::new();
    // Distinct trees are legitimate exploration, not churn.
    for command in ["ls src", "ls crates", "ls tests"] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 1);
}

#[test]
fn coarse_listing_count_keeps_binaries_in_separate_families() {
    let mut tracker = LoopTracker::new();
    for command in ["ls src", "find crates -name lib.rs", "fd main src"] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 1);
}

#[test]
fn coarse_listing_count_ignores_grep_style_searches() {
    let mut tracker = LoopTracker::new();
    for command in ["rg foo src", "rg bar crates", "grep -r baz src"] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 0);
    // `rg`/`grep` must not enter the coarse ledger at all, so they can
    // never be promoted to low-signal or named as dominant churn.
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.dominant_churn(), None);
    assert_eq!(tracker.low_signal_tool_calls, 0);
}

#[test]
fn same_pattern_grep_searches_do_not_promote_to_low_signal() {
    // Regression for turn_1303/turn_1304 (`exec::inspection::grep::enum ×5`)
    // and turn_1291 (`exec::inspection::rg::pub ×5`): five distinct
    // successful searches sharing one pattern (`enum` / `pub`) across
    // different files/flags are legitimate research, not churn. They must
    // not be promoted into the low-signal ledger and must not trip early
    // recovery on their own.
    let mut tracker = LoopTracker::new();
    for command in [
        "grep -n \"enum Commands\" -A 80 src/cli/mod.rs",
        "grep -n \"enum Command\\|pub enum\" src/cli/mod.rs",
        "grep -rn \"enum Commands\" crates/codegen/vtcode-core/src",
        "grep -rn \"enum ExecSubcommand\" -A 30 crates/codegen/vtcode-core/src/cli/args/",
        "grep -rn \"enum ScheduleSubcommand\" crates/codegen/vtcode-core/src/cli/args/",
    ] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 0);
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.dominant_churn(), None);

    let mut tracker = LoopTracker::new();
    for command in [
        "rg -n 'pub enum Commands' src/ -A 40",
        "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 50",
        "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 600",
        "rg -n 'pub enum Provider|Gemini|OpenAI' crates/codegen/vtcode-llm/src",
        "rg -n 'pub enum SecretCommand|Add|List' crates/codegen/vtcode-core/src/cli/args/secret.rs",
    ] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.max_coarse_listing_count(), 0);
    assert_eq!(tracker.max_low_signal_count(), 0);
    assert_eq!(tracker.dominant_churn(), None);
}

#[test]
fn coarse_inspection_root_extracts_first_positional_token() {
    assert_eq!(coarse_inspection_root("ls src/"), "src");
    assert_eq!(coarse_inspection_root("ls \"src/\""), "src");
    assert_eq!(coarse_inspection_root("find src-tauri/ -maxdepth 1"), "src-tauri");
    assert_eq!(coarse_inspection_root("ls -la"), ".");
    assert_eq!(coarse_inspection_root("ls"), ".");
    // Heuristic: an option value can be picked up as the root, which only
    // fragments families and keeps detection conservative.
    assert_eq!(coarse_inspection_root("ls --width 80 src"), "80");
}

#[test]
fn promoted_listing_repeat_counts_toward_low_signal_telemetry() {
    // Three same-root successful listings: the third is promoted into the
    // low-signal ledger, so telemetry records exactly one low-signal call
    // even though every listing succeeded (the old
    // `low_signal_tool_calls:0 on 3×find` diagnostics gap).
    let mut tracker = LoopTracker::new();
    for command in ["ls src", "ls -1 src", "ls src/"] {
        update_repetition_tracker(
            &mut tracker,
            &successful_exec_output(),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
    }
    assert_eq!(tracker.low_signal_tool_calls, 1);
    assert_eq!(tracker.total_low_signal_navigations, 1);
    assert_eq!(tracker.consecutive_low_signal_navigations, 1);
    assert_eq!(tracker.max_coarse_listing_count(), 3);
}

#[test]
fn concrete_exec_changes_and_unproven_inspections_incur_mutation_pressure() {
    for (command, modified_files) in [
        ("grep -n x README.md", vec!["src/lib.rs".to_string()]),
        ("echo \"$(touch marker)\"", vec![]),
        ("for p in a; do echo x > marker; done", vec![]),
        ("for p in a; do sed -i '' 's/a/b/' README.md; done", vec![]),
        ("for p in a; do echo x", vec![]),
    ] {
        let mut tracker = LoopTracker::new();
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code":0}),
            stdout: None,
            modified_files,
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &outcome, "exec_command", &json!({"cmd":command}));
        assert_eq!(tracker.consecutive_mutations, 1, "{command}");
    }
}
