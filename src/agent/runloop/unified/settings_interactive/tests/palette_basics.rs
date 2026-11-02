use super::*;

#[test]
fn parse_path_handles_arrays() {
    let tokens = parse_path_tokens("commands.allow_list[1]").expect("tokens");
    assert_eq!(tokens.len(), 3);
    matches!(tokens[0], PathToken::Key(_));
    matches!(tokens[1], PathToken::Key(_));
    matches!(tokens[2], PathToken::Index(1));
}
#[test]
fn normalize_field_path_replaces_indexes() {
    assert_eq!(normalize_config_path("commands.allow_list[12]"), "commands.allow_list[]");
}
#[test]
fn parent_view_path_handles_nested_segments() {
    assert_eq!(parent_view_path("agent"), None);
    assert_eq!(parent_view_path("agent.vibe_coding"), Some("agent".to_string()));
    assert_eq!(parent_view_path("group:model_provider:custom_providers"), Some("group:model_provider".to_string()));
    assert_eq!(
        parent_view_path("group:model_provider:custom_providers[0]"),
        Some("group:model_provider:custom_providers".to_string())
    );
    assert_eq!(parent_view_path("advanced:custom_providers[0]"), Some("advanced:custom_providers".to_string()));
    assert_eq!(
        parent_view_path("hooks.lifecycle.pre_tool_use[0].hooks[2]"),
        Some("hooks.lifecycle.pre_tool_use[0].hooks".to_string())
    );
}
#[test]
fn settings_selection_memory_restores_parent_and_falls_back_when_removed() {
    let mut state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: None,
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let root_selection = InlineListSelection::ConfigAction("settings:open:group:agent_automation".to_string());
    let nested_selection =
        InlineListSelection::ConfigAction("settings:set:agent.todo_planning_mode:toggle".to_string());

    state.remember_selection(None, root_selection.clone());
    let group_path = format!("{SETTINGS_GROUP_PREFIX}agent_automation");
    state.view_path = Some(group_path.clone());
    state.remember_selection(Some(&group_path), nested_selection.clone());

    assert_eq!(state.selection_for_view(Some(&group_path)), Some(nested_selection));
    assert_eq!(state.selection_for_view(None), Some(root_selection));
    assert_eq!(
        state.last_selection,
        Some(InlineListSelection::ConfigAction("settings:set:agent.todo_planning_mode:toggle".to_string(),))
    );

    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");
    let items = build_settings_items(&state, &draft).expect("settings items");
    let removed_selection = InlineListSelection::ConfigAction("settings:set:agent.removed:cycle".to_string());
    state.selection_by_view.remove(&group_path);
    let fallback = preferred_settings_selection(&state, &items, Some(removed_selection));
    let first_setting = items.iter().find_map(|item| {
        let InlineListSelection::ConfigAction(action) = item.selection.as_ref()? else {
            return item.selection.clone();
        };
        (!matches!(action.as_str(), ACTION_BACK | ACTION_OPEN_ROOT | ACTION_RELOAD))
            .then(|| item.selection.clone())
            .flatten()
    });
    assert_eq!(fallback, first_setting);
    let back_fallback =
        preferred_settings_selection(&state, &items, Some(InlineListSelection::ConfigAction(ACTION_BACK.to_string())));
    assert_eq!(back_fallback, first_setting);
}
#[test]
fn reset_confirmation_lists_only_confirm_and_cancel_actions() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(RESET_CONFIRMATION_VIEW.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("reset confirmation items");
    let actions = items.iter().filter_map(|item| item.selection.as_ref()).collect::<Vec<_>>();
    assert_eq!(actions.len(), 2);
    assert!(actions.contains(&&InlineListSelection::ConfigAction(ACTION_RESET_CONFIRM.to_string())));
    assert!(actions.contains(&&InlineListSelection::ConfigAction(ACTION_RESET_CANCEL.to_string())));
}
#[test]
fn parse_field_docs_has_known_entry() {
    assert!(FIELD_DOCS.lookup("agent.provider").is_some());
    assert!(FIELD_DOCS.lookup("provider_overrides.openai.base_url").is_some());
    assert!(
        FIELD_DOCS
            .lookup("custom_providers[0].profiles.gpt-5-mini.temperature")
            .is_some()
    );
}
