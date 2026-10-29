use super::*;

#[test]
fn permission_view_summary_includes_mode_and_rule_counts() {
    let mut config = VTCodeConfig::default();
    config.permissions.allow = vec!["Read".to_string()];
    config.permissions.ask = vec!["Bash".to_string(), "Write".to_string()];
    config.permissions.deny = vec!["Edit".to_string()];

    let summary = format_permission_summary(&config);
    assert!(summary.contains("Rules"));
    assert!(summary.contains("deny 1"));
    assert!(summary.contains("ask 2"));
    assert!(summary.contains("allow 1"));
}
#[test]
fn settings_header_shows_target_source_and_breadcrumbs() {
    for view in [
        None,
        Some("group:model_provider"),
        Some(SETTINGS_ADVANCED_VIEW_PATH),
        Some("advanced.agent.harness.max_tool_calls_per_turn"),
        Some("permissions"),
        Some(RESET_CONFIRMATION_VIEW),
    ] {
        let state = header_test_state(view);
        let lines = settings_header_lines(&state);
        assert!(lines.iter().any(|line| line.contains("Writing to vtcode.toml")));
        assert!(lines.iter().any(|line| line == "test"));
        assert!(lines.iter().all(|line| !line.contains("Enter") && !line.contains("Esc")));
    }
    let root = settings_header_lines(&header_test_state(None));
    assert_eq!(root.first().map(String::as_str), Some("Pick a group to edit."));
    let advanced = settings_header_lines(&header_test_state(Some(SETTINGS_ADVANCED_VIEW_PATH)));
    assert!(advanced.iter().any(|line| line.contains("Advanced settings")));
}
#[test]
fn settings_header_omits_source_notice_when_config_file_exists() {
    let mut state = header_test_state(None);
    state.source_label = None;
    let lines = settings_header_lines(&state);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines.first().map(String::as_str), Some("Pick a group to edit."));
    assert!(lines[1].starts_with("Writing to "));
    assert!(lines.iter().all(|line| !line.contains("Configuration source")));
}
#[test]
fn settings_search_config_enables_fuzzy_with_view_specific_placeholder() {
    let state = header_test_state(None);
    let config = settings_search_config(&state);
    assert!(config.fuzzy);
    assert_eq!(config.placeholder.as_deref(), Some(SETTINGS_SEARCH_PLACEHOLDER));

    let advanced = header_test_state(Some(SETTINGS_ADVANCED_VIEW_PATH));
    let config = settings_search_config(&advanced);
    assert!(config.fuzzy);
    assert_eq!(config.placeholder.as_deref(), Some(ADVANCED_SEARCH_PLACEHOLDER));
}
#[test]
fn short_display_path_prefers_workspace_relative_then_home() {
    let workspace = Path::new("/repo");
    assert_eq!(short_display_path(Path::new("/repo/vtcode.toml"), workspace), "vtcode.toml");
    assert_eq!(short_display_path(Path::new("/repo/sub/dir/vtcode.toml"), workspace), "sub/dir/vtcode.toml");
    if let Some(home) = dirs::home_dir() {
        assert_eq!(short_display_path(&home.join("vtcode.toml"), Path::new("/elsewhere")), "~/vtcode.toml");
    }
    assert_eq!(
        short_display_path(Path::new("/opt/other/vtcode.toml"), Path::new("/elsewhere")),
        "/opt/other/vtcode.toml"
    );
}
#[test]
fn desktop_config_catalog_reuses_cli_advanced_fields_and_curated_groups() {
    let config = VTCodeConfig::default();
    let draft = TomlValue::try_from(config.clone()).expect("default config draft");
    let fields = desktop_config_catalog(&config).expect("desktop CLI config catalog");
    let default_model = fields
        .iter()
        .find(|field| field.path == "agent.default_model")
        .expect("default model field");
    assert_eq!(default_model.group, "Model & Provider");
    assert!(!default_model.description.is_empty());
    assert!(fields.len() > 100, "all typed and documented CLI settings are included");
    assert!(fields.iter().any(|field| !field.options.is_empty()));
    let optional_storage = fields
        .iter()
        .find(|field| field.path == "agent.checkpointing.storage_dir")
        .expect("unset documented CLI setting");
    assert!(optional_storage.type_name.contains("string"));
    assert!(optional_storage.editable, "typed optional fields can be initialized from Desktop");
    let theme = fields
        .iter()
        .find(|field| field.path == "agent.theme")
        .expect("CLI theme field");
    assert_eq!(
        theme.options,
        mutations::resolve_cycle_options(Some(&draft), "agent.theme", "ciapre"),
        "dynamic CLI choices are shared with the desktop editor"
    );
    for group in items::curated_groups() {
        assert!(
            fields.iter().any(|field| field.group == group.title),
            "desktop catalog should include CLI group {}",
            group.title
        );
    }
    assert!(
        fields
            .iter()
            .filter(|field| items::is_schema_path(&field.path))
            .all(|field| !field.editable),
        "wildcard schema paths remain reference-only"
    );
    assert!(desktop_type_is_editable("null | string"));
    assert!(!desktop_type_is_editable("PointConfig"));
    assert!(
        fields
            .iter()
            .all(|field| !field.path.contains('[') && !field.path.contains(']'))
    );
}
