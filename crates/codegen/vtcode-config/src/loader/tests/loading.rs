use super::*;

#[test]
fn default_config_selects_build_primary_agent() {
    assert_eq!(VTCodeConfig::default().default_primary_agent, "build");
}

#[test]
fn compiled_default_config_contains_release_loop_budgets() {
    let config = VTCodeConfig::default();

    assert_eq!(config.agent.harness.max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);
    assert_eq!(config.tools.max_tool_loops, tool_limits::DEFAULT_MAX_TOOL_LOOPS);
    assert_eq!(config.automation.full_auto.max_turns, tool_limits::DEFAULT_FULL_AUTO_MAX_TURNS);
    assert_eq!(config.agent.max_conversation_turns, tool_limits::DEFAULT_MAX_CONVERSATION_TURNS);
}

#[test]
fn tool_profile_parses_and_serialises_in_tools_table() {
    let config: VTCodeConfig = toml::from_str(
        r#"
[tools]
profile = "advanced_vtcode"
"#,
    )
    .expect("tool profile should parse from the tools table");
    assert_eq!(config.tools.profile, crate::core::ToolProfile::AdvancedVtCode);

    let serialised = toml::to_string(&config).expect("configuration should serialise");
    assert!(serialised.contains("[tools]"));
    assert!(serialised.contains("profile = \"advanced_vtcode\""));
}

#[test]
#[serial]
fn test_layered_config_loading() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();

    // 1. User config
    let home_dir = workspace_root.join("home");
    fs::create_dir_all(&home_dir).expect("failed to create home dir");
    let user_config_path = home_dir.join("vtcode.toml");
    fs::write(&user_config_path, "agent.provider = \"anthropic\"").expect("failed to write user config");

    // 2. Workspace config
    let workspace_config_path = workspace_root.join("vtcode.toml");
    fs::write(&workspace_config_path, "agent.default_model = \"claude-sonnet-5\"")
        .expect("failed to write workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace_root, workspace_root.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![user_config_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace_root).expect("failed to load config");

        assert_eq!(manager.config().agent.provider, "anthropic");
        assert_eq!(manager.config().agent.default_model, "claude-sonnet-5");

        let layers = manager.layer_stack().layers();
        // User + Workspace
        assert_eq!(layers.len(), 2);
        assert!(matches!(layers[0].source, ConfigLayerSource::User { .. }));
        assert!(matches!(layers[1].source, ConfigLayerSource::Workspace { .. }));
    });
}

#[test]
#[serial]
fn canonical_xdg_user_config_overrides_legacy_user_config() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let legacy_path = workspace.path().join("legacy").join("vtcode.toml");
    let canonical_path = workspace.path().join("xdg").join("vtcode.toml");
    fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("legacy dir");
    fs::create_dir_all(canonical_path.parent().expect("canonical parent")).expect("canonical dir");
    fs::write(&legacy_path, "agent.provider = \"openai\"\n").expect("legacy config");
    fs::write(&canonical_path, "agent.provider = \"anthropic\"\n").expect("canonical config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider =
        WorkspacePathsDefaults::new(Arc::new(paths)).with_home_paths(vec![legacy_path.clone(), canonical_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load config");
        assert_eq!(manager.config().agent.provider, "anthropic");
        let user_files = manager
            .layer_stack()
            .layers()
            .iter()
            .filter_map(|layer| match &layer.source {
                ConfigLayerSource::User { file } => Some(file),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            user_files
                .into_iter()
                .map(|path| canonicalize(path).expect("canonical user path"))
                .collect::<Vec<_>>(),
            vec![
                canonicalize(&legacy_path).expect("canonical legacy path"),
                canonicalize(&canonical_path).expect("canonical XDG path"),
            ]
        );
    });
}

#[test]
#[serial]
fn system_config_candidates_are_loaded_low_to_high_without_duplicates() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let first_path = workspace.path().join("system-first.toml");
    let second_path = workspace.path().join("system-second.toml");
    fs::write(&first_path, "agent.provider = \"openai\"\n").expect("first system config");
    fs::write(&second_path, "agent.provider = \"anthropic\"\n").expect("second system config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(vec![second_path.clone(), first_path.clone(), first_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load config");
        let system_files = manager
            .layer_stack()
            .layers()
            .iter()
            .filter_map(|layer| match &layer.source {
                ConfigLayerSource::System { file } => Some(file),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(manager.config().agent.provider, "openai");
        assert_eq!(
            system_files
                .into_iter()
                .map(|path| canonicalize(path).expect("canonical system path"))
                .collect::<Vec<_>>(),
            vec![
                canonicalize(&second_path).expect("canonical second system path"),
                canonicalize(&first_path).expect("canonical first system path"),
            ]
        );
    });
}

