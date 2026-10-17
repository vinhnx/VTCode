use super::*;
use serde_json::json;

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

#[test]
fn completion_verification_required_yielded_check_survives_eviction_and_supersession() {
    let mut state = CompletionVerification::default();
    state.mutate();
    state.required_commands = vec!["cargo check 2>&1 | tail -5".into()];
    let running = state.launch("cargo check 2>&1".into());
    state.observe(running, &json!({"session_id":"required-check"}));
    for index in 0..64 {
        let id = state.launch(format!("pytest optional_{index}"));
        state.observe(id, &json!({"exit_code":0}));
    }
    assert!(state.pending(), "optional passes cannot satisfy the yielded requirement");
    assert!(
        state
            .evidence
            .iter()
            .any(|entry| entry.session_id.as_deref() == Some("required-check"))
    );
    state.observe(running, &json!({"exit_code":0}));
    assert!(!state.pending());
    let replacement = state.launch("cargo check 2>&1".into());
    state.observe(running, &json!({"exit_code":0}));
    assert!(state.pending(), "superseded evidence cannot replace the newest attempt");
    state.observe(replacement, &json!({"exit_code":7}));
    assert!(state.pending());
    assert_eq!(state.evidence.len(), 33, "one required check plus 32 optional checks");
}

#[tokio::test]
async fn completion_verification_preserves_debt_across_compaction_and_ignores_noop_and_prose() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    for (path, output) in [
        ("README.md", json!({"success":true})),
        (".vtcode/plans/plan.md", json!({"success":true})),
        ("src/main.rs", json!({"success":false,"error":"conflict"})),
        ("src/noop.rs", json!({"success":true,"diff_preview":{"is_empty":true}})),
    ] {
        registry.observe_completion_mutation(tools::WRITE_FILE, &json!({"path":path}), &output);
        assert!(!registry.completion_verification_pending());
    }
    registry.observe_completion_mutation(tools::WRITE_FILE, &json!({"path":"src/main.rs"}), &json!({"success":true}));
    assert!(registry.completion_verification_pending());
    registry.reconcile_source_inspection(&[]);
    assert!(registry.completion_verification_pending());
}

#[tokio::test]
async fn completion_verification_unrelated_lost_cancelled_and_cleanup_sessions_cannot_pass() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    let args = json!({"action":"run", "command":"cargo check"});
    let launch = registry.begin_completion_verifier(&args);
    assert!(launch.is_some());
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"session_id":"owned"})));
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait", "session_id":"other"}),
        &Ok(json!({"exit_code":0})),
    );
    assert!(registry.completion_verification_pending());
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait", "session_id":"owned"}),
        &Ok(json!({"cancelled":true, "exit_code":0})),
    );
    assert!(registry.completion_verification_pending());
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait", "session_id":"owned"}),
        &Ok(json!({"exit_code":0})),
    );
    assert!(registry.completion_verification_pending());
    let launch = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"session_id":"fresh"})));
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait", "session_id":"fresh"}),
        &Ok(json!({"exit_code":0})),
    );
    assert!(!registry.completion_verification_pending());
}

