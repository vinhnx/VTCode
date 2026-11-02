use super::*;

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
