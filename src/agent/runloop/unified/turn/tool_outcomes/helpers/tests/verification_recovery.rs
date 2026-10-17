use super::*;

#[test]
fn verification_snapshot_bundle_round_trips_without_drift() {
    let tracker = LoopTracker::with_verification_snapshot((true, FAILED_VERIFICATION_FIX_ALLOWANCE));
    assert_eq!(tracker.verification_snapshot(), (true, FAILED_VERIFICATION_FIX_ALLOWANCE));
    let cleared = LoopTracker::with_verification_snapshot((false, FAILED_VERIFICATION_FIX_ALLOWANCE));
    assert_eq!(cleared.verification_snapshot(), (false, 0));
}

#[test]
fn harness_verifier_override_must_be_standalone_or_pure_chain() {
    use super::resolve_harness_verifier_command;

    let dir = tempfile::TempDir::new().expect("workspace");
    let config_with = |override_command: &str| {
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.agent.harness.verification.default_verifier_override = Some(override_command.to_string());
        vt_cfg
    };

    // Valid overrides win over detection (empty dir detects nothing).
    let vt_cfg = config_with("cargo nextest run -p mycrate");
    assert_eq!(
        resolve_harness_verifier_command(Some(&vt_cfg), dir.path(), &[]).as_deref(),
        Some("cargo nextest run -p mycrate")
    );
    // Pure-`&&` verifier chains are truthful and accepted.
    let vt_cfg = config_with("cargo fmt --all -- --check && cargo check --locked");
    assert!(resolve_harness_verifier_command(Some(&vt_cfg), dir.path(), &[]).is_some());
    // Piped, joined, and mutating overrides fall back to detection, which
    // finds nothing here — never executing attacker- or typo-shaped text.
    for bad in [
        "cargo check --locked | tail -5",
        "cargo check; cargo test",
        "cargo check || cargo test",
        "rm -rf /tmp/scratch",
        "   ",
    ] {
        let vt_cfg = config_with(bad);
        assert_eq!(resolve_harness_verifier_command(Some(&vt_cfg), dir.path(), &[]), None, "must not resolve: {bad:?}");
    }
    // Fallback works when detection finds a marker: the override loses.
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo.toml");
    let vt_cfg = config_with("cargo check | tail -5");
    assert_eq!(
        resolve_harness_verifier_command(Some(&vt_cfg), dir.path(), &[]).as_deref(),
        Some("cargo check --locked")
    );
}

#[test]
fn harness_verifier_retries_recorded_failed_lint_for_docs_only_work() {
    let dir = tempfile::TempDir::new().expect("workspace");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo.toml");
    let lint = "npx --no-install markdownlint-cli2 README.md";
    let mut history = vec![
        uni::Message::user("fix README alignment".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "docs_1".to_string(),
                "write_file".to_string(),
                r#"{"path":"README.md","content":"text"}"#.to_string(),
            )],
        ),
        uni::Message::tool_response("docs_1".to_string(), r#"{"success":true}"#.to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "lint_1".to_string(),
                "exec_command".to_string(),
                serde_json::json!({"cmd": lint}).to_string(),
            )],
        ),
        uni::Message::tool_response("lint_1".to_string(), r#"{"exit_code":1,"stderr":"MD060"}"#.to_string()),
    ];
    assert_eq!(resolve_harness_verifier_command(None, dir.path(), &history).as_deref(), Some(lint));
    history.extend([
        uni::Message::user("continue".to_string()),
        uni::Message::assistant_with_tools(String::new(), vec![uni::ToolCall::function(
            "fix_1".to_string(), "apply_patch".to_string(), serde_json::json!({"input": "*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+new\n*** End Patch"}).to_string(),
        )]),
        uni::Message::tool_response("fix_1".to_string(), r#"{"success":true}"#.to_string()),
    ]);
    assert_eq!(resolve_harness_verifier_command(None, dir.path(), &history).as_deref(), Some(lint));
    let mut cfg = vtcode_core::config::loader::VTCodeConfig::default();
    cfg.agent.harness.verification.default_verifier_override = Some("cargo clippy --locked".to_string());
    assert_eq!(
        resolve_harness_verifier_command(Some(&cfg), dir.path(), &history).as_deref(),
        Some("cargo clippy --locked")
    );
    history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "code_1".to_string(),
            "write_file".to_string(),
            r#"{"path":"src/lib.rs","content":""}"#.to_string(),
        )],
    ));
    assert_eq!(
        resolve_harness_verifier_command(None, dir.path(), &history).as_deref(),
        Some("cargo check --locked")
    );
}

