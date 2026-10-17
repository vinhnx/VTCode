use super::*;

#[test]
fn piped_verifier_success_while_pending_queues_notice_once() {
    // Regression guard for session-vtcode-20260912T083718Z: a piped
    // verifier success exited 0 while the gate was pending, cleared
    // nothing, and said nothing — the model believed it had verified and
    // the turn deadlocked on unverified text responses.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let piped_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &piped_success,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked -p vtcode 2>&1 | grep -E 'error|warning'; true"}),
    ));
    assert!(tracker.verification_is_pending(), "piped success must not clear the gate");
    assert!(tracker.take_piped_verification_notice(), "piped success must queue the notice");
    assert!(!tracker.take_piped_verification_notice(), "notice is one-shot");

    // A standalone pure-`&&` verifier success afterwards clears the gate
    // together with any queued piped notice.
    let chained_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &chained_success,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo fmt --all -- --check && cargo check --locked"}),
    ));
    assert!(!tracker.verification_is_pending());
    assert!(!tracker.take_piped_verification_notice());
}

#[test]
fn piped_verifier_success_without_pending_gate_stays_silent() {
    let mut tracker = LoopTracker::new();
    let piped_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &piped_success,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked 2>&1 | grep error"}),
    ));
    assert!(!tracker.take_piped_verification_notice());
}

#[test]
fn masked_verifier_feedback_is_once_per_turn_without_changing_gate_or_repair_budget() {
    let args = json!({"cmd":"npx markdownlint-cli2 README.md; echo \"lint exit: $?\""});
    for pending in [false, true] {
        let mut tracker = LoopTracker::with_verification_snapshot((pending, 0));
        assert_eq!(
            mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &args),
            pending,
            "diagnostic detection must not change admission"
        );
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code":0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for attempt in 0..3 {
            assert!(!update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &args));
            assert_eq!(tracker.take_piped_verification_notice(), attempt == 0);
            assert_eq!(tracker.verification_is_pending(), pending);
            assert_eq!(tracker.fix_edits_remaining, 0);
            tracker.reset_after_balancer_recovery();
        }
        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::EXEC_COMMAND,
            &json!({"cmd":"npx markdownlint-cli2 README.md"}),
        );
        assert!(!tracker.verification_is_pending());
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &args);
        assert!(!tracker.take_piped_verification_notice(), "successful verification must not replenish coaching");
        assert!(!tracker.verification_is_pending());
    }
}

#[test]
fn masked_verifier_feedback_requires_execution_and_terminal_status() {
    let args = json!({"cmd":"cargo check; echo $?"});
    for status in [
        ToolExecutionStatus::Success {
            output: json!({"session_id":"running-check"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        },
        ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::policy_violation(tools::EXEC_COMMAND, "denied"),
        },
        ToolExecutionStatus::Cancelled,
    ] {
        let mut tracker = LoopTracker::new();
        update_repetition_tracker(&mut tracker, &ToolPipelineOutcome::from_status(status), tools::EXEC_COMMAND, &args);
        assert!(!tracker.take_piped_verification_notice());
        assert!(!tracker.verification_is_pending());
        assert_eq!(tracker.fix_edits_remaining, 0);
    }
}

#[test]
fn masked_checker_polling_retains_only_diagnostic_identity() {
    for pending in [false, true] {
        for exit_code in [0, 7] {
            let mut tracker = LoopTracker::with_verification_snapshot((pending, 0));
            let running = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: json!({"session_id": "42"}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            });
            for id in ["42", "43"] {
                let running = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                    output: json!({"session_id": id}),
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                });
                update_repetition_tracker(
                    &mut tracker,
                    &running,
                    tools::EXEC_COMMAND,
                    &json!({"cmd":"cargo check; echo $?"}),
                );
            }
            assert!(!tracker.take_piped_verification_notice());
            assert!(tracker.pending_verifier_session_id.is_none());
            let terminal = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: json!({"exit_code":exit_code}),
                stdout: None,
                modified_files: vec![],
                command_success: exit_code == 0,
            });
            update_repetition_tracker(&mut tracker, &terminal, tools::WRITE_STDIN, &json!({"session_id":"99"}));
            update_repetition_tracker(
                &mut tracker,
                &running,
                tools::UNIFIED_EXEC,
                &json!({"session_id":"42","action":"wait"}),
            );
            assert!(!tracker.take_piped_verification_notice());
            tracker.reset_after_balancer_recovery();
            for (id, name) in [(" 42 ", tools::WRITE_STDIN), ("43", tools::UNIFIED_EXEC)] {
                assert!(!update_repetition_tracker(
                    &mut tracker,
                    &terminal,
                    name,
                    &json!({"session_id":id,"action":"wait"})
                ));
                assert_eq!(tracker.take_piped_verification_notice(), id.trim() == "42");
                assert_eq!(tracker.verification_is_pending(), pending);
                assert_eq!(tracker.fix_edits_remaining, 0);
                assert!(!tracker.take_verification_result_lost_notice());
            }
            assert!(tracker.pending_checker_session_ids.is_empty());
            update_repetition_tracker(&mut tracker, &terminal, tools::WRITE_STDIN, &json!({"session_id":"42"}));
            assert!(!tracker.take_piped_verification_notice());
        }
    }
}

