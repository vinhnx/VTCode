use super::*;

#[test]
#[serial]
fn custom_providers_array_add_uses_valid_template() {
    let temp = tempfile::tempdir().expect("temp dir");
    let user_path = temp.path().join("home").join("vtcode.toml");
    std::fs::create_dir_all(user_path.parent().expect("user config parent")).expect("user config dir");
    let paths = StaticWorkspacePaths::new(temp.path(), temp.path().join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(vec![user_path.clone()])
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: temp.path().join("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("custom_providers".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };

        let outcome = apply_settings_action(&mut state, "settings:array_add:custom_providers")
            .expect("add custom provider template");

        assert_eq!(outcome.message.as_deref(), Some("Added item to Custom Providers"));
        assert!(outcome.saved);

        assert_eq!(state.draft.custom_providers.len(), 1);
        let provider = &state.draft.custom_providers[0];
        assert_eq!(provider.name, "custom-provider-1");
        assert_eq!(provider.display_name, "Custom Provider 1");
        assert_eq!(provider.base_url, "https://llm.example/v1");
        assert_eq!(provider.api_key_env, "");
        assert_eq!(provider.model, "");

        state.view_path = Some("custom_providers[0]".to_string());
        let draft = TomlValue::try_from(state.draft.clone()).expect("draft should serialize");
        let items = build_settings_items(&state, &draft).expect("custom provider fields should render");
        let base_url = items
            .iter()
            .find(|item| item.title == "Base Url")
            .expect("base URL field should be visible");
        assert_eq!(
            base_url.selection,
            Some(InlineListSelection::ConfigAction("settings:edit:custom_providers[0].base_url".to_string()))
        );

        apply_string_edit(&mut state, "custom_providers[0].base_url", "https://gateway.example/v1".to_string())
            .expect("custom provider base URL should be editable");
        assert_eq!(state.draft.custom_providers[0].base_url, "https://gateway.example/v1");
        let persisted = std::fs::read_to_string(&user_path).expect("updated provider config");
        assert!(persisted.contains("https://gateway.example/v1"));

        assert!(persisted.contains("custom_providers"));
    });
}
#[test]
#[serial]
fn settings_edit_does_not_copy_trusted_provider_into_workspace_config() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path();
    let user_path = workspace.join("home").join("vtcode.toml");
    let workspace_path = workspace.join("vtcode.toml");
    std::fs::create_dir_all(user_path.parent().expect("user config parent")).expect("user config dir");
    std::fs::write(
        &user_path,
        r#"
[[custom_providers]]
name = "trusted"
display_name = "Trusted"
base_url = "https://llm.example/v1"
model = "model"
api_key_env = "TRUSTED_API_KEY"
"#,
    )
    .expect("user config");
    std::fs::write(&workspace_path, "agent.provider = \"openai\"\n").expect("workspace config");

    let paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(vec![user_path.clone()])
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut state = create_settings_palette_state(workspace, None).expect("settings state");
        assert_eq!(
            canonicalize(&state.source_path).expect("canonical source path"),
            canonicalize(&workspace_path).expect("canonical workspace path")
        );
        assert_eq!(state.draft.custom_providers.len(), 1);

        apply_settings_action(&mut state, "settings:set:agent.todo_planning_mode:toggle")
            .expect("workspace setting should persist");

        let workspace_content = std::fs::read_to_string(&workspace_path).expect("workspace config content");
        assert!(workspace_content.contains("todo_planning_mode = false"));
        assert!(!workspace_content.contains("custom_providers"));
        assert!(!workspace_content.contains("trusted"));

        let manager = ConfigManager::load_from_workspace(workspace).expect("reloaded configuration");
        assert_eq!(manager.config().custom_providers.len(), 1);
        assert!(!manager.config().agent.todo_planning_mode);

        apply_settings_action(&mut state, "settings:array_add:custom_providers")
            .expect("custom provider should persist to the trusted layer");
        let workspace_content = std::fs::read_to_string(&workspace_path).expect("workspace config content");
        let user_content = std::fs::read_to_string(&user_path).expect("user config content");
        assert!(!workspace_content.contains("custom_providers"));
        assert!(user_content.contains("custom-provider-1"));

        let manager = ConfigManager::load_from_workspace(workspace).expect("reloaded provider configuration");
        assert_eq!(manager.config().custom_providers.len(), 2);
    });
}
#[test]
fn toggle_action_produces_change_feedback_message() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_path = temp.path().join("vtcode.toml");
    let mut state = SettingsPaletteState {
        workspace: temp.path().to_path_buf(),
        source_path: source_path.clone(),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: None,
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };

    let outcome = apply_settings_action(&mut state, "settings:set:agent.todo_planning_mode:toggle")
        .expect("toggle planning mode");

    assert_eq!(outcome.message.as_deref(), Some("Disabled Todo Planning Mode"));
    assert!(outcome.saved);
    assert!(!state.draft.agent.todo_planning_mode);
}
#[test]
fn settings_palette_state_loads_workspace_config_directly() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_path = temp.path().join("vtcode.toml");
    std::fs::write(&source_path, "[workspace]\nuse_root_config = true\n\n[agent]\ntheme = \"ansi\"\n")
        .expect("workspace config should be written");

    let state = create_settings_palette_state(temp.path(), None).expect("settings state should load");

    assert_eq!(
        canonicalize(&state.source_path).expect("canonical state source path"),
        canonicalize(&source_path).expect("canonical expected source path")
    );
    assert_eq!(state.draft.agent.theme, "ansi");
}
#[test]
#[serial]
fn settings_reload_is_fail_closed_and_tracks_layer_creation_and_deletion() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path();
    let fallback_path = workspace.join(".vtcode").join("vtcode.toml");
    let paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(paths))
        .with_home_paths(Vec::new())
        .with_system_config_paths(Vec::new());

    with_config_defaults_provider_for_test(Arc::new(provider), || {
        let mut state = create_settings_palette_state(workspace, None).expect("settings state should load");
        let initial_provider = state.draft.agent.provider.clone();

        std::fs::create_dir_all(fallback_path.parent().expect("fallback parent")).expect("fallback dir");
        std::fs::write(&fallback_path, "agent.provider = \"openai\"\n").expect("fallback config");
        reload_state_from_disk(&mut state).expect("created fallback should reload");
        assert_eq!(state.draft.agent.provider, "openai");
        assert_eq!(
            canonicalize(&state.source_path).expect("canonical source path"),
            canonicalize(&fallback_path).expect("canonical fallback path")
        );

        let valid_provider = state.draft.agent.provider.clone();
        std::fs::write(&fallback_path, "agent.provider = [\n").expect("malformed fallback config");
        assert!(reload_state_from_disk(&mut state).is_err());
        assert_eq!(state.draft.agent.provider, valid_provider);

        std::fs::remove_file(&fallback_path).expect("remove fallback config");
        reload_state_from_disk(&mut state).expect("deleted fallback should reload defaults");
        assert_eq!(state.draft.agent.provider, initial_provider);
        assert_eq!(state.source_path, workspace.join("vtcode.toml"));
    });
}