#[tokio::test]
async fn completion_verification_runtime_evidence_survives_source_projection_clearing_in_both_modes() {
    use crate::llm::provider::{Message, ToolCall};
    for mode in [
        vtcode_config::core::agent::InspectionMode::Warn,
        vtcode_config::core::agent::InspectionMode::Enforce,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.set_inspection_mode(mode);
        registry.allow_all_tools().await.unwrap();
        let patch = json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"});
        let output = registry.execute_tool(tools::APPLY_PATCH, patch).await.unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert!(registry.completion_verification_pending());
        let check = registry
            .execute_harness_command_session(json!({"action":"run", "command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(check["exit_code"], 0, "{check}");
        assert!(!registry.completion_verification_pending());
        registry.reconcile_source_inspection(&[]);
        assert!(!registry.completion_verification_pending());
        let failed_args = json!({"action":"run", "command":"cargo check"});
        let launch = registry.begin_completion_verifier(&failed_args);
        registry.observe_completion_verifier(launch, &failed_args, &Ok(json!({"exit_code":7})));
        assert!(registry.completion_verification_pending());
        let generation_before_repair = registry.harness_context.completion_verification.lock().generation;
        let repair =
            json!({"path":"main.rs","old_str":"fn main() {}","new_str":"fn main() { println!(\"changed\"); }"});
        let rejected_or_advisory = registry.execute_tool(tools::EDIT_FILE, repair.clone()).await.unwrap();
        if mode == vtcode_config::core::agent::InspectionMode::Enforce {
            assert!(rejected_or_advisory.get("error").is_some(), "{rejected_or_advisory}");
            assert!(registry.completion_verification_pending(), "the failed verifier remains pending");
            assert_eq!(
                registry.harness_context.completion_verification.lock().generation,
                generation_before_repair,
                "rejected edit adds no debt"
            );
            let args = json!({"path":"main.rs"});
            let read = registry.execute_tool(tools::READ_FILE, args.clone()).await.unwrap();
            registry.reconcile_source_inspection(&[
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "read".into(),
                        tools::READ_FILE.into(),
                        args.to_string(),
                    )],
                ),
                Message::tool_response("read".into(), read.to_string()),
            ]);
            let output = registry.execute_tool(tools::EDIT_FILE, repair).await.unwrap();
            assert!(output.get("error").is_none(), "{output}");
        } else {
            assert!(rejected_or_advisory.get("error").is_none(), "{rejected_or_advisory}");
        }
        assert!(registry.completion_verification_pending());
        assert_eq!(registry.harness_context.completion_verification.lock().generation, generation_before_repair + 1);
    }
}

#[tokio::test]
async fn completion_verification_restore_rejects_historical_pass_and_cleared_results() {
    use crate::llm::provider::{Message, ToolCall};
    for mutation in [json!({"success":true}), json!({"cleared":"tool_result"})] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.restore_completion_verification_from_history(&[
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "edit".into(),
                    tools::WRITE_FILE.into(),
                    json!({"path":"main.rs"}).to_string(),
                )],
            ),
            Message::tool_response("edit".into(), mutation.to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "check".into(),
                    tools::EXEC_COMMAND.into(),
                    json!({"cmd":"rustc --version"}).to_string(),
                )],
            ),
            Message::tool_response("check".into(), json!({"exit_code":0}).to_string()),
        ]);
        assert!(registry.completion_verification_pending());
    }
}

#[tokio::test]
async fn completion_verification_lost_and_closed_sessions_have_no_late_verdict() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    let launch_args = json!({"action":"run","command":"cargo check"});
    for cleanup in [false, true] {
        let id = registry.begin_completion_verifier(&launch_args);
        registry.observe_completion_verifier(id, &launch_args, &Ok(json!({"session_id":"check"})));
        registry.observe_completion_verifier(
            None,
            &json!({"action":"wait","session_id":"check"}),
            &Ok(json!({"session_id":"unrelated","exit_code":0})),
        );
        assert!(registry.completion_verification_pending());
        let (args, result) = if cleanup {
            (json!({"action":"close","session_id":"check"}), Ok(json!({"success":true})))
        } else {
            (json!({"action":"wait","session_id":"check"}), Err(anyhow::anyhow!("exec session not found")))
        };
        registry.observe_completion_verifier(None, &args, &result);
        registry.observe_completion_verifier(
            None,
            &json!({"action":"wait","session_id":"check"}),
            &Ok(json!({"exit_code":0})),
        );
        assert!(registry.completion_verification_pending());
    }
}

#[tokio::test]
async fn completion_verification_waits_for_mutating_sessions_before_accepting_checks() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.observe_completion_mutation(
        tools::EXEC_COMMAND,
        &json!({"cmd":"cargo fmt"}),
        &json!({"session_id":"formatter"}),
    );
    let args = json!({"action":"run","command":"cargo check"});
    let id = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    assert!(registry.completion_verification_pending(), "formatter is still active");
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait","session_id":"formatter"}),
        &Ok(json!({"exit_code":0})),
    );
    assert!(registry.completion_verification_pending(), "formatter completion invalidates the earlier check");
    let id = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(id, &args, &Ok(json!({"exit_code":0})));
    assert!(!registry.completion_verification_pending());
}