#[test]
fn masked_checker_session_loss_and_cleanup_never_grant_verifier_recovery() {
    for (action, lost) in [("wait", true), ("close", false), ("close", true)] {
        let mut tracker = LoopTracker::new();
        let running = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"session_id":"diagnostic"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &running, tools::EXEC_COMMAND, &json!({"cmd":"cargo check; echo $?"}));
        tracker.mark_verification_pending();
        let args = json!({"session_id":"diagnostic", "action":action});
        let cancelled = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);
        update_repetition_tracker(&mut tracker, &cancelled, tools::WRITE_STDIN, &args);
        assert!(tracker.pending_checker_session_ids.contains("diagnostic"));
        let rejected = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::policy_violation(tools::WRITE_STDIN, "denied"),
        });
        update_repetition_tracker(&mut tracker, &rejected, tools::WRITE_STDIN, &args);
        assert!(tracker.pending_checker_session_ids.contains("diagnostic"));
        let outcome = if !lost {
            ToolExecutionStatus::Success {
                output: json!({"exit_code":0}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            }
        } else {
            ToolExecutionStatus::Failure {
                error: vtcode_core::tools::registry::ToolExecutionError::new(
                    tools::WRITE_STDIN,
                    vtcode_core::tools::registry::ToolErrorType::ExecutionError,
                    "exec session 'diagnostic' not found".to_string(),
                ),
            }
        };
        assert!(!update_repetition_tracker(
            &mut tracker,
            &ToolPipelineOutcome::from_status(outcome),
            tools::WRITE_STDIN,
            &args
        ));
        assert!(tracker.pending_checker_session_ids.is_empty());
        assert!(tracker.verification_is_pending());
        assert_eq!(tracker.fix_edits_remaining, 0);
        assert!(!tracker.take_piped_verification_notice());
        assert!(!tracker.take_verification_result_lost_notice());
    }
}

#[test]
fn failed_masked_checker_queues_feedback_without_granting_repair_edits() {
    let mut tracker = LoopTracker::new();
    let failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"exit_code":7}),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });
    update_repetition_tracker(&mut tracker, &failure, tools::EXEC_COMMAND, &json!({"cmd":"cargo check; echo $?"}));
    assert!(tracker.take_piped_verification_notice());
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
}

#[test]
fn fmt_check_clears_gate_but_plain_fmt_does_not() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo fmt --all -- --check"})
    ));

    let fmt_check_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(
        &mut tracker,
        &fmt_check_success,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo fmt --all -- --check"}),
    );
    assert!(!tracker.verification_is_pending());

    // Plain `cargo fmt` rewrites files: it stays a mutation and never
    // clears the gate.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    assert!(mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": "cargo fmt"})));
}

