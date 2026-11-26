use super::*;

#[tokio::test]
async fn full_auto_without_skip_confirmations_does_not_take_dangerous_skip_path() {
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
async fn skip_confirmations_bypasses_active_agent_ask_default() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig { ..PermissionsConfig::default() };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);

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
async fn skip_confirmations_does_not_bypass_explicit_deny_rule() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        deny: vec!["Write(/docs/**)".to_string()],
        ..PermissionsConfig::default()
    };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Allow);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(true)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn skip_confirmations_does_not_bypass_tool_policy_deny() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .set_tool_policy(tools::UNIFIED_FILE, ToolPolicy::Deny)
        .await
        .expect("persist deny policy");

    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(true)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn active_agent_allow_default_allows_builtin_file_mutations() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Allow);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }))
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "notes.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}
#[tokio::test]
async fn active_agent_allow_default_keeps_protected_write_prompts() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Allow);

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }))
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": ".vtcode/settings.toml", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
#[tokio::test]
async fn agent_overlay_allow_disarms_safety_prompt_under_skip_confirmations() {
    // Mirror of `skip_confirmations_does_not_bypass_safety_gateway_needs_approval`
    // with the fixed session-setup behavior: the active agent's overrides are
    // applied before the first tool call, so an explicit agent Allow disarms
    // the safety-gateway prompt and skip_confirmations auto-approves.
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig { ..PermissionsConfig::default() };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);

    let _ = registry.set_tool_policy(tools::UNIFIED_EXEC, ToolPolicy::Prompt).await;
    registry
        .apply_runtime_tool_policy_overrides(&BTreeMap::from([(tools::UNIFIED_EXEC.to_string(), ToolPolicy::Allow)]))
        .await;

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(true)
            .with_safety_approval_justification(Some("Destructive operation: rm -rf".to_string()))
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "rm -rf /tmp/test"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}

#[tokio::test]
async fn build_exec_overlay_auto_approves_flagged_shell_command() {
    // Production name path: Build's override map is exactly
    // `{"exec_command": Allow}`; a safety-flagged shell command under
    // Auto-approve must not prompt.
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig { ..PermissionsConfig::default() };
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);

    registry
        .apply_runtime_tool_policy_overrides(&BTreeMap::from([(tools::EXEC_COMMAND.to_string(), ToolPolicy::Allow)]))
        .await;

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_approval_policy(reject_all_approvals())
            .with_skip_confirmations(true)
            .with_safety_approval_justification(Some(
                "Tool: exec_command\nRisk level: Medium\nCommand: osascript -e 'tell application \"Calendar\"'"
                    .to_string(),
            ))
            .build(),
        tools::EXEC_COMMAND,
        Some(&json!({"cmd": "osascript -e 'tell application \"Calendar\" to get name of every calendar'"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}

#[tokio::test]
async fn protected_write_prompts_without_matching_allow() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig { ..PermissionsConfig::default() };

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
        Some(&json!({"action": "write", "path": ".vtcode/settings.toml", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}
