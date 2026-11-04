use super::*;

#[test]
fn repository_save_removes_copilot_commands_in_table_and_inline_forms() {
    let dir = assert_fs::TempDir::new().expect("workspace");
    for content in [
        "[auth.copilot]\ncommand = 'old-command'\nhost = 'github.example'\n",
        "auth = { copilot = { command = 'old-command', host = 'github.example' } }\n",
    ] {
        let path = dir.path().join("vtcode.toml");
        fs::write(&path, content).expect("existing config");
        let mut config = VTCodeConfig::default();
        config.auth.copilot.command = Some("trusted-command".to_string());
        config.auth.copilot.host = Some("github.example".to_string());
        ConfigManager::save_repository_config_to_path(&path, &config).expect("save");
        let saved: toml::Value = toml::from_str(&fs::read_to_string(path).expect("read")).expect("parse");
        assert!(
            saved
                .get("auth")
                .and_then(|auth| auth.get("copilot"))
                .and_then(|copilot| copilot.get("command"))
                .is_none()
        );
        assert_eq!(saved["auth"]["copilot"]["host"].as_str(), Some("github.example"));
    }
}

#[test]
fn save_config_preserves_comments() {
    use std::io::Write;

    let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
    let config_with_comments = r#"# This is a test comment
[agent]
# Provider comment
provider = "anthropic"
default_model = "gpt-5-nano"

# Tools section comment
[tools]
default_policy = "deny"
"#;

    write!(temp_file, "{config_with_comments}").expect("failed to write temp config");
    temp_file.flush().expect("failed to flush");

    // Load config
    let manager = ConfigManager::load_from_file(temp_file.path()).expect("failed to load config");

    // Modify and save
    let mut modified_config = manager.config().clone();
    modified_config.agent.default_model = "gpt-5".to_string();

    ConfigManager::save_config_to_path(temp_file.path(), &modified_config).expect("failed to save config");

    // Read back and verify comments are preserved
    let saved_content = fs::read_to_string(temp_file.path()).expect("failed to read saved config");

    assert!(saved_content.contains("# This is a test comment"), "top-level comment should be preserved");
    assert!(saved_content.contains("# Provider comment"), "inline comment should be preserved");
    assert!(saved_content.contains("# Tools section comment"), "section comment should be preserved");
    assert!(saved_content.contains("gpt-5"), "modified value should be present");
}

#[test]
fn save_clears_default_absent_optional_fields_from_existing_file() {
    // Regression: picking "Project default" service_tier must remove a
    // previously saved tier. Optional fields absent from the default
    // serialization were never visited by the sparse merge, so stale values
    // survived every save and the next session kept using them.
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let path = workspace.path().join("vtcode.toml");
    fs::write(&path, "[provider.openai]\nservice_tier = \"ultrafast\"\n[provider.anthropic]\nspeed = \"fast\"\n[foreign_tool]\nkeep_me = true\n")
        .expect("seed config");

    let config = VTCodeConfig::default();
    assert_eq!(config.provider.openai.service_tier, None);
    ConfigManager::save_config_to_path(&path, &config).expect("save");

    let saved = fs::read_to_string(&path).expect("read back");
    assert!(!saved.contains("ultrafast"), "stale tier must be cleared, got:\n{saved}");
    assert!(!saved.contains("[provider.openai]"), "emptied table must be pruned, got:\n{saved}");
    assert!(saved.contains("keep_me"), "foreign keys must be preserved, got:\n{saved}");

    let reloaded: VTCodeConfig = toml::from_str(&saved).expect("reload");
    assert_eq!(reloaded.provider.openai.service_tier, None);
    assert_eq!(reloaded.provider.anthropic.speed, None);
    assert!(!saved.contains("[provider.anthropic]"), "stale Claude speed must be cleared: {saved}");
}

#[test]
fn save_config_rejects_malformed_existing_file_without_replacement() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let path = workspace.path().join("vtcode.toml");
    let original = "[agent\nprovider = \"openai\"\n";
    fs::write(&path, original).expect("seed malformed config");
    let error = ConfigManager::save_config_to_path(&path, &VTCodeConfig::default()).expect_err("reject malformed");
    assert!(format!("{error:#}").contains("Failed to parse existing config"));
    assert_eq!(fs::read_to_string(&path).expect("read back"), original);
}

