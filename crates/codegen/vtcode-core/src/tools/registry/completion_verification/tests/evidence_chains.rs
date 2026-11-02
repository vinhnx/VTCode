use super::*;

#[tokio::test]
async fn failed_verifier_execution_releases_attempt_without_passing_or_reopening_retry() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec!["cargo check".into()]);
    let args = json!({"cmd":"cargo check"});

    let launch = registry.begin_completion_verifier(&args).expect("required check is tracked");
    registry.observe_completion_verifier(Some(launch), &args, &Err(anyhow::anyhow!("execution failed")));
    assert!(registry.completion_verification_pending(), "an execution error has no passing terminal verdict");
    let generation = registry
        .completion_verifier_repeat_recovery_generation(tools::EXEC_COMMAND, &args)
        .expect("a failed attempt with no live session permits the one bounded recovery");
    assert!(registry.consume_completion_verifier_repeat_recovery(tools::EXEC_COMMAND, &args, generation));

    let retry = registry.begin_completion_verifier(&args).expect("recovery attempt is tracked");
    registry.observe_completion_verifier(Some(retry), &args, &Err(anyhow::anyhow!("recovery failed")));
    assert!(registry.completion_verification_pending(), "the failed recovery cannot clear the gate");
    assert_eq!(
        registry.completion_verifier_repeat_recovery_generation(tools::EXEC_COMMAND, &args),
        None,
        "the failed recovery cannot reopen its one-use allowance"
    );
}
#[test]
fn completion_verification_bounds_superseded_chain_evidence_per_requirement() {
    let mut state = CompletionVerification::default();
    state.mutate();
    state.required_commands = vec!["cargo check".into(), "cargo clippy".into()];
    let both = state.launch("cargo check && cargo clippy".into());
    state.observe(both, &json!({"exit_code":0}));
    let live = state.launch("cargo check && printf 'live'".into());
    state.observe(live, &json!({"session_id":"older-live-chain"}));
    for index in 0..64 {
        let id = state.launch(format!("cargo check && printf 'check-{index}'"));
        state.observe(id, &json!({"exit_code":0}));
        assert!(!state.pending(), "the older chain still owns the clippy verdict");
        assert!(state.evidence.len() <= OPTIONAL_EVIDENCE_LIMIT + 3);
    }
    assert_eq!(state.evidence.len(), OPTIONAL_EVIDENCE_LIMIT + 3);
    assert!(state.evidence.iter().any(|entry| entry.identity == live));
    state.observe(live, &json!({"exit_code":7}));
    assert_eq!(state.evidence.len(), OPTIONAL_EVIDENCE_LIMIT + 2);
    assert!(!state.pending(), "the older live failure cannot override the latest check verdict");
    let latest = state.launch("cargo check && printf 'latest'".into());
    state.observe(latest, &json!({"exit_code":7}));
    assert!(state.pending(), "an older pass cannot override the latest failure");
    assert!(state.command_passed("cargo clippy"));
    assert!(!state.command_passed("cargo check"));
}
#[tokio::test]
async fn completion_verification_resumes_required_segments_of_live_chains() {
    for exit_code in [0, 7] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.restore_completion_verification_debt();
        registry.set_required_completion_verifiers(vec!["cargo check".into(), "cargo clippy".into()]);
        let args = json!({"cmd":"cargo check && cargo clippy"});
        let launch = registry.begin_completion_verifier(&args);
        assert!(launch.is_some());
        registry.observe_completion_verifier(launch, &args, &Ok(json!({"session_id":"owned-chain"})));
        for required in ["cargo check", "cargo clippy"] {
            assert_eq!(registry.pending_completion_verifier_session(required).as_deref(), Some("owned-chain"));
            assert!(!registry.completion_verifier_passed(required));
        }
        registry.observe_completion_verifier(
            None,
            &json!({"action":"wait", "session_id":"owned-chain"}),
            &Ok(json!({"session_id":"owned-chain", "exit_code":exit_code})),
        );
        assert_eq!(registry.completion_verification_pending(), exit_code != 0);
        for required in ["cargo check", "cargo clippy"] {
            assert!(registry.pending_completion_verifier_session(required).is_none());
            assert_eq!(registry.completion_verifier_passed(required), exit_code == 0);
        }
    }
}
#[test]
fn completion_verification_retains_required_checks_under_optional_evidence_pressure() {
    for required_count in [1, 31, 32, 33, 40] {
        let mut state = CompletionVerification::default();
        state.mutate();
        state.required_commands = (0..required_count)
            .map(|index| format!("cargo check --package package_{index}"))
            .collect();
        for command in state.required_commands.clone() {
            let id = state.launch(command);
            state.observe(id, &json!({"exit_code":0}));
        }
        assert!(!state.pending());
        for index in 0..64 {
            let id = state.launch(format!("pytest optional_{index}"));
            state.observe(id, &json!({"exit_code":7}));
            assert!(!state.pending(), "optional check {index} must not erase {required_count} required passes");
        }
        assert_eq!(state.evidence.len(), required_count + 32);
        assert!(state.evidence.iter().any(|entry| entry.command == "pytest optional_32"));
        assert!(!state.evidence.iter().any(|entry| entry.command == "pytest optional_31"));
        state.mutate();
        assert!(state.pending(), "retained required passes must still become stale after edits");
    }
}
#[tokio::test]
async fn completion_verification_unpins_old_requirements_and_preserves_pipeline_identity() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    let commands: Vec<_> = (0..40)
        .map(|index| format!("cargo check --package package_{index} 2>&1 | tail -5"))
        .collect();
    registry.set_required_completion_verifiers(commands.clone());
    for command in &commands {
        let args = json!({"action":"run", "command":command});
        let id = registry.begin_completion_verifier(&args);
        assert!(id.is_some());
        registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    }
    let args = json!({"action":"run", "command":"pytest optional"});
    let id = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    assert!(!registry.completion_verification_pending());
    assert!(registry.completion_verifier_passed(&commands[0]));
    registry.begin_tracker_request(false);
    let state = registry.harness_context.completion_verification.lock();
    assert_eq!(state.evidence.len(), 32, "released requirements return to the ordinary evidence bound");
    assert_eq!(state.evidence.back().unwrap().command, "pytest optional");
}
#[tokio::test]
async fn completion_verification_reports_the_exact_remaining_check() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec!["cargo check".into(), "grep -n '^## ' README.md".into()]);
    let args = json!({"cmd":"cargo check"});
    let launch = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"exit_code":0})));
    let reason = registry.unverified_completion_reason();
    assert!(reason.contains("Next required tracker check: `grep -n '^## ' README.md`"), "{reason}");
    assert!(reason.contains("a different passing check does not satisfy it"), "{reason}");
    assert!(registry.completion_verification_pending());
}
#[tokio::test]
async fn completion_verification_npx_logging_does_not_invalidate_required_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec!["cargo check".into()]);
    let args = json!({"cmd":"cargo check"});
    let launch = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"exit_code":0})));
    for (command, pending) in [
        ("npx --yes --loglevel=error markdownlint-cli2@0.23.3 README.md", false),
        ("npx --yes --loglevel=error markdownlint-cli2@0.23.3 --fix README.md", true),
    ] {
        registry.observe_completion_mutation(tools::EXEC_COMMAND, &json!({"cmd":command}), &json!({"exit_code":0}));
        assert_eq!(registry.completion_verification_pending(), pending, "{command}");
        assert_eq!(registry.completion_verifier_passed("cargo check"), !pending, "{command}");
    }
}
