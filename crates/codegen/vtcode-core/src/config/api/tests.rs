use super::*;

use std::sync::Arc;

use serial_test::serial;
use vtcode_commons::reference::StaticWorkspacePaths;
use vtcode_config::defaults::WorkspacePathsDefaults;
use vtcode_config::defaults::provider::with_config_defaults_provider_for_test;
use vtcode_config::loader::set_explicit_config_path;

#[test]
#[serial]
fn read_returns_layers_and_origins() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let home_config = workspace.join("home").join("vtcode.toml");
    let workspace_config = workspace.join("vtcode.toml");
    fs::create_dir_all(home_config.parent().expect("home parent")).expect("home dir");

    fs::write(&home_config, "agent.provider = \"openai\"\n").expect("home config");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\nagent.default_model = \"claude-sonnet-4-6\"\n")
        .expect("workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![home_config]);

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::read(ConfigReadRequest {
            workspace: workspace.to_path_buf(),
            runtime_overrides: Vec::new(),
        })
        .expect("read response");

        assert!(!response.layers.is_empty());
        assert!(!response.merged_version.is_empty());
        assert!(response.origins.contains_key("agent.provider"));
    });
}

#[test]
#[serial]
fn write_reports_override_when_higher_layer_wins() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let home_config = workspace.join("home").join("vtcode.toml");
    let workspace_config = workspace.join("vtcode.toml");
    fs::create_dir_all(home_config.parent().expect("home parent")).expect("home dir");

    fs::write(&home_config, "agent.provider = \"openai\"\n").expect("home config");
    fs::write(&workspace_config, "agent.provider = \"gemini\"\n").expect("workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![home_config]);

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::User,
            path: "agent.provider".to_string(),
            value: TomlValue::String("anthropic".to_string()),
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: None,
        })
        .expect("write response");

        assert_eq!(response.effective_value, Some(TomlValue::String("gemini".to_string())));
        assert!(response.overridden_metadata.is_some());
    });
}

#[test]
#[serial]
fn repository_target_write_does_not_persist_provider_endpoint_or_credentials() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let home_config = workspace.join("home/vtcode.toml");
    let workspace_config = workspace.join("vtcode.toml");
    fs::create_dir_all(home_config.parent().expect("home parent")).expect("home dir");

    fs::write(
        &home_config,
        "[provider_overrides.openai]\nbase_url = \"https://trusted.example/v1\"\napi_key_env = \"TRUSTED_API_KEY\"\n",
    )
    .expect("home config");
    fs::write(&workspace_config, "agent.provider = \"openai\"\n").expect("workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(vec![home_config])
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::Workspace,
            path: "provider_overrides.openai.base_url".to_string(),
            value: TomlValue::String("https://attacker.example/v1".to_string()),
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: None,
        })
        .expect("repository write should complete");

        assert_eq!(response.effective_value, Some(TomlValue::String("https://trusted.example/v1".to_string())));
        let workspace_content = fs::read_to_string(&workspace_config).expect("workspace config");
        assert!(!workspace_content.contains("attacker.example"));
        assert!(!workspace_content.contains("base_url"));
        assert!(!workspace_content.contains("api_key_env"));
    });
}

#[test]
#[serial]
fn user_writes_target_the_canonical_path_not_the_legacy_fallback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let legacy_path = workspace.join("legacy").join("vtcode.toml");
    let canonical_path = workspace.join("xdg").join("vtcode.toml");
    fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("legacy dir");
    fs::write(&legacy_path, "agent.provider = \"openai\"\n").expect("legacy config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(vec![legacy_path.clone(), canonical_path.clone()]);

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::User,
            path: "agent.provider".to_string(),
            value: TomlValue::String("anthropic".to_string()),
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: None,
        })
        .expect("write response");

        assert!(canonical_path.exists(), "the canonical user config should be created");
        assert_eq!(fs::read_to_string(&legacy_path).expect("legacy config"), "agent.provider = \"openai\"\n");
    });
}

