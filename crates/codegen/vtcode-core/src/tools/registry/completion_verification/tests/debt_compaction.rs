use super::*;

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