#[test]
fn failed_verifier_reports_fix_window_for_text_streak_reset() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let failed_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 1}),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &failed_check,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked"}),
    ));
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);

    let successful_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &successful_check,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked"}),
    ));
}

#[test]
fn lost_verification_tool_failure_while_pending_grants_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let tool_failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::EXEC_COMMAND.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "check could not start".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &tool_failure,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(
        !mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})),
        "the lost-result grant must open the bounded fix window"
    );
    assert!(tracker.take_verification_result_lost_notice(), "the lost-result directive must be queued");
    assert!(!tracker.take_verification_result_lost_notice(), "the notice is one-shot");
}

#[test]
fn lost_verification_tool_timeout_while_pending_grants_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let tool_timeout = ToolPipelineOutcome::from_status(ToolExecutionStatus::Timeout {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::EXEC_COMMAND.to_string(),
            vtcode_core::tools::registry::ToolErrorType::Timeout,
            "verification command timed out".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &tool_timeout,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo nextest run"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
}

#[test]
fn verification_tool_failure_without_pending_gate_grants_no_fix_window() {
    let mut tracker = LoopTracker::new();
    let tool_failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::EXEC_COMMAND.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "check could not start".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &tool_failure,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked"}),
    ));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn missing_session_recovery_preserves_unrelated_running_verifier() {
    for (args, grants_recovery) in [
        (json!({"session_id": "run-verifier", "action": "wait"}), true),
        (json!({"session_id": " run-verifier ", "action": "wait"}), true),
        (json!({"s": "run-verifier", "action": "wait"}), true),
        (json!({"session_id": "run-other", "action": "wait"}), false),
        (json!({"action": "wait"}), false),
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.pending_verifier_session_id = Some("run-verifier".to_string());
        let failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                tools::WRITE_STDIN,
                vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
                "session unavailable",
            )
            .with_debug_metadata("failure_code", "exec_session_not_found"),
        });
        assert_eq!(
            update_repetition_tracker(&mut tracker, &failure, tools::WRITE_STDIN, &args),
            grants_recovery,
            "{args}"
        );
        assert!(tracker.verification_is_pending());
        assert_eq!(tracker.take_verification_result_lost_notice(), grants_recovery);
        if grants_recovery {
            assert!(tracker.pending_verifier_session_id.is_none());
            assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
        } else {
            assert_eq!(tracker.pending_verifier_session_id.as_deref(), Some("run-verifier"));
            assert_eq!(tracker.fix_edits_remaining, 0);
        }
    }
}

#[test]
fn write_stdin_lost_exec_session_failure_while_pending_grants_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::WRITE_STDIN.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::WRITE_STDIN,
        &json!({"session_id": "run-7", "chars": ""}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
    assert!(tracker.take_verification_result_lost_notice());
}

