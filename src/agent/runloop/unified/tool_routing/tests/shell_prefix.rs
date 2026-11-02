use super::*;

#[tokio::test]
async fn shell_approval_prefix_rules_persist_to_workspace_config() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "sandbox_permissions": "require_escalated",
    });

    let rendered = persist_shell_approval_prefix_rule(
        &registry,
        "exec_command",
        Some(&args),
        &["cargo".to_string(), "test".to_string()],
    )
    .await
    .expect("persist approval prefix");
    assert_eq!(rendered, "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null");

    let saved = ConfigManager::load_from_workspace(temp_dir.path()).expect("reload workspace config");
    assert!(saved.config().commands.approval_prefixes.iter().any(|entry| entry == &rendered));
    assert!(shell_command_has_persisted_approval_prefix(
        &registry,
        &[
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            "vtcode".to_string()
        ],
        "sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    ));
    assert!(
        registry
            .find_persisted_shell_approval_prefix(
                &[
                    "cargo".to_string(),
                    "test".to_string(),
                    "-p".to_string(),
                    "vtcode".to_string()
                ],
                "sandbox_permissions=\"require_escalated\"|additional_permissions=null",
            )
            .await
            .is_some()
    );
}
#[tokio::test]
async fn shell_approval_prefix_matching_respects_token_boundaries() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager = ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config.commands.approval_prefixes.push("echo hi".to_string());
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &["echo".to_string(), "history".to_string()],
        "sandbox_permissions=\"use_default\"|additional_permissions=null"
    ));
}
#[tokio::test]
async fn shell_approval_prefix_matching_respects_permission_scope() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager = ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config
        .commands
        .approval_prefixes
        .push("cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null".to_string());
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &[
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            "vtcode".to_string()
        ],
        "sandbox_permissions=\"with_additional_permissions\"|additional_permissions={\"fs_write\":[\"/tmp/demo.txt\"]}"
    ));
}
#[tokio::test]
async fn legacy_unscoped_shell_prefixes_do_not_match_escalated_runs() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager = ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config.commands.approval_prefixes.push("echo hi".to_string());
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &["echo".to_string(), "hi".to_string()],
        "sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    ));
}