#[test]
#[serial]
fn save_config_uses_the_canonical_path_captured_during_load() {
    let workspace = assert_fs::TempDir::new().expect("workspace");
    let canonical_path = workspace.path().join("canonical/vtcode.toml");
    let other_path = workspace.path().join("other/vtcode.toml");
    fs::create_dir_all(canonical_path.parent().expect("canonical parent")).expect("canonical directory");
    fs::write(&canonical_path, "agent.provider = \"openai\"\n").expect("canonical config");

    let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths)).with_home_paths(vec![canonical_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut manager = ConfigManager::load_from_workspace(workspace.path()).expect("load config");
        fs::remove_file(&canonical_path).expect("remove loaded config before save");

        let replacement_paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
        let replacement_provider =
            WorkspacePathsDefaults::new(Arc::new(replacement_paths)).with_home_paths(vec![other_path.clone()]);
        let previous = defaults::provider::install_config_defaults_provider(Arc::new(replacement_provider));

        let config = manager.config().clone();
        manager.save_config(&config).expect("save canonical config");

        let _ = defaults::provider::install_config_defaults_provider(previous);

        assert!(canonical_path.exists());
        assert!(!other_path.exists());
    });
}

#[test]
#[serial]
fn test_invalid_layer_is_reported_with_source_context() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();

    let home_dir = workspace_root.join("home");
    fs::create_dir_all(&home_dir).expect("failed to create home dir");
    let user_config_path = home_dir.join("vtcode.toml");
    fs::write(&user_config_path, "[agent\nprovider = \"openai\"").expect("failed to write invalid user config");

    let workspace_config_path = workspace_root.join("vtcode.toml");
    fs::write(&workspace_config_path, "agent.provider = \"anthropic\"").expect("failed to write workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace_root, workspace_root.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![user_config_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let result = ConfigManager::load_from_workspace(workspace_root);
        assert!(result.is_err(), "expected load to fail for invalid layer");
        let error = match result {
            Ok(_) => String::new(),
            Err(err) => format!("{err:#}"),
        };
        assert!(
            error.contains(&user_config_path.display().to_string()),
            "error should include invalid layer path, got: {error}"
        );
    });
}

#[test]
#[serial]
fn test_config_builder_overrides() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();

    let workspace_config_path = workspace_root.join("vtcode.toml");
    fs::write(&workspace_config_path, "agent.provider = \"openai\"").expect("failed to write workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace_root, workspace_root.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigBuilder::new()
            .workspace(workspace_root.to_path_buf())
            .cli_override("agent.provider".to_string(), toml::Value::String("gemini".to_string()))
            .cli_override("agent.default_model".to_string(), toml::Value::String("gemini-1.5-pro".to_string()))
            .cli_overrides(&[("tools.profile".to_string(), "advanced_vtcode".to_string())])
            .build()
            .expect("failed to build config");

        assert_eq!(manager.config().agent.provider, "gemini");
        assert_eq!(manager.config().agent.default_model, "gemini-1.5-pro");
        assert_eq!(manager.config().tools.profile, crate::core::ToolProfile::AdvancedVtCode);

        let layers = manager.layer_stack().layers();
        // Workspace + Runtime
        assert_eq!(layers.len(), 2);
        assert!(matches!(layers[0].source, ConfigLayerSource::Workspace { .. }));
        assert!(matches!(layers[1].source, ConfigLayerSource::Runtime));
    });
}
