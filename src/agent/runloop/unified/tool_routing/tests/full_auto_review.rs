use super::*;

#[tokio::test]
async fn matching_ask_rule_prompts_user() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        ask: vec!["Write(/docs/**)".to_string()],
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_approval_policy(AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }))
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn matching_deny_rule_denies_without_classifier_review() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::UNIFIED_EXEC.to_string()],
        deny: vec![tools::UNIFIED_EXEC.to_string()],
        ..PermissionsConfig::default()
    };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Auto);
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        1,
        "deny rule should not consume the classifier response"
    );
}
#[tokio::test]
async fn full_auto_active_agent_ask_routes_to_auto_review_without_prompt() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.enable_full_auto_permission(&[tools::UNIFIED_EXEC.to_string()]).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        0,
        "active-agent Ask should be reviewed automatically in full-auto"
    );
}
#[tokio::test]
async fn full_auto_reviewer_error_blocks_without_prompt_fallback() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.enable_full_auto_permission(&[tools::UNIFIED_EXEC.to_string()]).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);
    let mut provider = ErrorProvider;
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    match flow {
        ToolPermissionFlow::Blocked { reason } => {
            assert!(reason.contains("full-auto permission review is active"));
        }
        other => panic!("expected blocked flow, got {other:?}"),
    }
}
#[tokio::test]
async fn full_auto_allowlist_does_not_bypass_tool_policy_deny() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.enable_full_auto_permission(&[tools::UNIFIED_EXEC.to_string()]).await;
    registry
        .set_tool_policy(tools::UNIFIED_EXEC, ToolPolicy::Deny)
        .await
        .expect("persist deny policy");
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        1,
        "tool-policy denials should not consume classifier responses"
    );
}
#[tokio::test]
async fn full_auto_allowlist_does_not_bypass_explicit_deny_rule() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.enable_full_auto_permission(&[tools::UNIFIED_EXEC.to_string()]).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        deny: vec![tools::UNIFIED_EXEC.to_string()],
        ..PermissionsConfig::default()
    };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        1,
        "explicit permission denials should not consume classifier responses"
    );
}
