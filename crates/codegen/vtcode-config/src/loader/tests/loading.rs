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

/// A config file that names a section but omits keys inside it must fall back
/// to the same built-in defaults as `VTCodeConfig::default()`, not to the
/// type's zero value.
///
/// Regression: these fields carried a bare `#[serde(default)]`, so `[security]`
/// with any single key silently flipped `zero_trust_mode`/`encrypt_payloads`
/// from `true` to `false`, and a `[commands]` section dropped all 57 built-in
/// `deny_list` entries — including `rm -rf /` — to nothing. With no section at
/// all the inner `Default` applies, which is why the bug only appeared for
/// partially specified configs.
#[test]
fn partial_section_keeps_builtin_defaults() {
    // Each section contains exactly one key, so every sibling must come from
    // its own default_*() source.
    let partial: VTCodeConfig = toml::from_str(
        r#"
[security]
human_in_the_loop = true

[commands]
approval_prefixes = []

[telemetry]
trajectory_enabled = true

[plugins]
manifests = []
"#,
    )
    .expect("partial sections parse");

    let compiled = VTCodeConfig::default();

    assert_eq!(
        partial.security.zero_trust_mode, compiled.security.zero_trust_mode,
        "omitting zero_trust_mode must not disable zero-trust checks"
    );
    assert!(partial.security.zero_trust_mode, "zero-trust must stay on for a partial [security]");

    assert_eq!(partial.commands.deny_list, compiled.commands.deny_list, "built-in deny rules must survive");
    assert!(!partial.commands.deny_list.is_empty(), "a [commands] section must not wipe the deny list");
    assert_eq!(partial.commands.allow_glob, compiled.commands.allow_glob);
    assert_eq!(partial.commands.allow_list, compiled.commands.allow_list);
    assert_eq!(partial.commands.deny_regex, compiled.commands.deny_regex);
    assert_eq!(
        partial.telemetry.trajectory_max_files, compiled.telemetry.trajectory_max_files,
        "sibling telemetry keys must keep their defaults"
    );

    // An explicit value must still win over the built-in.
    let overridden: VTCodeConfig = toml::from_str("[commands]\ndeny_list = []\n").expect("explicit parse");
    assert!(overridden.commands.deny_list.is_empty(), "an explicit empty list must still override");
}

/// `data/default_config.toml` is what `vtcode init` scaffolds as the user's
/// starting config, so a section it declares must not weaken the compiled
/// defaults by restating a subset of them.
///
/// Restating a list *replaces* it during deserialization; it does not extend it.
/// The scaffold therefore omits the curated security lists, and this test pins
/// that: a scaffolded config must carry at least the safety rules a config with
/// no file at all would get.
///
/// Deliberately narrow rather than a full field-by-field diff: the scaffold
/// legitimately differs from `VTCodeConfig::default()` in quickstart
/// presentation (provider, theme, status-line timings, a commented example MCP
/// server). Only the security-relevant invariants are asserted here.
#[test]
fn scaffold_config_does_not_weaken_security_defaults() {
    let scaffold_text = include_str!("../../../data/default_config.toml");
    let scaffold: VTCodeConfig = toml::from_str(scaffold_text).expect("scaffold config must parse");
    let compiled = VTCodeConfig::default();

    assert!(
        scaffold.commands.deny_list.len() >= compiled.commands.deny_list.len(),
        "scaffold deny_list has {} entries but the built-in default has {}; restating a shorter \
         list replaces the built-in rules instead of extending them",
        scaffold.commands.deny_list.len(),
        compiled.commands.deny_list.len()
    );
    assert!(
        scaffold.commands.deny_regex.len() >= compiled.commands.deny_regex.len(),
        "scaffold deny_regex must not drop the built-in deny patterns"
    );
    assert!(
        scaffold.commands.deny_glob.len() >= compiled.commands.deny_glob.len(),
        "scaffold deny_glob must not drop the built-in deny globs"
    );

    // The same rule for the security switches: a scaffolded config must not be
    // less strict than a default-constructed one.
    assert!(
        scaffold.security.zero_trust_mode || !compiled.security.zero_trust_mode,
        "scaffold must not disable zero-trust checks that are on by default"
    );
    assert!(
        scaffold.tools.policies.len() <= compiled.tools.policies.len(),
        "scaffold must not add tool policies the built-in default does not set"
    );
}

/// The scaffold must not contradict the compiled program-status default: it
/// either omits the section or restates the default, so `vtcode init` users
/// get the documented on-by-default reporting.
#[test]
fn scaffold_config_inherits_program_status_default() {
    let scaffold_text = include_str!("../../../data/default_config.toml");
    let scaffold: VTCodeConfig = toml::from_str(scaffold_text).expect("scaffold config must parse");
    let compiled = VTCodeConfig::default();
    assert_eq!(
        scaffold.ui.program_status.enabled, compiled.ui.program_status.enabled,
        "scaffold program_status.enabled must match the compiled default"
    );
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