#[test]
fn typed_lost_exec_session_failure_preserves_verification_gate() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    let error = vtcode_core::tools::registry::ToolExecutionError::new(
        tools::WRITE_STDIN,
        vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
        "runtime handle unavailable",
    )
    .with_debug_metadata("failure_code", "exec_session_not_found");
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure { error });
    assert!(update_repetition_tracker(
        &mut tracker,
        &outcome,
        tools::WRITE_STDIN,
        &json!({"action": "wait", "session_id": "run-7"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(tracker.take_verification_result_lost_notice());
}

#[test]
fn write_stdin_unrelated_failure_while_pending_grants_no_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let unrelated = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::WRITE_STDIN.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "session is not writable".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &unrelated,
        tools::WRITE_STDIN,
        &json!({"session_id": "run-7", "chars": "q"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn unified_exec_wait_on_lost_session_while_pending_grants_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::UNIFIED_EXEC,
        &json!({"action": "wait", "session_id": "run-7"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
    assert!(tracker.take_verification_result_lost_notice());
    assert!(!tracker.take_verification_result_lost_notice(), "the notice is one-shot");
}

#[test]
fn unified_exec_poll_on_lost_session_while_pending_grants_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::UNIFIED_EXEC,
        &json!({"action": "poll", "session_id": "run-7"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(tracker.take_verification_result_lost_notice());
}

#[test]
fn unified_exec_inferred_poll_on_lost_session_while_pending_grants_fix_window() {
    // No explicit `action`: a bare `session_id` infers a poll follow-up,
    // so the lost-session branch must still fire.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::UNIFIED_EXEC,
        &json!({"session_id": "run-7"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(tracker.take_verification_result_lost_notice());
}

#[test]
fn unified_exec_wait_unrelated_failure_while_pending_grants_no_fix_window() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let unrelated = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "session_id is required for command session wait".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &unrelated,
        tools::UNIFIED_EXEC,
        &json!({"action": "wait"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn unified_exec_wait_lost_session_without_pending_gate_grants_nothing() {
    let mut tracker = LoopTracker::new();
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::UNIFIED_EXEC,
        &json!({"action": "wait", "session_id": "run-7"}),
    ));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn unified_exec_run_action_with_lost_session_text_takes_no_follow_up_path() {
    // A fresh `run` creates its session, so it is not a follow-up: the
    // error text alone must not open the lost-result window. (A run whose
    // command classifies as Verification still takes the generic
    // verifier-loss branch; `echo` classifies as Inspection, so nothing
    // is granted here.)
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::UNIFIED_EXEC.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "exec session 'run-7' not found. Copy the exact `session_id` from the original run response".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &failure,
        tools::UNIFIED_EXEC,
        &json!({"action": "run", "command": "echo hi"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn read_pty_session_poll_on_lost_pty_session_while_pending_grants_fix_window() {
    // A verifier waited on via a PTY session poll whose session died
    // reports "PTY session '<id>' not found" — the same lost-result shape
    // as the exec-session manager, and it needs the same bounded window.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::READ_PTY_SESSION.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "PTY session 'pty-3' not found".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::READ_PTY_SESSION,
        &json!({"session_id": "pty-3"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
    assert!(tracker.take_verification_result_lost_notice());
    assert!(!tracker.take_verification_result_lost_notice(), "the notice is one-shot");
}

#[test]
fn send_pty_input_on_lost_pty_session_while_pending_grants_fix_window() {
    // `send_pty_input` is a session follow-up (carries `session_id`, never
    // classifies as Verification), so a dead session there grants too.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::SEND_PTY_INPUT.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "PTY session 'pty-3' not found".to_string(),
        ),
    });
    assert!(update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::SEND_PTY_INPUT,
        &json!({"session_id": "pty-3", "input": "q"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(tracker.take_verification_result_lost_notice());
}

#[test]
fn create_pty_session_run_with_lost_session_text_takes_no_follow_up_path() {
    // A fresh PTY `run` creates its session, so it is not a follow-up:
    // the error text alone must not open the lost-result window.
    // (`cargo check` classifies as Verification, so use `echo` which
    // classifies as Inspection — nothing is granted here.)
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let failure = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::CREATE_PTY_SESSION.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "PTY session 'pty-3' not found".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &failure,
        tools::CREATE_PTY_SESSION,
        &json!({"command": "echo hi"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn pty_follow_up_lost_session_without_pending_gate_grants_nothing() {
    let mut tracker = LoopTracker::new();
    let lost_session = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::READ_PTY_SESSION.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "PTY session 'pty-3' not found".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &lost_session,
        tools::READ_PTY_SESSION,
        &json!({"session_id": "pty-3"}),
    ));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}

#[test]
fn pty_follow_up_unrelated_failure_while_pending_grants_no_fix_window() {
    // "no longer writable" is a live-session failure, not a lost session:
    // it carries no "not found", so the lost-result branch must not fire.
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let unrelated = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::new(
            tools::READ_PTY_SESSION.to_string(),
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "PTY session 'pty-3' is no longer writable".to_string(),
        ),
    });
    assert!(!update_repetition_tracker(
        &mut tracker,
        &unrelated,
        tools::READ_PTY_SESSION,
        &json!({"session_id": "pty-3"}),
    ));
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(!tracker.take_verification_result_lost_notice());
}