#[cfg(unix)]
#[test]
fn save_config_rejects_symlink_without_modifying_destination() {
    use std::os::unix::fs::symlink;

    let workspace = assert_fs::TempDir::new().expect("workspace");
    let destination = workspace.path().join("outside.toml");
    let path = workspace.path().join("vtcode.toml");
    let original = "# protected destination\n[agent]\nprovider = \"openai\"\n";
    fs::write(&destination, original).expect("seed destination");
    symlink(&destination, &path).expect("create symlink");
    let config = VTCodeConfig::default();
    for result in [
        ConfigManager::save_config_to_path(&path, &config),
        ConfigManager::save_repository_config_to_path(&path, &config),
    ] {
        assert!(format!("{:#}", result.expect_err("reject symlink")).contains("symlink"));
        assert_eq!(fs::read_to_string(&destination).expect("read destination"), original);
        assert!(fs::symlink_metadata(&path).expect("link metadata").file_type().is_symlink());
    }
}

#[test]
#[serial]
fn config_defaults_provider_overrides_paths_and_theme() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join("config-root");
    fs::create_dir_all(&config_dir).expect("failed to create config directory");

    let config_file_name = "custom-config.toml";
    let config_path = config_dir.join(config_file_name);
    let serialized = toml::to_string(&VTCodeConfig::default()).expect("failed to serialize default config");
    fs::write(&config_path, serialized).expect("failed to write config file");

    let static_paths = StaticWorkspacePaths::new(workspace_root, &config_dir);
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_config_file_name(config_file_name)
        .with_home_paths(Vec::new())
        .with_syntax_theme("custom-theme")
        .with_syntax_languages(vec!["zig".to_string()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace_root).expect("failed to load workspace config");

        let resolved_path = manager.config_path().expect("config path should be resolved");
        let resolved_canonical = canonicalize(resolved_path).expect("resolved config path should canonicalize");
        let expected_canonical = canonicalize(&config_path).expect("expected config path should canonicalize");
        assert_eq!(resolved_canonical, expected_canonical);

        assert_eq!(SyntaxHighlightingDefaults::theme(), "custom-theme");
        assert_eq!(SyntaxHighlightingDefaults::enabled_languages(), vec!["zig".to_string()]);
    });
}

#[test]
#[serial]
fn save_config_updates_disk_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path();
    let config_path = workspace.join("vtcode.toml");

    // Write initial config
    let initial_config = r#"
[ui]
display_mode = "minimal"
show_sidebar = false
"#;
    fs::write(&config_path, initial_config).expect("failed to write initial config");

    // Load config
    let mut manager = ConfigManager::load_from_workspace(workspace).expect("failed to load config");
    assert_eq!(manager.config().ui.display_mode, crate::UiDisplayMode::Minimal);

    // Modify config (simulating /config palette changes)
    let mut modified_config = manager.config().clone();
    modified_config.ui.display_mode = crate::UiDisplayMode::Full;
    modified_config.ui.show_sidebar = true;

    // Save config
    manager.save_config(&modified_config).expect("failed to save config");

    // Verify disk file was updated
    let saved_content = fs::read_to_string(&config_path).expect("failed to read saved config");
    assert!(
        saved_content.contains("display_mode = \"full\""),
        "saved config should contain full display_mode. Got:\n{saved_content}"
    );
    assert!(
        !saved_content.contains("show_sidebar"),
        "saved config should prune default show_sidebar. Got:\n{saved_content}"
    );

    // Create a NEW manager to simulate reopening /config palette
    let new_manager = ConfigManager::load_from_workspace(workspace).expect("failed to reload config");
    assert_eq!(
        new_manager.config().ui.display_mode,
        crate::UiDisplayMode::Full,
        "reloaded config should have full display_mode"
    );

    // Force disk read by loading from file directly
    let new_manager2 = ConfigManager::load_from_file(&config_path).expect("failed to reload from file");
    assert!(
        new_manager2.config().ui.show_sidebar,
        "reloaded config should have show_sidebar = true, got: {}",
        new_manager2.config().ui.show_sidebar
    );
}

