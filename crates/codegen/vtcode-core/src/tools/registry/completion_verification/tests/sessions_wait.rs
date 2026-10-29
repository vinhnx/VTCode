use super::*;

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
