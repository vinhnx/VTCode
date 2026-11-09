use super::*;

#[test]
fn tool_display_mode_is_exposed_as_a_cycle_setting() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("ui".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let item = items
        .iter()
        .find(|item| item.title == "Tool Display Mode")
        .expect("tool display mode entry");
    assert_eq!(
        item.selection,
        Some(InlineListSelection::ConfigAction("settings:set:ui.tool_display_mode:cycle".to_string()))
    );
    assert_eq!(item.badge.as_deref(), None, "setting rows carry value, not action badges");
    assert!(
        item.value
            .as_deref()
            .is_some_and(|value| value.contains("compact") || value.contains("expanded")),
        "live value should render in the value slot: {:?}",
        item.value
    );
}
#[test]
fn tool_display_mode_cycle_persists_to_disk() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_path = temp.path().join("vtcode.toml");
    let mut state = SettingsPaletteState {
        workspace: temp.path().to_path_buf(),
        source_path: source_path.clone(),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("ui".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };

    let outcome =
        apply_settings_action(&mut state, "settings:set:ui.tool_display_mode:cycle").expect("cycle tool display mode");

    assert!(outcome.saved);
    // The default is compact; cycling advances to expanded.
    assert_eq!(state.draft.ui.tool_display_mode, vtcode_core::config::ToolDisplayMode::Expanded);
    let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
    assert!(persisted.contains("tool_display_mode = \"expanded\""));
}
#[test]
fn decisions_probe_toggle_persists_reloads_and_survives_provider_changes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_path = temp.path().join("vtcode.toml");
    let mut state = SettingsPaletteState {
        workspace: temp.path().to_path_buf(),
        source_path: source_path.clone(),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("group:approvals_security".into()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("serialize config");
    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.selection.as_ref()
        == Some(&InlineListSelection::ConfigAction(
            "settings:set:permissions.auto.use_decisions_probe:toggle".into()
        ))));
    assert!(
        apply_settings_action(&mut state, "settings:set:permissions.auto.use_decisions_probe:toggle")
            .expect("toggle")
            .saved
    );
    assert!(state.draft.permissions.auto_permission.use_decisions_probe);
    let persisted = std::fs::read_to_string(&source_path).expect("read config");
    assert!(persisted.contains("use_decisions_probe = true"));
    state.draft.permissions.auto_permission.use_decisions_probe = false;
    reload_state_from_disk(&mut state).expect("reload settings");
    assert!(state.draft.permissions.auto_permission.use_decisions_probe);
    let mut reloaded: VTCodeConfig = toml::from_str(&persisted).expect("reload config");
    for provider in ["anthropic", "openai"] {
        reloaded.agent.provider = provider.into();
        let roundtrip: VTCodeConfig = toml::from_str(&toml::to_string(&reloaded).expect("serialize")).expect("reload");
        assert!(roundtrip.permissions.auto_permission.use_decisions_probe);
    }
    assert!(
        apply_settings_action(&mut state, "settings:set:permissions.auto.use_decisions_probe:toggle")
            .expect("toggle off")
            .saved
    );
    assert!(!state.draft.permissions.auto_permission.use_decisions_probe);
}
#[test]
fn interface_terminal_group_exposes_copy_on_select_toggle() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(format!("{SETTINGS_GROUP_PREFIX}interface_terminal")),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let item = items
        .iter()
        .find(|item| {
            item.selection.as_ref().is_some_and(|selection| {
                matches!(selection, InlineListSelection::ConfigAction(action)
                    if action == "settings:set:ui.fullscreen.copy_on_select:toggle")
            })
        })
        .expect("copy on select entry in Interface & Terminal");
    assert_eq!(item.badge.as_deref(), None, "boolean state lives in the value slot");
    assert_eq!(item.kind, vtcode_commons::ui_protocol::InlineItemKind::Setting);
}
#[test]
fn interface_terminal_group_exposes_message_timestamps_toggle() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(format!("{SETTINGS_GROUP_PREFIX}interface_terminal")),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let item = items
        .iter()
        .find(|item| {
            item.selection.as_ref().is_some_and(|selection| {
                matches!(selection, InlineListSelection::ConfigAction(action)
                    if action == "settings:set:ui.show_message_timestamps:toggle")
            })
        })
        .expect("message timestamps entry in Interface & Terminal");
    assert_eq!(item.badge.as_deref(), None, "boolean state lives in the value slot");
    assert_eq!(item.kind, vtcode_commons::ui_protocol::InlineItemKind::Setting);
}
#[test]
fn copy_on_select_toggle_persists_manual_mode_to_disk() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_path = temp.path().join("vtcode.toml");
    let mut state = SettingsPaletteState {
        workspace: temp.path().to_path_buf(),
        source_path: source_path.clone(),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(format!("{SETTINGS_GROUP_PREFIX}interface_terminal")),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    assert!(state.draft.ui.fullscreen.copy_on_select);

    let outcome = apply_settings_action(&mut state, "settings:set:ui.fullscreen.copy_on_select:toggle")
        .expect("toggle copy on select");

    assert!(outcome.saved);
    assert!(!state.draft.ui.fullscreen.copy_on_select);
    let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
    assert!(persisted.contains("copy_on_select = false"));
}