#[test]
fn repository_config_save_strips_trusted_provider_settings_and_repairs_existing_keys() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(
        &config_path,
        r#"
[agent]
default_model = "old-model"

[[custom_providers]]
name = "stale-provider"
display_name = "Stale Provider"
base_url = "https://stale.example/v1"
model = "stale-model"

[provider_overrides.openai]
models = ["old-model"]
base_url = "https://stale.example/v1"
api_key_env = "STALE_API_KEY"
"#,
    )
    .expect("write stale repository config");

    let config: VTCodeConfig = toml::from_str(
        r#"
[agent]
default_model = "new-model"

[[custom_providers]]
name = "trusted-provider"
display_name = "Trusted Provider"
base_url = "https://trusted.example/v1"
model = "trusted-model"

[provider_overrides.openai]
models = ["trusted-model"]
base_url = "https://trusted.example/v1"
api_key_env = "TRUSTED_API_KEY"
"#,
    )
    .expect("parse trusted effective config");

    ConfigManager::save_repository_config_to_path(&config_path, &config).expect("save repository config");

    let saved: toml::Value = toml::from_str(&fs::read_to_string(&config_path).expect("read repository config"))
        .expect("parse saved repository config");
    assert!(saved.get("custom_providers").is_none(), "custom providers must stay in trusted layers");
    let openai_override = saved
        .get("provider_overrides")
        .and_then(|value| value.get("openai"))
        .expect("model-list override should remain");
    assert_eq!(openai_override.get("models").and_then(toml::Value::as_array).map(Vec::len), Some(1));
    assert!(openai_override.get("base_url").is_none(), "repository endpoint must be removed");
    assert!(openai_override.get("api_key_env").is_none(), "repository credential selector must be removed");
    assert_eq!(
        saved
            .get("agent")
            .and_then(|value| value.get("default_model"))
            .and_then(toml::Value::as_str),
        Some("new-model")
    );
}

#[test]
#[serial]
fn save_config_does_not_flatten_trusted_provider_settings_into_workspace() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let user_config = workspace.path().join("home/vtcode.toml");
    fs::create_dir_all(user_config.parent().expect("user config parent")).expect("create user config directory");
    fs::write(
        &user_config,
        r#"
[[custom_providers]]
name = "trusted-provider"
display_name = "Trusted Provider"
base_url = "https://trusted.example/v1"
model = "trusted-model"

[provider_overrides.openai]
models = ["trusted-openai-model"]
base_url = "https://trusted.example/v1"
api_key_env = "TRUSTED_OPENAI_API_KEY"
"#,
    )
    .expect("write user config");
    let workspace_config = workspace.path().join("vtcode.toml");
    fs::write(&workspace_config, "[agent]\ndefault_model = \"workspace-model\"\n").expect("write workspace config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(vec![user_config])
        .with_system_config_paths(Vec::new());

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut manager = ConfigManager::load_from_workspace(workspace.path()).expect("load layered config");
        assert_eq!(manager.config().custom_providers.len(), 1);
        assert_eq!(
            manager.config().provider_overrides["openai"].base_url.as_deref(),
            Some("https://trusted.example/v1")
        );

        let mut modified = manager.config().clone();
        modified.agent.theme = "ansi".to_string();
        manager.save_config(&modified).expect("save workspace setting");

        let written = fs::read_to_string(&workspace_config).expect("read workspace config");
        assert!(!written.contains("custom_providers"), "workspace must not receive trusted providers");
        assert!(!written.contains("trusted.example"), "workspace must not receive trusted provider endpoints");
        assert!(!written.contains("TRUSTED_OPENAI_API_KEY"), "workspace must not receive credential selectors");
        assert!(written.contains("theme = \"ansi\""), "workspace setting should still be persisted");

        let reloaded = ConfigManager::load_from_workspace(workspace.path()).expect("reload layered config");
        assert_eq!(reloaded.config().custom_providers[0].name, "trusted-provider");
        assert_eq!(
            reloaded.config().provider_overrides["openai"].base_url.as_deref(),
            Some("https://trusted.example/v1")
        );
        assert_eq!(reloaded.config().agent.theme, "ansi");
    });
}

