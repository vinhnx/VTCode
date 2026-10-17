use super::*;

#[test]
fn verification_gate_blocks_mutations_but_allows_reads_checks_and_plan_artifacts() {
    let mut tracker = LoopTracker::new();
    tracker.verification_pending = true;

    assert!(mutation_blocked_until_verification(
        &tracker,
        tools::WRITE_FILE,
        &json!({"path":"src/lib.rs","content":"new"})
    ));
    // Docs-only prose stays allowed while pending and never trips the gate.
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::WRITE_FILE,
        &json!({"path":"README.md","content":"new"})
    ));
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::WRITE_FILE,
        &json!({"path":"docs/guide.md","content":"new"})
    ));
    // Mixed docs+code patches stay blocked (fail closed).
    assert!(mutation_blocked_until_verification(
        &tracker,
        tools::APPLY_PATCH,
        &json!({"patch":"*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+new\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch\n"})
    ));
    assert!(mutation_blocked_until_verification(
        &tracker,
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -i '' 's/old/new/' README.md"})
    ));
    assert!(!mutation_blocked_until_verification(&tracker, tools::READ_FILE, &json!({"path":"README.md"})));
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::EXEC_COMMAND,
        &json!({"cmd":"cargo check --locked"})
    ));
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::WRITE_FILE,
        &json!({"path":".vtcode/plans/next.md","content":"plan"})
    ));
    assert!(!mutation_blocked_until_verification(&tracker, tools::TASK_TRACKER, &json!({"action":"update"})));
}

#[test]
fn inspection_does_not_clear_mutations_waiting_for_verification() {
    let mut tracker = LoopTracker::new();
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;

    for command in ["git diff -- README.md", "git diff --check"] {
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd":command}));
    }

    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
}

#[test]
fn failed_verification_does_not_clear_mutations_waiting_for_verification() {
    let mut tracker = LoopTracker::new();
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    tracker.verification_pending = true;
    let failed_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 1}),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });

    update_repetition_tracker(&mut tracker, &failed_check, tools::EXEC_COMMAND, &json!({"cmd":"cargo check"}));

    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    // A failed verifier keeps the gate but opens a bounded fix-up window
    // so the broken build can be repaired instead of deadlocking.
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
    assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
}

#[test]
fn failed_verification_fix_window_is_consumed_by_repair_edits() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    let failed_check = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 1}),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });
    update_repetition_tracker(&mut tracker, &failed_check, tools::EXEC_COMMAND, &json!({"cmd":"cargo check"}));
    assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);

    let edit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for _ in 0..FAILED_VERIFICATION_FIX_ALLOWANCE {
        assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
        update_repetition_tracker(&mut tracker, &edit, tools::EDIT_FILE, &json!({"path": "src/lib.rs"}));
        assert!(tracker.verification_is_pending());
    }
    // Window exhausted: further mutations block again until a standalone
    // verifier succeeds.
    assert_eq!(tracker.fix_edits_remaining, 0);
    assert!(mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
}

#[test]
fn filtered_verifier_success_clears_gate_with_pipefail() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    // The kernel enables pipefail, so an observed exit 0 verifies every stage.
    assert!(!mutation_blocked_until_verification(
        &tracker,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked 2>&1 | grep error"})
    ));
    let piped_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(
        &mut tracker,
        &piped_success,
        tools::EXEC_COMMAND,
        &json!({"cmd": "cargo check --locked 2>&1 | grep error"}),
    );
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_mutations, 0);
}

#[test]
fn unsafe_verifier_aliases_and_suffixes_cannot_bypass_or_clear_pending_gate() {
    for args in [
        json!({"cmd":"cargo check | grep marker", "raw_command":"printf marker"}),
        json!({"cmd":"cargo check | sort", "args":["-o", "changed.txt"]}),
        json!({"cmd":"set -o pipefail && cargo check | sort", "args":["-o", "changed.txt"]}),
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        assert!(mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &args), "{args}");
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code":0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &args);
        assert!(tracker.verification_is_pending(), "{args}");
        assert_eq!(tracker.fix_edits_remaining, 0, "{args}");
    }
}

