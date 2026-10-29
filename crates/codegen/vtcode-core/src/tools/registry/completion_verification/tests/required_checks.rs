use super::*;

#[tokio::test]
async fn required_inspections_accept_successful_and_chains_without_masked_status_or_context_changes() {
    for (command, passes) in [
        ("sed -n '35,90p' README.md && sed -n '90,240p' README.md", true),
        ("sed -n '35,90p' README.md; true", false),
        ("sed -n '35,90p' README.md || true", false),
        ("! sed -n '35,90p' README.md && true", false),
        ("! (false && sed -n '35,90p' README.md) && true", false),
        ("sed -n '35,90p' README.md | cat", false),
        ("cd other && sed -n '35,90p' README.md", false),
        ("sed -n '35,90p' README.md && rm README.md", false),
        ("echo \"sed -n '35,90p' README.md\" && true", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.restore_completion_verification_debt();
        registry.set_required_completion_verifiers(vec!["sed -n '35,90p' README.md".into()]);
        let args = json!({"cmd":command});
        let id = registry.begin_completion_verifier(&args);
        registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
        assert_eq!(registry.completion_verification_pending(), !passes, "{command}");
        if passes {
            for index in 0..40 {
                let optional = json!({"cmd":format!("cargo check -p optional_{index}")});
                let id = registry.begin_completion_verifier(&optional);
                registry.observe_completion_verifier(id, &optional, &Ok(json!({"exit_code":1})));
            }
            assert!(!registry.completion_verification_pending(), "required chain evidence must stay pinned");
            registry.restore_completion_verification_debt();
        }
    }
}
#[tokio::test]
async fn completion_verification_bulk_status_sync_retains_required_checks() {
    for planning in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        registry.restore_completion_verification_debt();
        if planning {
            let state = registry.planning_workflow_state();
            let plan_file = state.plans_dir().join("verification.md");
            std::fs::create_dir_all(plan_file.parent().unwrap()).unwrap();
            std::fs::write(&plan_file, "# Verification\n").unwrap();
            state.set_plan_file(Some(plan_file)).await;
            state.enable();
            registry.enable_planning();
        }
        registry
            .execute_tool(
                tools::TASK_TRACKER,
                json!({"action":"create", "items":[
                    {"description":"Check build", "verify":["cargo check"]},
                    {"description":"Check lints", "verify":["cargo clippy"]}
                ]}),
            )
            .await
            .unwrap();
        let args = json!({"action":"update", "items":["[x] Check lints", "[x] Check build"]});
        let output = registry.execute_tool(tools::TASK_TRACKER, args.clone()).await.unwrap();
        assert_eq!(output["status"], "updated", "{output}");
        assert_eq!(registry.missing_required_completion_verifiers(), vec!["cargo clippy", "cargo check"]);
        let verifier_args = json!({"cmd":"cargo check"});
        let id = registry.begin_completion_verifier(&verifier_args);
        registry.observe_completion_verifier(id, &verifier_args, &Ok(json!({"exit_code":0})));
        assert_eq!(registry.missing_required_completion_verifiers(), vec!["cargo clippy"]);
        registry.begin_tracker_request(false);
        assert!(!registry.tracker_adopted_for_request());
        let repeated = registry.execute_tool(tools::TASK_TRACKER, args).await.unwrap();
        assert_eq!(repeated["status"], "unchanged", "{repeated}");
        assert!(registry.tracker_adopted_for_request(), "unchanged live updates adopt the new request");
        assert_eq!(registry.missing_required_completion_verifiers(), vec!["cargo clippy"]);
    }
}
#[test]
fn completion_verification_requires_fresh_terminal_evidence_after_one_five_and_six_edits() {
    for edits in [1, 5, 6] {
        let mut state = CompletionVerification::default();
        assert!(!state.pending());
        for _ in 0..edits {
            state.mutate();
        }
        assert!(state.pending());
        let verifier = state.launch("cargo check".into());
        state.observe(verifier, &json!({"session_id":"check-a"}));
        assert!(state.pending());
        state.observe(verifier, &json!({"exit_code":0}));
        assert!(!state.pending());
        state.mutate();
        assert!(state.pending());
    }
}
#[test]
fn completion_verification_rejects_stale_and_superseded_checks() {
    let mut state = CompletionVerification::default();
    let old = state.launch("pytest".into());
    state.mutate();
    state.observe(old, &json!({"exit_code":0}));
    assert!(state.pending());
    let current = state.launch("pytest".into());
    state.observe(current, &json!({"exit_code":7}));
    assert!(state.pending());
    state.observe(old, &json!({"exit_code":0}));
    assert!(state.pending());
}
#[test]
fn completion_verification_all_required_tracker_checks_must_pass() {
    let mut state = CompletionVerification::default();
    state.mutate();
    state.required_commands = vec!["cargo check".into(), "cargo clippy".into()];
    for (command, pending) in [("pytest", true), ("cargo check", true), ("cargo clippy", false)] {
        let id = state.launch(command.into());
        state.observe(id, &json!({"exit_code":0}));
        assert_eq!(state.pending(), pending);
    }
}
#[test]
fn repeated_required_check_recovery_is_exact_bounded_and_generation_scoped() {
    let mut state = CompletionVerification::default();
    state.mutate();
    state.required_commands = vec!["cargo check".into()];
    let first_generation = state.generation;

    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), Some(first_generation));
    assert_eq!(
        state.repeated_required_command_recovery_generation("cargo check --locked"),
        None,
        "the exception applies only to the exact required command"
    );

    let active = state.launch("cargo check".into());
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);
    state.observe(active, &json!({"session_id":"check-running"}));
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);
    state.observe(active, &json!({"exit_code":1}));
    assert!(state.pending(), "a terminal failure does not satisfy the required check");
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);

    state.mutate();
    let second_generation = state.generation;
    assert!(second_generation > first_generation);
    assert!(!state.consume_repeated_required_command_recovery("cargo check", first_generation));
    assert!(state.consume_repeated_required_command_recovery("cargo check", second_generation));
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);
    state.required_commands.clear();
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);
    state.required_commands.push("cargo check".into());
    assert_eq!(
        state.repeated_required_command_recovery_generation("cargo check"),
        None,
        "tracker requirement updates do not replenish a generation's spent admission"
    );

    state.mutate();
    let third_generation = state.generation;
    assert_eq!(
        state.repeated_required_command_recovery_generation("cargo check"),
        Some(third_generation),
        "a new mutation generation opens one fresh admission"
    );
    let passing = state.launch("cargo check".into());
    state.observe(passing, &json!({"exit_code":0}));
    assert!(!state.pending());
    assert_eq!(state.repeated_required_command_recovery_generation("cargo check"), None);
}