#[test]
fn harness_verifier_does_not_replay_unsafe_unfinished_or_previous_task_checks() {
    let dir = tempfile::TempDir::new().expect("workspace");
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo.toml");
    // Every execution-context field must survive replay or prevent reduction
    // to a command string, including supported directory aliases.
    for (key, value) in [
        ("working_dir", json!("docs")),
        ("cwd", json!("docs")),
        ("workdir", json!("docs")),
        ("working_directory", json!("docs")),
        ("shell", json!("/bin/bash")),
        ("login", json!(true)),
        ("env", json!({"RULES":"strict"})),
        ("stdin", json!(true)),
        ("tty", json!(true)),
        ("background", json!(true)),
        ("sandbox_permissions", json!("require_escalated")),
        ("additional_permissions", json!({"network":true})),
    ] {
        let mut args = json!({"cmd":"npx --no-install markdownlint-cli2 README.md"});
        args[key] = value;
        let history = vec![
            uni::Message::user("fix docs README".to_string()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "docs".to_string(),
                    "write_file".to_string(),
                    json!({"path":"docs/README.md", "content":"text"}).to_string(),
                )],
            ),
            uni::Message::tool_response("docs".to_string(), json!({"success":true}).to_string()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "lint".to_string(),
                    "exec_command".to_string(),
                    args.to_string(),
                )],
            ),
            uni::Message::tool_response("lint".to_string(), json!({"exit_code":1}).to_string()),
        ];
        assert_eq!(
            resolve_harness_verifier_command(None, dir.path(), &history).as_deref(),
            Some("cargo check --locked"),
            "lost context: {key}"
        );
    }
    for (args, output) in [
        (json!({"cmd":"npx --no-install markdownlint-cli2 README.md"}), json!({"exit_code":0})),
        (json!({"cmd":"npx --no-install markdownlint-cli2 README.md"}), json!({"status":"running"})),
        (
            json!({"cmd":"npx --no-install markdownlint-cli2 README.md"}),
            json!({"exit_code":1,"not_executed":true}),
        ),
        (json!({"cmd":"npx --no-install markdownlint-cli2 README.md"}), json!({"exit_code":1,"blocked":true})),
        (json!({"cmd":"npx --no-install markdownlint-cli2 README.md; echo ok"}), json!({"exit_code":1})),
        (json!({"cmd":"npx --no-install markdownlint-cli2 README.md | grep MD060"}), json!({"exit_code":1})),
        (json!({"cmd":"npx --no-install markdownlint-cli2", "args":["README.md"]}), json!({"exit_code":1})),
        (
            json!({"cmd":"npx --no-install markdownlint-cli2 README.md", "workdir":"other"}),
            json!({"exit_code":1}),
        ),
    ] {
        let history = vec![
            uni::Message::user("fix README alignment".to_string()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "docs_1".to_string(),
                    "write_file".to_string(),
                    r#"{"path":"README.md","content":"text"}"#.to_string(),
                )],
            ),
            uni::Message::tool_response("docs_1".to_string(), r#"{"success":true}"#.to_string()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "lint_1".to_string(),
                    "exec_command".to_string(),
                    args.to_string(),
                )],
            ),
            uni::Message::tool_response("lint_1".to_string(), output.to_string()),
        ];
        assert_eq!(
            resolve_harness_verifier_command(None, dir.path(), &history).as_deref(),
            Some("cargo check --locked"),
            "{args}/{output}"
        );
    }
    let mut history = vec![
        uni::Message::user("fix README".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "docs_1".to_string(),
                "write_file".to_string(),
                r#"{"path":"README.md","content":"text"}"#.to_string(),
            )],
        ),
        uni::Message::tool_response("docs_1".to_string(), r#"{"success":true}"#.to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "lint_1".to_string(),
                "exec_command".to_string(),
                r#"{"cmd":"npx --no-install markdownlint-cli2 README.md"}"#.to_string(),
            )],
        ),
        uni::Message::tool_response("unmatched".to_string(), r#"{"exit_code":1}"#.to_string()),
    ];
    assert_eq!(
        resolve_harness_verifier_command(None, dir.path(), &history).as_deref(),
        Some("cargo check --locked")
    );
    history.push(uni::Message::tool_response("lint_1".to_string(), r#"{"exit_code":1}"#.to_string()));
    history.push(uni::Message::user("fix the compiler error".to_string()));
    assert_eq!(
        resolve_harness_verifier_command(None, dir.path(), &history).as_deref(),
        Some("cargo check --locked")
    );
}

#[test]
fn verification_auto_recovery_budget_is_bounded_and_cleared_on_success() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    for _ in 0..MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS {
        assert!(tracker.record_verification_auto_recovery_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS));
    }
    assert!(!tracker.record_verification_auto_recovery_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS));
    assert_eq!(tracker.verification_auto_recovery_attempts(), MAX_VERIFICATION_AUTO_RECOVERY_ATTEMPTS);
    // A fresh turn starts with a fresh budget.
    let fresh = LoopTracker::with_verification_snapshot((true, 0));
    assert_eq!(fresh.verification_auto_recovery_attempts(), 0);

    // A successful standalone verifier clears the budget with the gate.
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd": "cargo check --locked"}));
    assert!(!tracker.verification_is_pending());
    assert_eq!(tracker.verification_auto_recovery_attempts(), 0);
}

#[test]
fn auto_execute_verifier_is_one_shot_per_turn_until_success() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 0));
    tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;
    assert!(tracker.should_auto_execute_verifier());

    tracker.record_auto_verification_executed();
    assert!(!tracker.should_auto_execute_verifier());

    // A failed verifier keeps the gate but does not re-arm execution.
    let failed = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 1}),
        stdout: None,
        modified_files: vec![],
        command_success: false,
    });
    update_repetition_tracker(&mut tracker, &failed, tools::EXEC_COMMAND, &json!({"cmd": "cargo check --locked"}));
    assert!(tracker.verification_is_pending());
    assert!(!tracker.should_auto_execute_verifier());

    // A fresh turn re-arms; success clears the flag with the gate.
    let fresh = LoopTracker::with_verification_snapshot((true, 0));
    assert!(fresh.should_auto_execute_verifier());
    let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    update_repetition_tracker(&mut tracker, &success, tools::EXEC_COMMAND, &json!({"cmd": "cargo check --locked"}));
    assert!(!tracker.verification_is_pending());
    assert!(!tracker.auto_verification_executed);
}
