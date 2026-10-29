use super::*;

#[test]
fn codex_app_server_custom_command_entry_is_cycleable() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("agent.codex_app_server".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft: TomlValue = toml::from_str(
        r#"
        [agent.codex_app_server]
        command = "/usr/local/bin/codex"
        args = ["app-server"]
        startup_timeout_secs = 10
        experimental_features = false
        "#,
    )
    .expect("valid draft value");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let entry = items.iter().find(|item| item.title == "Command").expect("command entry");
    assert_eq!(
        entry.selection,
        Some(InlineListSelection::ConfigAction("settings:set:agent.codex_app_server.command:cycle".to_string()))
    );
}
#[test]
fn model_config_root_shows_main_section() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_MODEL_CONFIG_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Main Model"));
    assert!(!items.iter().any(|item| item.title == "Lightweight Model"));
}
#[test]
fn model_config_main_uses_picker_backed_default_model() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_MODEL_CONFIG_MAIN_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Provider"));
    let default_model = items
        .iter()
        .find(|item| item.title == "Default Model")
        .expect("default model entry");
    assert_eq!(default_model.selection, Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string())));
}
#[test]
fn tools_view_routes_external_editor_to_configure_action() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("tools".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let entry = items
        .iter()
        .find(|item| item.title == "External Editor")
        .expect("tools.external editor entry");
    assert_eq!(entry.selection, Some(InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string())));
}
#[test]
fn agent_view_uses_picker_backed_default_model() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("agent".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let default_model = items
        .iter()
        .find(|item| item.title == "Default Model")
        .expect("default model entry");
    assert_eq!(default_model.selection, Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string())));
}