#[test]
fn markdownlint_verification_clears_only_on_truthful_success() {
    for (command, exit_code, clears_gate) in [
        ("npx --yes markdownlint-cli2@0.23.3 README.md", 0, true),
        ("npx --yes markdownlint-cli2@0.23.3 README.md", 1, false),
        ("npx --yes markdownlint-cli2@0.23.3 README.md | grep error", 0, true),
        ("python3 scripts/check_markdown.py", 0, true),
        ("python3 scripts/check_markdown.py", 1, false),
        ("python3 scripts/check_markdown.py --fix", 0, false),
        ("python3 scripts/check_markdown.py --list", 0, false),
        ("python3 scripts/check_markdown.py | grep error", 0, true),
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code": exit_code}),
            stdout: None,
            modified_files: vec![],
            command_success: exit_code == 0,
        });
        update_repetition_tracker(&mut tracker, &outcome, tools::EXEC_COMMAND, &json!({"cmd": command}));
        assert_eq!(!tracker.verification_is_pending(), clears_gate, "{command}, exit {exit_code}");
        if exit_code != 0 {
            assert_eq!(tracker.fix_edits_remaining, FAILED_VERIFICATION_FIX_ALLOWANCE);
        }
    }
}

#[test]
fn truncation_only_piped_verifier_success_clears_gate() {
    // The kernel elides a pure `| head`/`| tail` tail and runs the
    // standalone verifier, so its exit 0 is the verifier's own. The
    // tracker must agree even when handed the typed (raw) arguments,
    // e.g. after a PreToolUse hook rewrite.
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for command in [
        "cargo check --locked 2>&1 | tail -5",
        "cargo check --locked 2>&1 | head -c 4000",
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        assert!(
            !mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "{command}"
        );
        assert!(
            !update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "{command}"
        );
        assert!(!tracker.verification_is_pending(), "elided verifier success must clear: {command}");
        assert_eq!(tracker.consecutive_mutations, 0, "{command}");
        assert!(!tracker.take_piped_verification_notice(), "no piped notice for {command}");
    }
}

#[test]
fn anti_blind_editing_directive_states_the_shared_shell_form_note() {
    assert!(ANTI_BLIND_EDITING_DIRECTIVE.ends_with(vtcode_core::tools::tool_intent::VERIFIER_SHELL_FORM_NOTE));
    assert!(!PIPED_VERIFICATION_DIRECTIVE.contains("`tail`/`head`"));
}

#[test]
fn smuggled_mutation_behind_verifier_prefix_stays_blocked() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    for command in [
        "cargo check && rm -rf target",
        "cargo check; rm foo.txt",
        "cargo check --locked && cargo test && rm foo.txt",
    ] {
        assert!(
            mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "smuggled mutation must stay blocked: {command}"
        );
    }
}

#[test]
fn pure_and_chained_verifiers_are_admitted_and_clear_gate() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    tracker.verification_result_lost_notice_pending = true;
    tracker.piped_verification_notice_pending = true;
    tracker.verifier_failed_notice_pending = true;
    for command in [
        "cargo fmt --all -- --check && cargo check --locked",
        "cargo check --locked && cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked && cargo clippy --locked -p vtcode-ui -- -D warnings",
    ] {
        assert!(
            !mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "pure && verifier chain must be admitted: {command}"
        );
    }

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
    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.take_verification_result_lost_notice());
    assert!(!tracker.take_piped_verification_notice());
    assert!(!tracker.take_verifier_failed_notice());
}

#[test]
fn non_and_chained_verifiers_do_not_clear_gate() {
    for command in [
        "cargo check --locked; cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked || cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked | grep -v warning || true",
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        let chained_success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &chained_success, tools::EXEC_COMMAND, &json!({"cmd": command}));
        assert!(tracker.verification_is_pending(), "`;`/`||`/`|` chains must not clear the gate: {command}");
    }
}

