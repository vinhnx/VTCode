use super::*;

#[cfg(unix)]
#[tokio::test]
async fn required_inspections_bypass_primed_results_and_record_actual_verdicts() {
    for (command, expected_exit) in [
        ("rg -n original fixture.txt", 1),
        ("grep -n original fixture.txt", 1),
        ("grep -n '---\\s\\+\\|' fixture.txt", 2),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fixture.txt"), "original\n").unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let mut commands = registry.commands_config();
        commands.allow_list.push("rg".into());
        registry.apply_commands_config(&commands);
        let args = json!({"cmd":command, "yield_time_ms":10000});
        let first = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
        assert_eq!(first["exit_code"], if expected_exit == 2 { 2 } else { 0 }, "{command}: {first}");
        std::fs::write(dir.path().join("fixture.txt"), "changed\n").unwrap();
        registry.restore_completion_verification_debt();
        registry.set_required_completion_verifiers(vec![command.into()]);
        let result = registry.execute_tool(tools::EXEC_COMMAND, args).await.unwrap();
        assert_eq!(result["exit_code"], expected_exit, "{command}: {result}");
        assert_ne!(result["reused_recent_result"], true);
        assert!(!registry.completion_verifier_passed(command));
        assert!(registry.completion_verification_pending());
    }
}
#[tokio::test]
async fn inspection_accounting_preserves_generation_but_concrete_changes_invalidate_passes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec!["cargo check".into()]);
    let args = json!({"cmd":"cargo check"});
    let id = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    for command in [
        "for p in $(grep -o 'x' README.md | sort -u); do test -f \"$p\" || echo \"missing: $p\"; done",
        "grep x README.md; echo \"exit:$?\"",
        "rg -n \"MD013|line_length\" docs/development/*.md | head",
    ] {
        registry.observe_completion_mutation(tools::EXEC_COMMAND, &json!({"cmd":command}), &json!({"exit_code":0}));
        assert!(registry.completion_verifier_passed("cargo check"), "{command}");
    }
    registry.observe_completion_mutation(
        tools::EXEC_COMMAND,
        &json!({"cmd":"grep x README.md"}),
        &json!({"exit_code":0, "modified_files":["src/lib.rs"]}),
    );
    assert!(!registry.completion_verifier_passed("cargo check"));
}
#[cfg(unix)]
#[tokio::test]
async fn repeated_required_check_gets_one_fresh_execution_per_mutation_generation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture.rs"), "initial fixture\n").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    registry.execution_history.set_loop_detection_limits(5, 2);
    let command = "cat fixture.rs";
    let args = json!({"cmd":command});
    for _ in 0..2 {
        let result = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
        assert_eq!(result["exit_code"], 0);
    }

    let write = registry
        .execute_tool(tools::WRITE_FILE, json!({"path":"fixture.rs", "content":"updated fixture\n", "overwrite":true}))
        .await
        .unwrap();
    assert!(write.get("error").is_none(), "{write}");
    assert!(registry.completion_verification_pending());
    registry.set_required_completion_verifiers(vec![command.into()]);

    let fresh = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
    assert_eq!(fresh["exit_code"], 0, "{fresh}");
    assert!(fresh.to_string().contains("updated fixture"), "the run must observe current contents: {fresh}");
    assert_ne!(fresh["reused_recent_result"], true, "required checks must execute instead of reusing output");
    assert!(registry.completion_verifier_passed(command));
    assert!(!registry.completion_verification_pending());
    {
        let state = registry.harness_context.completion_verification.lock();
        let evidence = state
            .evidence
            .iter()
            .rev()
            .find(|entry| entry.command == command)
            .expect("the terminal execution must record verifier evidence");
        assert_eq!(evidence.generation, state.generation);
        assert_eq!(evidence.exit_code, Some(0));
    }

    let repeated = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
    assert_eq!(repeated["loop_detected"], true, "{repeated}");
    assert!(repeated.get("exit_code").is_none(), "further identical calls stay blocked");

    let write = registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({"path":"fixture.rs", "content":"second mutation fixture\n", "overwrite":true}),
        )
        .await
        .unwrap();
    assert!(write.get("error").is_none(), "{write}");
    assert!(registry.completion_verification_pending());
    let after_mutation = registry.execute_tool(tools::EXEC_COMMAND, args).await.unwrap();
    assert_eq!(after_mutation["exit_code"], 0, "{after_mutation}");
    assert!(
        after_mutation.to_string().contains("second mutation fixture"),
        "the new generation must execute the required check again: {after_mutation}"
    );
    assert_ne!(after_mutation["reused_recent_result"], true);
    assert!(registry.completion_verifier_passed(command));
    assert!(!registry.completion_verification_pending());
}
#[tokio::test]
async fn docs_session_path_glob_does_not_activate_a_malformed_tracker_verifier() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.set_required_completion_verifiers(vec![
        "rg -n \"brew trust\" README.md && ! rg -n \"brew tap\" README.md".into(),
    ]);
    registry.observe_completion_mutation(
        tools::APPLY_PATCH,
        &json!({"input":"*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+new\n*** End Patch"}),
        &json!({"success":true,"modified_files":["README.md"]}),
    );
    registry.observe_completion_mutation(
        tools::EXEC_COMMAND,
        &json!({"cmd":"rg -n \"MD013|line_length\" docs/development/*.md | head"}),
        &json!({"exit_code":0}),
    );
    assert_eq!(registry.harness_context.completion_verification.lock().generation, 0);
    assert!(!registry.completion_verification_pending());
    assert_eq!(
        registry.next_required_completion_verifier().as_deref(),
        Some("rg -n \"brew trust\" README.md && ! rg -n \"brew tap\" README.md")
    );
}
#[tokio::test]
async fn declared_source_repair_cannot_clear_completion_verification() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    for command in [
        "cargo clippy --fix",
        "ruff check --fix-only .",
        "uv run --locked ruff check --fix .",
        "echo before && cargo clippy --fix",
    ] {
        registry.restore_completion_verification_debt();
        registry.set_required_completion_verifiers(vec![command.into()]);
        let args = json!({"cmd":command});
        let id = registry.begin_completion_verifier(&args);
        assert!(id.is_none(), "repair cannot launch a verification attempt: {command}");
        let generation = registry.harness_context.completion_verification.lock().generation;
        registry.observe_completion_mutation(tools::EXEC_COMMAND, &args, &json!({"exit_code":0}));
        assert_eq!(registry.harness_context.completion_verification.lock().generation, generation + 1, "{command}");
        registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
        assert!(registry.completion_verification_pending(), "{command}");
        assert_eq!(
            registry.next_required_completion_verifier().as_deref(),
            Some(command),
            "requirements must not be silently replaced"
        );
    }
    registry.set_required_completion_verifiers(vec!["cargo clippy --locked".into()]);
    let args = json!({"cmd":"cargo clippy --locked"});
    let id = registry.begin_completion_verifier(&args);
    assert!(id.is_some());
    registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    assert!(!registry.completion_verification_pending());
}
#[tokio::test]
async fn required_local_obligations_reuse_both_passes_and_reject_stale_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.set_required_completion_verifiers(vec!["rustc --version".into(), "rustc --help".into()]);
    assert!(!registry.completion_verification_pending());
    registry.restore_completion_verification_debt();
    assert!(registry.completion_verification_pending());
    let first = serde_json::json!({"cmd":"rustc --version"});
    let launch = registry.begin_completion_verifier(&first);
    registry.observe_completion_verifier(launch, &first, &Ok(serde_json::json!({"exit_code":0})));
    assert!(registry.completion_verification_pending());
    assert!(!registry.has_current_completion_verification());
    let second = serde_json::json!({"cmd":"rustc --help"});
    let launch = registry.begin_completion_verifier(&second);
    registry.observe_completion_verifier(launch, &second, &Ok(serde_json::json!({"exit_code":0})));
    assert!(!registry.completion_verification_pending());
    assert!(registry.has_current_completion_verification());
    registry.observe_completion_mutation(
        tools::WRITE_FILE,
        &json!({"path":"src/lib.rs"}),
        &json!({"success":true,"modified_files":["src/lib.rs"]}),
    );
    assert!(!registry.has_current_completion_verification(), "new mutation makes the previous checks stale");
}
#[tokio::test]
async fn docs_only_failed_or_running_checker_is_not_hidden_by_an_earlier_pass() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let first = json!({"cmd":"rustc --version"});
    let launch = registry.begin_completion_verifier(&first);
    registry.observe_completion_verifier(launch, &first, &Ok(json!({"exit_code":0})));
    assert!(registry.has_current_completion_verification());
    let second = json!({"cmd":"rustc --help"});
    let launch = registry.begin_completion_verifier(&second);
    registry.observe_completion_verifier(launch, &second, &Ok(json!({"session_id":"new-checker"})));
    assert!(!registry.has_current_completion_verification());
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait","session_id":"new-checker"}),
        &Ok(json!({"exit_code":1})),
    );
    assert!(!registry.has_current_completion_verification());
}
#[tokio::test]
async fn completion_verification_unwraps_backticked_tracker_verify_commands() {
    // Regression for session-vtcode-20261019T075619Z_862610-57915: a tracker
    // `verify` entry persisted as a markdown code span seeded a required
    // command whose identity carried the backticks, so the model's bare
    // successful run never satisfied `command_passed` — while the backticked
    // literal needed to match it was itself rejected by preflight (code spans
    // are command substitution). The exact command then deadlocked the gate
    // for the rest of the session.
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec![
        "`./scripts/update-contributors-readme.sh --dry-run`".into(),
        "  cargo check  ".into(),
    ]);
    // Stored requirements are executable identities (no code spans, trimmed).
    assert_eq!(
        registry.missing_required_completion_verifiers(),
        vec![
            "./scripts/update-contributors-readme.sh --dry-run".to_string(),
            "cargo check".to_string()
        ]
    );
    // The model executes the bare command (the only form preflight admits).
    let args = json!({"action":"run", "command":"./scripts/update-contributors-readme.sh --dry-run"});
    let launch = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"exit_code":0})));
    assert!(
        registry.completion_verifier_passed("./scripts/update-contributors-readme.sh --dry-run"),
        "a successful bare run must satisfy the backtick-wrapped requirement"
    );
    assert!(registry.completion_verification_pending(), "one required check intentionally remains: cargo check");
    assert_eq!(registry.missing_required_completion_verifiers(), vec!["cargo check"]);
}
#[tokio::test]
async fn webmcp_inspection_trace_preserves_generation_and_fresh_passes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    let inspections = [
        r#"grep -rn "default_webmcp\|WebmcpConfig" crates/codegen/vtcode-config/src --include=*.rs | head -10"#,
        "grep -rn WebMCP src --exclude-dir=target* --exclude=*.lock",
        "sed -n '1,100p' docs/development/webmcp.md",
    ];
    for with_pass in [false, true] {
        if with_pass {
            registry.restore_completion_verification_debt();
            let args = json!({"cmd":"cargo check --locked"});
            let id = registry.begin_completion_verifier(&args);
            registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
        }
        let generation = registry.completion_mutation_generation();
        for command in inspections {
            registry.observe_completion_mutation(tools::EXEC_COMMAND, &json!({"cmd":command}), &json!({"exit_code":0}));
            assert_eq!(registry.completion_mutation_generation(), generation, "{command}");
            assert!(!registry.completion_verification_pending(), "{command}");
        }
        if with_pass {
            assert!(registry.completion_verifier_passed("cargo check --locked"));
        }
    }
    registry.observe_completion_mutation(
        tools::APPLY_PATCH,
        &json!({}),
        &json!({"success":true,"modified_files":["src/main.rs"]}),
    );
    assert!(registry.completion_verification_pending());
    assert!(!registry.completion_verifier_passed("cargo check --locked"));
}
