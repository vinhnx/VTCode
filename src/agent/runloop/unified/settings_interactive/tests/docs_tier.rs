use super::*;

#[test]
fn agent_view_hides_deprecated_auto_permissions_field() {
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
    assert!(!items.iter().any(|item| item.title == "Autonomous Execution"));
}
#[test]
fn provider_openai_view_includes_missing_service_tier_doc_field() {
    let state = SettingsPaletteState {
        workspace: PathBuf::from("."),
        source_path: PathBuf::from("vtcode.toml"),
        source_label: None,
        draft: VTCodeConfig::default(),
        view_path: Some("provider.openai".to_string()),
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    };
    let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

    let items = build_settings_items(&state, &draft).expect("settings items");
    assert!(items.iter().any(|item| item.title == "Service Tier"));
}
#[test]
fn render_commented_config_includes_section_heading() {
    let mut config = VTCodeConfig::default();
    config.agent.default_model = "gpt-5.6-sol".to_string();

    let rendered = render_commented_config(&config).expect("config should render");
    assert!(rendered.contains("# Agent Defaults"));
    assert!(rendered.contains("[agent]"));
}
#[test]
fn render_commented_config_quotes_dynamic_table_keys() {
    let mut config = VTCodeConfig::default();
    config.mcp.allowlist.providers.insert(
        "provider.with.dot".to_string(),
        vtcode_config::mcp::McpAllowListRules {
            tools: Some(vec!["*".to_string()]),
            ..Default::default()
        },
    );

    let rendered = render_commented_config(&config).expect("config should render");
    assert!(rendered.contains("[mcp.allowlist.providers.\"provider.with.dot\"]"));

    let reparsed: VTCodeConfig = toml::from_str(&rendered).expect("rendered config should remain valid TOML");
    assert!(reparsed.mcp.allowlist.providers.contains_key("provider.with.dot"));
}
#[test]
fn missing_service_tier_cycle_creates_value() {
    assert_eq!(cycle_service_tier_from(None), Some(vtcode_config::OpenAIServiceTier::Flex));
}
#[test]
fn service_tier_cycle_advances_from_flex_to_priority() {
    assert_eq!(
        cycle_service_tier_from(Some(vtcode_config::OpenAIServiceTier::Flex)),
        Some(vtcode_config::OpenAIServiceTier::Priority)
    );
}
#[test]
fn service_tier_cycle_advances_from_priority_to_ultrafast() {
    assert_eq!(
        cycle_service_tier_from(Some(vtcode_config::OpenAIServiceTier::Priority)),
        Some(vtcode_config::OpenAIServiceTier::Ultrafast)
    );
}
#[test]
fn service_tier_cycle_wraps_from_ultrafast_to_flex() {
    assert_eq!(
        cycle_service_tier_from(Some(vtcode_config::OpenAIServiceTier::Ultrafast)),
        Some(vtcode_config::OpenAIServiceTier::Flex)
    );
}