#[cfg(all(unix, feature = "tui"))]
#[tokio::test]
async fn completion_verification_bulk_termination_reconciles_only_gone_sessions() {
    use crate::tools::pty::PtySize;
    use hashbrown::HashMap;
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    // Spawn sessions under the registry's canonical workspace root: on macOS
    // the tempdir path goes through the `/var` → `/private/var` symlink and
    // the session guard rejects non-canonical working dirs.
    let workspace = registry.workspace_root().clone();
    let manager = registry.exec_session_manager();
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let spawn = |id: &str| vec!["/bin/sh".to_string(), "-c".to_string(), format!("sleep 30 # {id}")];
    manager
        .create_pty_session(
            "foreground-mutator".to_string().into(),
            spawn("foreground-mutator"),
            workspace.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await
        .unwrap();
    manager
        .create_pty_session_for_managed_background(
            "background-mutator".to_string().into(),
            spawn("background-mutator"),
            workspace.clone(),
            size,
            HashMap::new(),
            None,
            HashMap::new(),
            false,
        )
        .await
        .unwrap();
    for session in ["foreground-mutator", "background-mutator"] {
        registry.observe_completion_mutation(
            tools::EXEC_COMMAND,
            &json!({"cmd":"cargo fmt"}),
            &json!({"session_id":session}),
        );
    }
    assert!(registry.completion_verification_pending());
    registry.terminate_active_exec_sessions_async().await.unwrap();
    let live: Vec<String> = manager
        .list_sessions()
        .await
        .into_iter()
        .map(|session| session.id.to_string())
        .collect();
    assert!(!live.contains(&"foreground-mutator".to_string()), "foreground session must be gone: {live:?}");
    assert!(live.contains(&"background-mutator".to_string()), "background session is preserved: {live:?}");
    assert!(registry.completion_verification_pending(), "the preserved background mutator still owns the gate");
    registry.terminate_all_exec_sessions_async().await.unwrap();
    assert!(
        registry.completion_verification_pending(),
        "terminated mutators release the session block, not the code debt"
    );
    let args = json!({"action":"run", "command":"cargo check"});
    let launch = registry.begin_completion_verifier(&args);
    assert!(launch.is_some());
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"exit_code":0})));
    assert!(
        !registry.completion_verification_pending(),
        "a fresh check after bulk termination must satisfy the gate"
    );
}

