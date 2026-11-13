use super::*;

#[test]
fn curated_groups_resolve_to_their_declared_field_paths() {
    let state = SettingsPaletteState {
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
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    for group in items::curated_groups() {
        let group_state = SettingsPaletteState {
            view_path: Some(format!("{SETTINGS_GROUP_PREFIX}{}", group.id)),
            ..state.clone()
        };
        let items = build_settings_items(&group_state, &draft).expect("curated group items");
        let editable = items.iter().filter(|item| item.selection.is_some()).count();
        assert!(editable > 0, "{} should expose editable settings", group.title);
        for path in group.paths {
            assert!(
                get_node(&draft, path).is_some() || FIELD_DOCS.lookup(path).is_some_and(|doc| !doc.options.is_empty()),
                "curated path {path} should resolve or have documented options"
            );
        }
    }
}
#[test]
fn resolve_settings_view_path_maps_model_aliases() {
    assert_eq!(resolve_settings_view_path("model"), SETTINGS_MODEL_CONFIG_PATH);
    assert_eq!(resolve_settings_view_path("model.main"), SETTINGS_MODEL_CONFIG_MAIN_PATH);
    assert_eq!(resolve_settings_view_path("codex"), "agent.codex_app_server");
    assert_eq!(resolve_settings_view_path("codex_app_server"), "agent.codex_app_server");
    assert_eq!(resolve_settings_view_path("advanced"), SETTINGS_ADVANCED_VIEW_PATH);
    for group in items::curated_groups() {
        assert_eq!(resolve_settings_view_path(group.id), format!("{SETTINGS_GROUP_PREFIX}{}", group.id));
    }
    assert_eq!(
        resolve_settings_view_path("advanced.agent.harness.max_tool_wall_clock_secs"),
        "advanced.agent.harness.max_tool_wall_clock_secs"
    );
}
#[test]
fn advanced_view_indexes_every_documented_field_for_search() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Max Tool Wall Clock Secs"));
    let provider_search = items
        .iter()
        .find(|item| {
            item.search_value
                .as_deref()
                .is_some_and(|search| search.contains("agent.provider"))
        })
        .and_then(|item| item.search_value.as_deref())
        .expect("provider search entry");
    assert!(provider_search.contains(&VTCodeConfig::default().agent.provider.to_ascii_lowercase()));
    let tool_limit_search = items
        .iter()
        .find(|item| item.title == "Max Tool Wall Clock Secs")
        .and_then(|item| item.search_value.as_deref())
        .expect("tool limit search entry");
    assert!(tool_limit_search.contains("maximum wall clock time"));
    assert!(items.iter().any(|item| {
        item.search_value
            .as_deref()
            .is_some_and(|search| search.contains("flex") && search.contains("priority"))
    }));
    for path in FIELD_DOCS.sorted_paths() {
        let needle = path.to_ascii_lowercase();
        assert!(
            items
                .iter()
                .any(|item| item.search_value.as_deref().is_some_and(|search| search.contains(&needle))),
            "advanced search index is missing {path}"
        );
    }
}
#[test]
fn advanced_view_keeps_uninstantiated_wildcard_fields_read_only() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("advanced settings items");
    let wildcard = items
        .iter()
        .find(|item| {
            item.search_value
                .as_deref()
                .is_some_and(|search| search.contains("custom_providers[].api_key_env"))
        })
        .expect("wildcard provider field should remain searchable");
    assert_eq!(wildcard.badge.as_deref(), Some("Schema"));
    assert!(wildcard.selection.is_none());
}
#[test]
fn advanced_direct_path_opens_the_specific_nested_field() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("advanced.agent.harness.max_tool_wall_clock_secs".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    let entry = items
        .iter()
        .find(|item| item.title == "Max Tool Wall Clock Secs")
        .expect("advanced direct field");
    assert_eq!(
        entry.selection,
        Some(InlineListSelection::ConfigAction(
            "settings:set:agent.harness.max_tool_wall_clock_secs:inc".to_string()
        ))
    );
}
#[test]
fn curated_container_navigation_returns_to_its_group() {
    let mut state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(format!("{SETTINGS_GROUP_PREFIX}model_provider")),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("curated group items");
    let providers = items
        .iter()
        .find(|item| item.title == "Custom Providers")
        .expect("custom providers entry");
    let action = providers.selection.clone().expect("custom providers should be navigable");
    assert_eq!(
        action,
        InlineListSelection::ConfigAction("settings:open:group:model_provider:custom_providers".to_string())
    );

    let InlineListSelection::ConfigAction(action) = &action else {
        panic!("expected a config action");
    };
    apply_settings_action(&mut state, action).expect("open custom providers");
    assert_eq!(state.view_path.as_deref(), Some("group:model_provider:custom_providers"));
    apply_settings_action(&mut state, ACTION_BACK).expect("back to model provider group");
    assert_eq!(state.view_path.as_deref(), Some("group:model_provider"));
}
#[test]
fn advanced_container_navigation_returns_to_advanced_search() {
    let mut state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("advanced settings");
    let providers = items
        .iter()
        .find(|item| {
            item.search_value
                .as_deref()
                .is_some_and(|search| search.contains("custom_providers"))
                && item.selection
                    == Some(InlineListSelection::ConfigAction("settings:open:advanced.custom_providers".to_string()))
        })
        .expect("advanced custom providers entry");
    let action = providers.selection.as_ref().expect("entry action");
    let InlineListSelection::ConfigAction(action) = action else {
        panic!("expected a config action");
    };
    apply_settings_action(&mut state, action).expect("open advanced custom providers");
    assert_eq!(state.view_path.as_deref(), Some("advanced.custom_providers"));
    apply_settings_action(&mut state, ACTION_BACK).expect("back to advanced search");
    assert_eq!(state.view_path.as_deref(), Some(SETTINGS_ADVANCED_VIEW_PATH));
}
#[test]
fn advanced_view_handles_dotted_dynamic_map_keys() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft: TomlValue = toml::from_str(
        r#"
        [[custom_providers]]
        name = "mycorp"
        display_name = "MyCorp"
        base_url = "https://llm.corp.example/v1"

        [custom_providers.profiles."gpt-5.4"]
        temperature = 0.2
        "#,
    )
    .expect("valid custom provider profile");

    let items = build_settings_items(&state, &draft).expect("advanced settings with dynamic map keys");
    assert!(items.iter().any(|item| {
        item.subtitle
            .as_deref()
            .is_some_and(|subtitle| subtitle.contains(r#"custom_providers[0].profiles["gpt-5.4"].temperature"#))
    }));
}
#[test]
fn curated_group_navigation_preserves_breadcrumb_parent() {
    let mut state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some(format!("{SETTINGS_GROUP_PREFIX}tools_integrations")),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "External Editor"));
    assert_eq!(parent_view_path(&format!("{SETTINGS_GROUP_PREFIX}tools_integrations")), None);
    assert_eq!(parent_view_path("advanced.agent.harness.max_tool_wall_clock_secs"), Some("advanced".to_string()));
    assert_eq!(parent_view_path("advanced:custom_providers[0]"), Some("advanced:custom_providers".to_string()));

    apply_settings_action(&mut state, ACTION_BACK).expect("group back action");
    assert_eq!(state.view_path, None);
    state.view_path = Some("advanced.agent.harness.max_tool_wall_clock_secs".to_string());
    apply_settings_action(&mut state, ACTION_BACK).expect("advanced back action");
    assert_eq!(state.view_path.as_deref(), Some(SETTINGS_ADVANCED_VIEW_PATH));
}