#[test]
#[serial]
fn write_rejects_stale_expected_version() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let workspace_config = workspace.join("vtcode.toml");
    fs::write(&workspace_config, "agent.provider = \"openai\"\n\n[workspace]\nuse_root_config = true\n")
        .expect("workspace config");

    let response = ConfigService::write(ConfigWriteRequest {
        workspace: workspace.to_path_buf(),
        target: ConfigWriteTarget::Workspace,
        path: "agent.provider".to_string(),
        value: TomlValue::String("anthropic".to_string()),
        strategy: ConfigWriteStrategy::Replace,
        expected_layer_version: Some("stale-version".to_string()),
    });

    assert!(response.is_err());
    let error = format!("{:#}", response.expect_err("expected stale version error"));
    assert!(error.contains("Layer version mismatch"));
}

#[test]
#[serial]
fn reset_workspace_layer_preserves_lower_precedence_user_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let user_config = workspace.join("home").join("vtcode.toml");
    let workspace_config = workspace.join("vtcode.toml");
    fs::create_dir_all(user_config.parent().expect("user parent")).expect("user dir");
    fs::write(&user_config, "agent.provider = \"openai\"\n").expect("user config");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\n").expect("workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![user_config]);

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::reset(ConfigResetRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::Workspace,
            expected_layer_version: None,
            path: None,
        })
        .expect("workspace reset");

        assert_eq!(response.target, ConfigWriteTarget::Workspace);
        assert!(response.had_file);
        assert!(fs::read_to_string(&workspace_config).expect("reset file").trim().is_empty());

        let effective: VTCodeConfig = serde_json::from_value(response.effective_config).expect("effective config");
        assert_eq!(effective.agent.provider, "openai");
    });
}

#[test]
#[serial]
fn reset_canonical_user_layer_preserves_legacy_user_layer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path();
    let legacy_config = workspace.join("legacy").join("vtcode.toml");
    let canonical_config = workspace.join("canonical").join("vtcode.toml");
    fs::create_dir_all(legacy_config.parent().expect("legacy parent")).expect("legacy dir");
    fs::create_dir_all(canonical_config.parent().expect("canonical parent")).expect("canonical dir");
    fs::write(&legacy_config, "agent.provider = \"openai\"\n").expect("legacy config");
    fs::write(&canonical_config, "agent.provider = \"anthropic\"\n").expect("canonical config");

    let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(vec![legacy_config.clone(), canonical_config.clone()]);

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::reset(ConfigResetRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::User,
            expected_layer_version: None,
            path: None,
        })
        .expect("user reset");

        assert_eq!(response.path, canonical_config);
        assert!(canonical_config.exists());
        assert_eq!(fs::read_to_string(&legacy_config).expect("legacy config"), "agent.provider = \"openai\"\n");
        let effective: VTCodeConfig = serde_json::from_value(response.effective_config).expect("effective config");
        assert_eq!(effective.agent.provider, "openai");
    });
}

#[test]
#[serial]
fn reset_project_layer_preserves_workspace_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    let config_dir = workspace.join(".vtcode");
    let project_dir = config_dir.join("projects").join("demo").join("config");
    fs::create_dir_all(&project_dir).expect("project config dir");
    fs::write(workspace.join(".vtcode-project"), "demo\n").expect("project marker");
    fs::write(workspace.join("vtcode.toml"), "agent.provider = \"openai\"\n").expect("workspace config");
    let project_config = project_dir.join("vtcode.toml");
    fs::write(&project_config, "agent.default_model = \"project-model\"\n").expect("project config");

    let static_paths = StaticWorkspacePaths::new(&workspace, &config_dir);
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::reset(ConfigResetRequest {
            workspace: workspace.clone(),
            target: ConfigWriteTarget::Project,
            expected_layer_version: None,
            path: None,
        })
        .expect("project reset");

        assert_eq!(response.target, ConfigWriteTarget::Project);
        assert_eq!(response.path, project_config);
        assert!(
            fs::read_to_string(&project_config)
                .expect("reset project file")
                .trim()
                .is_empty()
        );
        let effective: VTCodeConfig = serde_json::from_value(response.effective_config).expect("effective config");
        assert_eq!(effective.agent.provider, "openai");
    });
}

