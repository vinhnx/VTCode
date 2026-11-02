use super::*;

#[tokio::test]
async fn matching_allow_rule_allows_tool_call() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::READ_FILE.to_string()],
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}
#[tokio::test]
async fn pre_tool_use_hook_rewrite_reaches_approved_args() {
    use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};
    use vtcode_core::hooks::LifecycleHookEngine;

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::READ_FILE.to_string()],
        ..PermissionsConfig::default()
    };

    let hooks_config = HooksConfig {
        lifecycle: LifecycleHooksConfig {
            pre_tool_use: vec![HookGroupConfig {
                matcher: Some(tools::READ_FILE.into()),
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: r#"printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{"path":"src/lib.rs"}}}'"#.into(),
                    timeout_seconds: None,
                }],
            }],
            ..Default::default()
        },
    };
    let hooks = LifecycleHookEngine::new_with_session_gated(
        temp_dir.path().to_path_buf(),
        &hooks_config,
        vtcode_core::hooks::SessionStartTrigger::Startup,
        "test-session",
        false,
    )
    .expect("hook engine")
    .unwrap();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_hooks(&hooks)
            .with_permissions_config(&permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: Some(json!({"path": "src/lib.rs"})) });
}
#[tokio::test]
async fn pre_tool_use_hook_rewrite_survives_forwarded_hook_phase() {
    use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};
    use vtcode_core::hooks::LifecycleHookEngine;

    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::READ_FILE.to_string()],
        ..PermissionsConfig::default()
    };

    let hooks_config = HooksConfig {
        lifecycle: LifecycleHooksConfig {
            pre_tool_use: vec![HookGroupConfig {
                matcher: Some(tools::READ_FILE.into()),
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    // Fail closed on a second invocation: if the permission
                    // flow re-ran the hook phase, this returns exit 2 (deny)
                    // and the flow below would not be Approved.
                    command: format!(
                        "f=\"{}/marker\"; [ -f \"$f\" ] && exit 2; touch \"$f\"; exit 0",
                        temp_dir.path().display()
                    ),
                    timeout_seconds: None,
                }],
            }],
            ..Default::default()
        },
    };
    let hooks = LifecycleHookEngine::new_with_session_gated(
        temp_dir.path().to_path_buf(),
        &hooks_config,
        vtcode_core::hooks::SessionStartTrigger::Startup,
        "test-session",
        false,
    )
    .expect("hook engine")
    .unwrap();

    let rewritten = json!({"path": "docs/index.md"});
    let flow = crate::agent::runloop::unified::tool_routing::ensure_tool_permission_forwarded(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            // // The pipeline already ran the hook phase upstream; hooks here
            // // must not run again (command above would double-fire).
            .with_hooks(&hooks)
            .with_permissions_config(&permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
        Some("call-forwarded"),
        Some(PreToolHookPhaseResult::Proceed {
            rewritten_args: Some(rewritten.clone()),
            requires_prompt: false,
        }),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: Some(rewritten) });
}