#[test]
#[serial]
fn explicitly_selected_config_save_preserves_custom_provider_settings() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let explicit_config = temp_dir.path().join("explicit.toml");
    fs::write(
        &explicit_config,
        r#"
[[custom_providers]]
name = "explicit-provider"
display_name = "Explicit Provider"
base_url = "https://explicit.example/v1"
model = "explicit-model"
"#,
    )
    .expect("write explicit config");

    let paths = StaticWorkspacePaths::new(temp_dir.path(), temp_dir.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut manager = ConfigManager::load_from_file(&explicit_config).expect("load explicit config");
        let mut modified = manager.config().clone();
        modified.agent.theme = "ansi".to_string();
        manager.save_config(&modified).expect("save explicit config");

        let saved: toml::Value = toml::from_str(&fs::read_to_string(&explicit_config).expect("read explicit config"))
            .expect("parse explicit config");
        assert_eq!(
            saved
                .get("custom_providers")
                .and_then(toml::Value::as_array)
                .and_then(|providers| providers.first())
                .and_then(|provider| provider.get("name"))
                .and_then(toml::Value::as_str),
            Some("explicit-provider")
        );
        assert!(saved.to_string().contains("theme = \"ansi\""));
    });
}

#[test]
#[serial]
fn save_config_writes_sparse_model_theme_and_permission_values() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path();
    let config_path = workspace.join("vtcode.toml");
    // Opt out of the developer's global config so this persistence test is
    // deterministic when run on a machine with user-level settings.
    fs::write(&config_path, "[workspace]\nuse_root_config = true\n").expect("failed to write initial config");

    let mut manager = ConfigManager::load_from_workspace(workspace).expect("failed to load config");
    let mut modified_config = manager.config().clone();
    modified_config.agent.default_model = "gpt-5.6-sol".to_string();
    modified_config.agent.theme = "ansi".to_string();
    modified_config.permissions.allow = vec!["read_file".to_string()];

    manager.save_config(&modified_config).expect("failed to save config");

    let saved_content = fs::read_to_string(&config_path).expect("failed to read saved config");
    assert!(
        !saved_content.contains("default_primary_agent"),
        "unchanged default primary agent should not be persisted. Got:\n{saved_content}"
    );
    assert!(saved_content.contains("[agent]"));
    assert!(saved_content.contains("default_model = \"gpt-5.6-sol\""));
    assert!(saved_content.contains("theme = \"ansi\""));
    assert!(saved_content.contains("[permissions]"));
    assert!(saved_content.contains("allow = [\"read_file\"]"));
    assert!(
        !saved_content.contains("provider = \"openai\""),
        "default agent provider should not be expanded. Got:\n{saved_content}"
    );
    assert!(!saved_content.contains("[ui]"), "default UI section should not be expanded. Got:\n{saved_content}");

    let reloaded = ConfigManager::load_from_workspace(workspace).expect("failed to reload config");
    assert_eq!(reloaded.config().agent.default_model, "gpt-5.6-sol");
    assert_eq!(reloaded.config().agent.theme, "ansi");
    assert_eq!(reloaded.config().permissions.allow, vec!["read_file".to_string()]);
}

#[test]
#[serial]
fn deprecated_permission_keys_are_migrated_on_save() {
    for deprecated_key in ["allowed_tools", "disallowed_tools"] {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = temp_dir.path();
        let config_path = workspace.join("vtcode.toml");
        fs::write(
            &config_path,
            format!(
                r#"
[workspace]
use_root_config = true

[permissions]
{deprecated_key} = ["read_file"]
"#
            ),
        )
        .expect("failed to write config");

        // Deprecated keys are accepted on load (they are not a hard error),
        // so existing configs keep working until they are migrated on save.
        let manager = ConfigManager::load_from_workspace(workspace)
            .unwrap_or_else(|e| panic!("deprecated permission keys should load: {e:#}"));

        // Saving strips the deprecated keys (the save path migrates them away).
        ConfigManager::save_config_to_path(&config_path, manager.config()).expect("failed to save config");

        let written = fs::read_to_string(&config_path).unwrap();
        assert!(
            !written.contains(deprecated_key),
            "deprecated key {deprecated_key} should be stripped on save. Got:\n{written}"
        );
    }
}