#[test]
fn gate_trips_on_sixth_consecutive_code_mutation_not_fifth() {
    let mut tracker = LoopTracker::new();
    let edit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for _ in 0..(BLIND_EDITING_THRESHOLD - 1) {
        update_repetition_tracker(&mut tracker, &edit, tools::EDIT_FILE, &json!({"path": "src/lib.rs"}));
    }
    assert!(!tracker.verification_is_pending());
    assert!(!mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
    update_repetition_tracker(&mut tracker, &edit, tools::EDIT_FILE, &json!({"path": "src/lib.rs"}));
    assert!(tracker.verification_is_pending());
    assert!(mutation_blocked_until_verification(&tracker, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
}

#[test]
fn docs_only_writes_stay_allowed_and_do_not_increment_counter() {
    let mut tracker = LoopTracker::new();
    let docs_edit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for _ in 0..BLIND_EDITING_THRESHOLD {
        update_repetition_tracker(
            &mut tracker,
            &docs_edit,
            tools::WRITE_FILE,
            &json!({"path": "README.md", "content": "prose"}),
        );
    }
    assert_eq!(tracker.consecutive_mutations, 0);
    assert!(!tracker.verification_is_pending());

    let mut pending = LoopTracker::with_verification_snapshot((true, 0));
    pending.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    update_repetition_tracker(&mut pending, &docs_edit, tools::EDIT_FILE, &json!({"path": "docs/guide.md"}));
    assert!(pending.verification_is_pending());
    assert_eq!(pending.consecutive_mutations, BLIND_EDITING_THRESHOLD);
    assert!(!mutation_blocked_until_verification(&pending, tools::EDIT_FILE, &json!({"path": "docs/guide.md"})));
    assert!(mutation_blocked_until_verification(&pending, tools::EDIT_FILE, &json!({"path": "src/lib.rs"})));
}

#[test]
fn expanded_verifiers_clear_gate_while_mutating_lookalikes_do_not() {
    for command in [
        "bun test",
        "deno lint",
        "make test",
        "just lint",
        "ruff check src/",
        "tsc --noEmit",
        "eslint src/",
        "python3 -m pytest",
        "uv run pytest",
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        assert!(
            !mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "verifier must be admitted: {command}"
        );
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd": command}));
        assert!(!tracker.verification_is_pending(), "verifier must clear the gate: {command}");
    }
    for command in [
        "make clean",
        "make test clean",
        "tsc",
        "eslint --fix src/",
        "ruff format src/",
        "bun install",
    ] {
        let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
        assert!(
            mutation_blocked_until_verification(&tracker, tools::EXEC_COMMAND, &json!({"cmd": command})),
            "mutating lookalike must stay blocked: {command}"
        );
    }
}

#[test]
fn docs_only_boundary_cases_fail_closed() {
    let pending = LoopTracker::with_verification_snapshot((true, 0));
    // Code under docs/ stays a code edit.
    for path in [
        "docs/script.py",
        "docs/app.ts",
        "README.py",
        "readme_script.py",
        "LICENSE-MIT",
    ] {
        assert!(
            mutation_blocked_until_verification(&pending, tools::EDIT_FILE, &json!({"path": path})),
            "code-looking path must stay blocked: {path}"
        );
        assert!(!is_docs_only_write(tools::EDIT_FILE, &json!({"path": path})), "{path}");
    }
    // Prose spellings stay allowed, including camelCase `filePath`
    // (supplemented: `mutation_target_paths` lacks that key).
    for args in [
        json!({"path": "README"}),
        json!({"path": "README.md"}),
        json!({"path": "CHANGELOG.rst"}),
        json!({"path": "LICENSE"}),
        json!({"path": "docs/guide.md"}),
        json!({"filePath": "docs/guide.md"}),
    ] {
        assert!(!mutation_blocked_until_verification(&pending, tools::EDIT_FILE, &args), "{args}");
        assert!(is_docs_only_write(tools::EDIT_FILE, &args), "{args}");
    }
    // Exec-tool mutations never qualify, even for prose paths.
    assert!(!is_docs_only_write(tools::EXEC_COMMAND, &json!({"cmd": "echo hi > README.md"})));
}

#[test]
fn verification_completion_after_a_later_edit_cannot_clear_the_checkpoint() {
    let mut tracker = LoopTracker::new();
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD - 1;
    tracker.pending_verifier_session_id = Some("old-check".into());
    tracker.record_successful_mutation();
    assert!(tracker.verification_is_pending());
    let terminal = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: json!({"exit_code":0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(
        &mut tracker,
        &terminal,
        tools::WRITE_STDIN,
        &json!({"session_id":"old-check","chars":""}),
    );
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
}
