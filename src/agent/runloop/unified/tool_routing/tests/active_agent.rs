use super::*;

#[tokio::test]
async fn active_agent_auto_rule_enters_classifier_review() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .set_tool_policy(tools::UNIFIED_EXEC, ToolPolicy::Prompt)
        .await
        .expect("set prompt policy");
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig {
        default: PermissionDefault::Ask,
        auto: vec!["Bash(*)".to_string()],
        ..AgentPermissionsConfig::new(PermissionDefault::Ask)
    };
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("inspect the project".to_string())];
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

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
    registry
        .execute_tool(tools::UNIFIED_EXEC, json!({"action": "run", "command": "echo hi"}))
        .await
        .expect("an auto-approved call must reach execution");
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        0,
        "auto rule should consume the classifier response"
    );
}
#[tokio::test]
async fn active_agent_permissions_restrict_global_direct_allow() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::UNIFIED_FILE.to_string()],
        ..PermissionsConfig::default()
    };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "notes.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn subagent_local_permissions_restrict_global_direct_allow() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        allow: vec![tools::UNIFIED_FILE.to_string()],
        ..PermissionsConfig::default()
    };
    let subagent_permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&subagent_permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "notes.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn active_agent_ask_allows_skip_confirmations_auto_approval() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig { ..PermissionsConfig::default() };
    let agent_permissions = AgentPermissionsConfig {
        default: PermissionDefault::Allow,
        ask: vec!["Write(/docs/**)".to_string()],
        ..AgentPermissionsConfig::new(PermissionDefault::Allow)
    };

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(true)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}
#[tokio::test]
async fn skip_confirmations_auto_approves_without_active_agent_policy() {
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
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(true)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}