#[test]
fn workspace_config_parses_from_toml() {
    let toml_str = r#"
[workspace]
use_root_config = true
include_context = false
max_context_size = 2097152
"#;
    let config: VTCodeConfig = toml::from_str(toml_str).expect("parse workspace config");
    assert!(config.workspace.use_root_config);
    assert!(!config.workspace.include_context);
    assert_eq!(config.workspace.max_context_size, Some(2_097_152));
}

#[test]
fn workspace_config_defaults_match_spec() {
    let config = VTCodeConfig::default();
    assert!(!config.workspace.use_root_config);
    assert!(config.workspace.include_context);
    assert_eq!(config.workspace.max_context_size, None);
}

#[test]
fn workspace_config_partial_toml_uses_defaults() {
    let toml_str = r#"
[workspace]
use_root_config = true
"#;
    let config: VTCodeConfig = toml::from_str(toml_str).expect("parse partial workspace config");
    assert!(config.workspace.use_root_config);
    assert!(config.workspace.include_context);
    assert_eq!(config.workspace.max_context_size, None);
}

#[test]
fn partial_optimization_agent_execution_uses_defaults() {
    let toml_str = r#"
[optimization.agent_execution]
max_execution_time_secs = 300
state_history_size = 100
resource_monitor_interval_ms = 5000
max_memory_mb = 4096
idle_timeout_ms = 30000
idle_backoff_ms = 1000
"#;
    let config: VTCodeConfig = toml::from_str(toml_str).expect("parse partial agent_execution config");
    assert_eq!(config.optimization.agent_execution.max_execution_time_secs, 300);
    assert_eq!(config.optimization.agent_execution.idle_timeout_ms, 30000);
    assert_eq!(config.optimization.agent_execution.idle_backoff_ms, 1000);
}

#[test]
#[serial]
fn test_config_loader_phase_timing_recorded() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let manager = ConfigManager::load_from_workspace(temp_dir.path()).expect("load workspace config");
    let timing = manager.phase_timing().expect("phase timing should be recorded");
    assert!(
        timing.path_resolution_us > 0
            || timing.layer_loading_us > 0
            || timing.merge_and_parse_us > 0
            || timing.validation_us > 0
    );
}

#[test]
#[serial]
fn explicit_session_override_loads_requested_file_as_workspace_layer() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");
    let workspace_config = workspace.join("vtcode.toml");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\n").expect("workspace config");

    let override_path = temp.path().join("custom-night.toml");
    fs::write(&override_path, "agent.provider = \"openai\"\n").expect("override config");

    let _guard = session_override::ExplicitConfigPathGuard::set(Some(override_path.clone()));

    let manager = ConfigManager::load_from_workspace(&workspace).expect("load with override");
    assert_eq!(manager.config().agent.provider, "openai");
    let canonical_override_path = canonicalize(&override_path).expect("canonical override path");
    assert_eq!(manager.config_path(), Some(canonical_override_path.as_path()));
    let workspace_files = manager
        .layer_stack()
        .layers()
        .iter()
        .filter_map(|layer| match &layer.source {
            ConfigLayerSource::Workspace { file } => Some(file.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        workspace_files
            .iter()
            .any(|file| canonicalize(file).ok().as_ref() == Some(&canonical_override_path)),
        "override file must appear as the Workspace layer: {workspace_files:?}"
    );
    assert_eq!(
        manager.workspace_root(),
        Some(canonicalize(&workspace).expect("canonical workspace").as_path()),
        "session workspace_root must remain the workspace, not the override file's parent"
    );
}

#[test]
#[serial]
fn explicit_session_override_without_override_falls_back_to_defaults() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path();
    let workspace_config = workspace.join("vtcode.toml");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\n").expect("workspace config");

    let _guard = session_override::ExplicitConfigPathGuard::set(None);

    let manager = ConfigManager::load_from_workspace(workspace).expect("load without override");
    assert_eq!(manager.config().agent.provider, "anthropic");
}

