use super::*;

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