#[test]
#[serial]
fn reset_workspace_target_uses_explicit_session_config_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");
    let explicit_path = temp.path().join("night.toml");
    fs::write(&explicit_path, "agent.provider = \"openai\"\n").expect("explicit config");

    let static_paths = StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        struct ExplicitConfigGuard(Option<PathBuf>);

        impl Drop for ExplicitConfigGuard {
            fn drop(&mut self) {
                set_explicit_config_path(self.0.take());
            }
        }

        let _override = ExplicitConfigGuard(explicit_config_path());
        set_explicit_config_path(Some(explicit_path.clone()));
        let response = ConfigService::reset(ConfigResetRequest {
            workspace,
            target: ConfigWriteTarget::Workspace,
            expected_layer_version: None,
            path: None,
        })
        .expect("explicit config reset");

        assert_eq!(response.target, ConfigWriteTarget::Workspace);
        assert_eq!(response.path, explicit_path);
        assert!(
            fs::read_to_string(response.path)
                .expect("reset explicit file")
                .trim()
                .is_empty()
        );
    });
}

#[test]
#[serial]
fn reset_creates_missing_explicit_session_config_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");
    let explicit_path = temp.path().join("new-session.toml");

    let static_paths = StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        struct ExplicitConfigGuard(Option<PathBuf>);

        impl Drop for ExplicitConfigGuard {
            fn drop(&mut self) {
                set_explicit_config_path(self.0.take());
            }
        }

        let _override = ExplicitConfigGuard(explicit_config_path());
        set_explicit_config_path(Some(explicit_path.clone()));
        let response = ConfigService::reset(ConfigResetRequest {
            workspace,
            target: ConfigWriteTarget::Workspace,
            expected_layer_version: None,
            path: None,
        })
        .expect("missing explicit config reset");

        assert!(!response.had_file);
        assert_eq!(fs::read_to_string(&explicit_path).expect("created config"), "");
    });
}

#[cfg(unix)]
#[test]
#[serial]
fn reset_rejects_symlinked_workspace_config() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    let outside = temp.path().join("outside.toml");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::write(&outside, "agent.provider = \"openai\"\n").expect("outside config");
    symlink(&outside, workspace.join("vtcode.toml")).expect("config symlink");

    let response = ConfigService::reset(ConfigResetRequest {
        workspace,
        target: ConfigWriteTarget::Workspace,
        expected_layer_version: None,
        path: None,
    });

    let error = format!("{:#}", response.expect_err("symlink reset must fail"));
    assert!(error.contains("symlink"));
    assert!(outside.exists(), "the symlink target must remain untouched");
}

#[cfg(unix)]
#[test]
#[serial]
fn reset_rejects_dangling_workspace_symlink_instead_of_using_fallback() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    let config_dir = workspace.join(".vtcode");
    let fallback = config_dir.join("vtcode.toml");
    let dangling_target = temp.path().join("missing.toml");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(&fallback, "agent.provider = \"openai\"\n").expect("fallback config");
    symlink(&dangling_target, workspace.join("vtcode.toml")).expect("dangling config symlink");

    let static_paths = StaticWorkspacePaths::new(&workspace, &config_dir);
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let response = ConfigService::reset(ConfigResetRequest {
            workspace: workspace.clone(),
            target: ConfigWriteTarget::Workspace,
            expected_layer_version: None,
            path: None,
        });

        let error = format!("{:#}", response.expect_err("dangling symlink reset must fail"));
        assert!(error.contains("symlink"));
        assert_eq!(fs::read_to_string(&fallback).expect("fallback config"), "agent.provider = \"openai\"\n");
    });
}
