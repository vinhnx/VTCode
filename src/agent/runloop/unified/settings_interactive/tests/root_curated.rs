use super::*;

#[test]
#[serial]
fn settings_palette_uses_explicit_session_override_as_source() {
    use vtcode_config::loader::set_explicit_config_path;

    struct OverrideGuard;

    impl OverrideGuard {
        fn set(path: Option<PathBuf>) -> Self {
            set_explicit_config_path(path);
            Self
        }
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            set_explicit_config_path(None);
        }
    }

    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::write(workspace.join("vtcode.toml"), "agent.provider = \"anthropic\"\n").expect("workspace config");

    let override_path = temp.path().join("custom-night.toml");
    std::fs::write(&override_path, "agent.provider = \"openai\"\n").expect("override config");

    let _guard = OverrideGuard::set(Some(override_path.clone()));
    let state = create_settings_palette_state(&workspace, None);

    let state = state.expect("settings state with override");
    assert_eq!(
        canonicalize(&state.source_path).expect("canonical source path"),
        canonicalize(&override_path).expect("canonical override path"),
        "settings palette must treat the explicit override file as its source"
    );
    assert_eq!(state.draft.agent.provider, "openai");
}
#[test]
fn root_settings_contains_only_curated_groups_and_global_actions() {
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

    let items = build_settings_items(&state, &draft).expect("settings items");
    let titles: Vec<_> = items.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Model & Provider",
            "Agent & Automation",
            "Approvals & Security",
            "Tools & Integrations",
            "Context & Memory",
            "Interface & Terminal",
            "Performance & Diagnostics",
            "Advanced settings",
            "Actions",
            "Reload configuration",
            "Reset configuration",
        ]
    );
    assert!(items.iter().all(|item| item.selection.is_some() || item.title == "Actions"));
    assert!(items.iter().all(|item| {
        item.title == "Actions"
            || item
                .subtitle
                .as_deref()
                .is_some_and(|subtitle| subtitle.contains("editable setting"))
            || matches!(item.title.as_str(), "Advanced settings" | "Reload configuration" | "Reset configuration")
    }));
}
#[test]
fn root_settings_remove_quick_access_duplicates_and_nested_fields() {
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
    let draft: TomlValue = toml::from_str(
        r#"
        [tools.editor]
        preferred_editor = "code --wait"
        "#,
    )
    .expect("valid draft value");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Tools & Integrations"));
    for duplicate in [
        "Quick Access",
        "Model Config",
        "External Editor",
        "Editor Mode",
        "Codex App Server",
    ] {
        assert!(!items.iter().any(|item| item.title == duplicate), "unexpected root item {duplicate}");
    }
    assert!(!items.iter().any(|item| item.title == "Preferred Editor"));
    let tools = items
        .iter()
        .find(|item| item.title == "Tools & Integrations")
        .expect("tools group");
    assert!(
        !tools
            .search_value
            .as_deref()
            .is_some_and(|search| search.contains("preferred_editor"))
    );
}
#[test]
fn root_settings_do_not_show_missing_nested_optional_fields() {
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

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(!items.iter().any(|item| item.title == "Service Tier"));
}
#[test]
fn nested_settings_titles_are_humanized() {
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
    let draft: TomlValue = toml::from_str(
        r#"
        [agent]
        default_model = "gpt-5.6-sol"
        [agent.circuit_breaker]
        enabled = true
        "#,
    )
    .expect("valid draft value");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Default Model"));
    assert!(items.iter().any(|item| item.title == "Circuit Breaker"));
}