#[tokio::test]
async fn completion_verification_restored_mutators_create_debt_without_live_handles() {
    use crate::llm::provider::{Message, ToolCall};
    for archived_terminal in [None, Some(0), Some(7)] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        let mut history = vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "format".into(),
                    tools::EXEC_COMMAND.into(),
                    json!({"cmd":"cargo fmt"}).to_string(),
                )],
            ),
            Message::tool_response("format".into(), json!({"session_id":"archived-formatter"}).to_string()),
        ];
        if let Some(exit_code) = archived_terminal {
            history.extend([
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "wait".into(),
                        tools::WRITE_STDIN.into(),
                        json!({"action":"wait","session_id":"archived-formatter"}).to_string(),
                    )],
                ),
                Message::tool_response("wait".into(), json!({"exit_code":exit_code}).to_string()),
            ]);
        }
        registry.restore_completion_verification_from_history(&history);
        assert!(registry.completion_verification_pending());
        assert!(
            registry
                .harness_context
                .completion_verification
                .lock()
                .pending_mutation_sessions
                .is_empty(),
            "archived sessions cannot be owned by this registry"
        );
        let fresh = registry
            .execute_harness_command_session(json!({"action":"run","command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(fresh["exit_code"], 0, "{fresh}");
        assert!(!registry.completion_verification_pending());
    }
}

#[tokio::test]
async fn completion_verification_code_targets_cannot_use_prose_or_planning_exemptions() {
    for path in ["CMakeLists.txt", "requirements.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let check = registry
            .execute_harness_command_session(json!({"action":"run", "command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(check["exit_code"], 0, "{check}");
        let output = registry
            .execute_tool(tools::WRITE_FILE, json!({"path":path, "content":"changed\n"}))
            .await
            .unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(std::fs::read_to_string(dir.path().join(path)).unwrap(), "changed\n");
        assert!(registry.completion_verification_pending(), "code target {path}: {output}");
        assert!(!registry.completion_verifier_passed("rustc --version"));
    }
}

#[test]
fn completion_verification_temp_plans_root_matches_real_temp_dir() {
    use crate::tools::tool_intent::tmp_plans_dir;
    let root = tmp_plans_dir();
    let root_str = root.to_str().expect("temp plans root must be UTF-8").to_owned();
    for path in [
        root_str.clone(),
        format!("{root_str}/draft.md"),
        format!("{root_str}/workspace-slug/draft.md"),
    ] {
        assert!(
            tool_intent::is_plan_artifact_write(tools::WRITE_FILE, &json!({"path": path})),
            "{path} must classify as a plan artifact under {}",
            root.display()
        );
    }
}

#[test]
fn completion_verification_exemptions_require_all_normalized_targets() {
    for args in [
        json!({"action":"move", "path":".vtcode/plans/draft.rs", "destination":"src/main.rs"}),
        json!({"action":"write", "path":"src/main.rs", "filePath":".vtcode/plans/draft.rs"}),
        json!({"action":"write", "path":".vtcode/plans/../../src/main.rs"}),
    ] {
        assert!(!tool_intent::is_plan_artifact_write(tools::UNIFIED_FILE, &args), "{args}");
    }
    assert!(tool_intent::is_plan_artifact_write(tools::WRITE_FILE, &json!({"path":".vtcode/plans/draft.md"})));
    assert!(tool_intent::is_docs_only_write(tools::WRITE_FILE, &json!({"path":"notes.txt"})));
}

#[cfg(unix)]
#[tokio::test]
async fn completion_verification_uses_effective_targets_for_prose_and_plan_symlinks() {
    for alias in [".vtcode/plans/alias.rs", "notes.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let code = dir.path().join("main.rs");
        std::fs::write(&code, "original\n").unwrap();
        let link = dir.path().join(alias);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&code, &link).unwrap();
        let check = registry
            .execute_harness_command_session(json!({"action":"run", "command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(check["exit_code"], 0, "{check}");
        let output = registry
            .execute_tool(tools::WRITE_FILE, json!({"path":alias, "content":"changed\n", "overwrite":true}))
            .await
            .unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(std::fs::read_to_string(&code).unwrap(), "changed\n");
        assert!(registry.completion_verification_pending(), "effective code target via {alias}: {output}");
    }
}

#[tokio::test]
async fn completion_verification_exempt_file_results_preserve_existing_debt() {
    for debt in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        if debt {
            registry.restore_completion_verification_debt();
        }
        let patch = registry
            .execute_tool(
                tools::APPLY_PATCH,
                json!({
                    "input":"*** Begin Patch\n*** Add File: README.md\n+Documentation\n*** End Patch"
                }),
            )
            .await
            .unwrap();
        assert!(patch.get("error").is_none(), "{patch}");
        assert_eq!(registry.completion_verification_pending(), debt, "{patch}");
        for path in ["notes.txt", ".vtcode/plans/draft.rs"] {
            let output = registry
                .execute_tool(tools::WRITE_FILE, json!({"path":path, "content":"prose\n"}))
                .await
                .unwrap();
            assert!(output.get("error").is_none(), "{output}");
            assert_eq!(registry.completion_verification_pending(), debt, "{output}");
        }
    }
}

#[tokio::test]
async fn completion_verification_required_pipelines_match_executed_evidence() {
    for command in ["rustc --version 2>&1 | tail -5", "rustc --version 2>&1 | grep rustc"] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        registry.restore_completion_verification_debt();
        let tracker = registry
            .execute_tool(
                tools::TASK_TRACKER,
                json!({
                    "action":"create", "title":"Verification", "items":[{
                        "description":"Check toolchain", "status":"completed", "verify":[command]
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(tracker.get("error").is_none(), "{tracker}");
        assert!(registry.tracker_adopted_for_request());
        assert_eq!(registry.next_required_completion_verifier().as_deref(), Some(command));
        let result = registry
            .execute_tool(tools::EXEC_COMMAND, json!({"cmd":command}))
            .await
            .unwrap();
        assert_eq!(result["exit_code"], 0, "{result}");
        assert!(registry.completion_verifier_passed(command));
        assert!(registry.missing_required_completion_verifiers().is_empty());
        assert!(registry.next_required_completion_verifier().is_none());
        assert!(!registry.completion_verification_pending());
    }
}

#[tokio::test]
async fn completion_verification_pipeline_wait_identity_and_failure_stay_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.restore_completion_verification_debt();
    let command = "cargo check 2>&1 | tail -5";
    registry.set_required_completion_verifiers(vec![command.into()]);
    let args = json!({"action":"run", "command":command});
    let launch = registry.begin_completion_verifier(&args);
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"session_id":"pipeline"})));
    assert_eq!(registry.pending_completion_verifier_session(command).as_deref(), Some("pipeline"));
    registry.observe_completion_verifier(
        None,
        &json!({"action":"wait", "session_id":"pipeline"}),
        &Ok(json!({"exit_code":7})),
    );
    assert!(!registry.completion_verifier_passed(command));
    assert!(registry.completion_verification_pending());
}

#[cfg(unix)]
#[tokio::test]
async fn completion_verification_declared_custom_checks_do_not_invalidate_their_verdicts() {
    use std::os::unix::fs::PermissionsExt;
    for exit_code in [0, 7] {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("verify.sh");
        std::fs::write(&script, format!("#!/bin/sh\nprintf 'custom-check-evidence\\n'\nexit {exit_code}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let mut commands = registry.commands_config();
        commands.allow_list.push("./verify.sh".into());
        registry.apply_commands_config(&commands);
        registry.restore_completion_verification_debt();
        let tracker = registry
            .execute_tool(
                tools::TASK_TRACKER,
                json!({
                    "action":"create", "title":"Custom verification", "items":[{
                        "description":"Check custom invariant", "status":"completed", "verify":["./verify.sh"]
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(tracker.get("error").is_none(), "{tracker}");
        let generation = registry.harness_context.completion_verification.lock().generation;
        let result = registry
            .execute_tool(tools::EXEC_COMMAND, json!({"cmd":"./verify.sh"}))
            .await
            .unwrap();
        assert_eq!(result["exit_code"], exit_code, "{result}");
        assert_eq!(registry.harness_context.completion_verification.lock().generation, generation);
        assert_eq!(registry.completion_verifier_passed("./verify.sh"), exit_code == 0);
        assert_eq!(registry.completion_verification_pending(), exit_code != 0);
        if exit_code == 0 {
            commands.deny_list.push("./verify.sh".into());
            registry.apply_commands_config(&commands);
            registry.observe_completion_mutation(
                tools::WRITE_FILE,
                &json!({"path":"main.rs"}),
                &json!({"success":true}),
            );
            let denied = registry
                .execute_tool(tools::EXEC_COMMAND, json!({"cmd":"./verify.sh"}))
                .await
                .unwrap();
            assert_eq!(denied["error"]["error_type"], "PolicyViolation", "{denied}");
            assert!(denied.get("exit_code").is_none());
            assert!(registry.completion_verification_pending());
            assert!(!registry.completion_verifier_passed("./verify.sh"));
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn completion_verification_undeclared_custom_commands_still_create_mutation_debt() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("verify.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf done\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    let mut commands = registry.commands_config();
    commands.allow_list.push("./verify.sh".into());
    registry.apply_commands_config(&commands);
    let result = registry
        .execute_tool(tools::EXEC_COMMAND, json!({"cmd":"./verify.sh"}))
        .await
        .unwrap();
    assert_eq!(result["exit_code"], 0, "{result}");
    assert!(registry.completion_verification_pending());
    assert!(!registry.completion_verifier_passed("./verify.sh"));
}

#[tokio::test]
async fn completion_verification_accepts_empty_adapter_fields_but_rejects_effective_context() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    for env in [json!([]), json!({})] {
        let args = json!({"action":"run", "command":"cargo check", "args":[], "env":env});
        assert!(registry.begin_completion_verifier(&args).is_some(), "{args}");
    }
    for (field, value) in [
        ("args", json!(["--locked"])),
        ("args", Value::Null),
        ("args", json!("")),
        ("env", json!([{"name":"MODE", "value":"fixture"}])),
        ("env", json!({"MODE":"fixture"})),
        ("env", Value::Null),
        ("shell", json!("/bin/sh")),
        ("login", json!(false)),
        ("stdin", json!("")),
    ] {
        let mut args = json!({"action":"run", "command":"cargo check", "args":[], "env":[]});
        args[field] = value;
        assert!(registry.begin_completion_verifier(&args).is_none(), "context must not be dropped: {args}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn completion_verification_native_terminal_adapter_observes_yielded_verdicts() {
    use std::os::unix::fs::PermissionsExt;
    for exit_code in [0, 7] {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("rustc");
        std::fs::write(&script, format!("#!/bin/sh\nwhile [ ! -e release-check ]; do sleep 0.01; done\nprintf 'native-verified\\n'\nexit {exit_code}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        let mut commands = registry.commands_config();
        commands.allow_list.push("./rustc".into());
        registry.apply_commands_config(&commands);
        registry.restore_completion_verification_debt();
        // Match Copilot terminal_run_args, including its empty adapter defaults.
        let result = registry
            .execute_harness_command_session_terminal_run(json!({
                "action":"run", "command":"./rustc --version", "args":[], "cwd":null,
                "tty":true, "yield_time_ms":100, "env":[],
            }))
            .await
            .unwrap();
        let session_id = result["session_id"].as_str().expect("retained native session");
        assert!(result["exit_code"].is_null(), "the release file prevents early completion: {result}");
        assert!(registry.completion_verification_pending());
        assert_eq!(registry.pending_completion_verifier_session("./rustc --version").as_deref(), Some(session_id));
        std::fs::write(dir.path().join("release-check"), "release").unwrap();
        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(code) = registry.harness_exec_session_completed(session_id).await.unwrap() {
                    break code;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        registry.close_harness_exec_session(session_id).await.unwrap();
        assert_eq!(observed.unwrap(), exit_code);
        assert_eq!(registry.completion_verifier_passed("./rustc --version"), exit_code == 0);
        assert_eq!(registry.completion_verification_pending(), exit_code != 0);
    }
}

#[tokio::test]
async fn completion_verification_bulk_termination_releases_mutators_without_erasing_debt() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    for session in ["formatter-a", "formatter-b"] {
        registry.observe_completion_mutation(
            tools::EXEC_COMMAND,
            &json!({"cmd":"cargo fmt"}),
            &json!({"session_id":session}),
        );
    }
    assert!(registry.completion_verification_pending());
    // Terminating an unrelated session changes nothing.
    registry.observe_completion_exec_sessions_terminated(&["unrelated".to_owned()]);
    assert!(registry.completion_verification_pending());
    // Releasing one mutator leaves the other pending.
    registry.observe_completion_exec_sessions_terminated(&["formatter-a".to_owned()]);
    assert!(registry.completion_verification_pending());
    // Releasing the last mutator keeps code debt but unblocks fresh verification.
    registry.observe_completion_exec_sessions_terminated(&["formatter-b".to_owned()]);
    assert!(
        registry.completion_verification_pending(),
        "terminated mutators release the session block, not the code debt"
    );
    let args = json!({"action":"run", "command":"cargo check"});
    let launch = registry.begin_completion_verifier(&args);
    assert!(launch.is_some());
    registry.observe_completion_verifier(launch, &args, &Ok(json!({"exit_code":0})));
    assert!(
        !registry.completion_verification_pending(),
        "a fresh check after bulk termination must satisfy the gate"
    );
}

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
async fn required_inspection_repetition_limit_rejects_instead_of_replaying_a_verdict() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture.txt"), "original\n").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    registry.execution_history.set_loop_detection_limits(5, 2);
    let command = "grep -n original fixture.txt";
    let args = json!({"cmd":command});
    for _ in 0..2 {
        let result = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
        assert_eq!(result["exit_code"], 0);
    }
    registry.restore_completion_verification_debt();
    registry.set_required_completion_verifiers(vec![command.into()]);
    let result = registry.execute_tool(tools::EXEC_COMMAND, args).await.unwrap();
    assert_eq!(result["loop_detected"], true, "{result}");
    assert!(result["error"]["message"].as_str().unwrap().contains("Loop detection"), "{result}");
    assert!(result.get("exit_code").is_none());
    assert!(!registry.completion_verifier_passed(command));
    assert!(registry.completion_verification_pending());
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
