use super::*;

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
