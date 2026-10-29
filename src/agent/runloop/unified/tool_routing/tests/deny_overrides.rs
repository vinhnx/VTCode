use super::*;

#[tokio::test]
async fn skip_confirmations_does_not_bypass_cached_tool_denial() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    permission_cache
        .write()
        .await
        .cache_grant(tools::READ_FILE.to_string(), PermissionGrant::Denied);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_tool_permission_cache(&permission_cache)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(true)
            .build(),
        tools::READ_FILE,
        None,
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn tool_policy_deny_overrides_cached_session_approval() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .set_tool_policy(tools::READ_FILE, ToolPolicy::Deny)
        .await
        .expect("persist deny policy");

    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    permission_cache
        .write()
        .await
        .cache_grant(tools::READ_FILE.to_string(), PermissionGrant::Session);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_tool_permission_cache(&permission_cache)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn active_agent_deny_default_denies_non_allowed_requests() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn inline_request_user_input_bypasses_generic_permission_prompt() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(false)
            .build(),
        tools::REQUEST_USER_INPUT,
        Some(&json!({
            "questions": [{
                "id": "scope",
                "header": "Scope",
                "question": "Which scope should the plan cover?",
                "options": [{"label": "Focused", "description": "Keep the change narrow."}]
            }]
        })),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}