#[test]
#[serial]
fn explicit_session_override_save_config_writes_override_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");
    let workspace_config = workspace.join("vtcode.toml");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\n").expect("workspace config");

    let override_path = temp.path().join("custom-night.toml");
    fs::write(&override_path, "agent.provider = \"openai\"\n").expect("override config");

    let _guard = session_override::ExplicitConfigPathGuard::set(Some(override_path.clone()));

    let mut manager = ConfigManager::load_from_workspace(&workspace).expect("load with override");
    let mut modified = manager.config().clone();
    modified.ui.display_mode = crate::UiDisplayMode::Full;
    manager.save_config(&modified).expect("save with override");

    let saved = fs::read_to_string(&override_path).expect("read saved override file");
    assert!(saved.contains("display_mode = \"full\""), "override file must receive the save. Got:\n{saved}");
    let workspace_saved = fs::read_to_string(&workspace_config).expect("read workspace config");
    assert_eq!(
        workspace_saved, "agent.provider = \"anthropic\"\n",
        "workspace config must stay untouched when an explicit override is active"
    );
}

#[test]
#[serial]
fn explicit_session_override_survives_cache_invalidation() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path();

    let override_path = temp.path().join("custom-night.toml");
    fs::write(&override_path, "agent.provider = \"openai\"\n").expect("override config");

    let _guard = session_override::ExplicitConfigPathGuard::set(Some(override_path.clone()));

    let first = ConfigManager::load_from_workspace(workspace).expect("first load");
    assert_eq!(first.config().agent.provider, "openai");

    fs::write(&override_path, "agent.provider = \"anthropic\"\n").expect("rewrite override config");
    ConfigManager::invalidate_workspace_cache(workspace);

    let second = ConfigManager::load_from_workspace(workspace).expect("reload after invalidate");
    assert_eq!(
        second.config().agent.provider,
        "anthropic",
        "reload must re-read the override file after cache invalidation"
    );
}

#[test]
#[serial]
fn explicit_session_override_file_with_use_root_config_drops_lower_layers() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");
    let workspace_config = workspace.join("vtcode.toml");
    fs::write(&workspace_config, "agent.provider = \"anthropic\"\n").expect("workspace config");

    let override_path = temp.path().join("custom-night.toml");
    fs::write(&override_path, "agent.provider = \"openai\"\n\n[workspace]\nuse_root_config = true\n")
        .expect("override config");

    let _guard = session_override::ExplicitConfigPathGuard::set(Some(override_path));

    let manager = ConfigManager::load_from_workspace(&workspace).expect("load with override");
    assert_eq!(manager.config().agent.provider, "openai");
    assert!(manager.config().workspace.use_root_config, "use_root_config from the override file must be honored");
}

#[test]
#[serial]
fn legacy_top_level_default_provider_and_model_promote_to_agent() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let user_config_path = workspace.path().join("vtcode.toml");
    fs::write(&user_config_path, "default_provider = \"ollama\"\ndefault_model = \"gpt-oss:20b\"\n")
        .expect("legacy top-level config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths)).with_home_paths(vec![user_config_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load legacy config");
        assert_eq!(manager.config().agent.provider, "ollama");
        assert_eq!(manager.config().agent.default_model, "gpt-oss:20b");
    });
}

#[test]
#[serial]
fn explicit_agent_table_wins_over_legacy_top_level_alias_in_same_file() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let user_config_path = workspace.path().join("vtcode.toml");
    fs::write(&user_config_path, "default_provider = \"ollama\"\n[agent]\nprovider = \"openai\"\n")
        .expect("mixed config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths)).with_home_paths(vec![user_config_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load mixed config");
        assert_eq!(manager.config().agent.provider, "openai");
    });
}

#[test]
fn loader_loads_prompt_cache_ttl_and_rejects_invalid_values() {
    use crate::core::PromptCacheTtl;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vtcode.toml");
    for (wire, expected) in [("30m", PromptCacheTtl::M30), ("12h", PromptCacheTtl::H12)] {
        fs::write(&path, format!("[prompt_cache.providers.openai]\nprompt_cache_ttl = \"{wire}\"\n")).unwrap();
        let manager = ConfigManager::load_from_file(&path).unwrap();
        assert_eq!(manager.config().prompt_cache.providers.openai.prompt_cache_ttl, Some(expected));
    }
    fs::write(&path, "[prompt_cache.providers.openai]\nprompt_cache_ttl = \"24h\"\n").unwrap();
    assert!(ConfigManager::load_from_file(&path).is_err());
}
