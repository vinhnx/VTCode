use super::{
    ConfigTargetResolution, ConfigValueKind, ConfigValueState, config_palette_groups, config_search_terms,
    config_setting_meta, config_value_state, resolve_config_target, setting_matches_query,
};
use serde_json::json;

fn classified_setting(value: serde_json::Value, origin: Option<&str>, default: Option<&str>) -> ConfigValueState {
    config_value_state(&super::protocol::ConfigSetting {
        path: "test.setting".into(),
        value,
        origin: origin.map(str::to_string),
        label: None,
        type_name: None,
        default_value: default.map(str::to_string),
        description: None,
        options: Vec::new(),
        group: None,
        group_description: None,
        editable: true,
    })
}

#[test]
fn config_value_state_prefers_provenance_then_documented_default() {
    use ConfigValueState::{Default, Overridden, Unset};
    // Null is unset regardless of provenance or default.
    assert_eq!(classified_setting(json!(null), Some("workspace"), Some("false")), Unset);
    assert_eq!(classified_setting(json!(null), None, None), Unset);
    // A layer origin always means explicitly applied, even at the default value.
    assert_eq!(classified_setting(json!(false), Some("workspace"), Some("false")), Overridden);
    assert_eq!(classified_setting(json!(true), Some("user"), Some("false")), Overridden);
    // Without provenance, matching the documented JSON default reads as default.
    assert_eq!(classified_setting(json!(false), None, Some("false")), Default);
    assert_eq!(classified_setting(json!("prompt"), None, Some("\"prompt\"")), Default);
    assert_eq!(classified_setting(json!([1, 2]), None, Some("[1, 2]")), Default);
    // Unquoted documented strings match string values via the plain fallback.
    assert_eq!(classified_setting(json!("prompt"), None, Some("prompt")), Default);
    // Mismatches and unparseable defaults without provenance read as overridden.
    assert_eq!(classified_setting(json!(true), None, Some("false")), Overridden);
    assert_eq!(classified_setting(json!("fast"), None, Some("prompt")), Overridden);
    assert_eq!(classified_setting(json!(42), None, Some("not json {{{")), Overridden);
    // No documented default and no provenance reads as default.
    assert_eq!(classified_setting(json!(true), None, None), Default);
    // Blank origins carry no provenance.
    assert_eq!(classified_setting(json!(false), Some("  "), Some("false")), Default);
}

fn meta_line(path: &str, origin: Option<&str>, default: Option<&str>) -> String {
    config_setting_meta(&super::protocol::ConfigSetting {
        path: path.into(),
        value: json!(true),
        origin: origin.map(str::to_string),
        label: None,
        type_name: None,
        default_value: default.map(str::to_string),
        description: None,
        options: Vec::new(),
        group: None,
        group_description: None,
        editable: true,
    })
}

#[test]
fn config_setting_meta_names_path_origin_and_default() {
    let full = meta_line("sandbox.enabled", Some("workspace"), Some("false"));
    assert!(full.contains("sandbox.enabled"), "path anchors the line: {full}");
    assert!(full.contains("workspace"), "origin tier stays visible: {full}");
    assert!(full.contains("false"), "default tier stays visible: {full}");
    let bare = meta_line("sandbox.enabled", None, None);
    assert_eq!(bare, "sandbox.enabled", "no tiers collapse to the bare path");
    let origin_only = meta_line("sandbox.enabled", Some("user"), None);
    assert!(origin_only.contains("user"), "origin without default: {origin_only}");
    assert!(!origin_only.contains("false"), "no default means no default hint: {origin_only}");
}

#[test]
fn config_editor_parses_strings_and_typed_json_without_coercion() {
    assert_eq!(ConfigValueKind::String.parse("a \"quoted\" value"), Ok(json!("a \"quoted\" value")));
    assert_eq!(ConfigValueKind::Integer.parse("42"), Ok(json!(42)));
    assert!(ConfigValueKind::Integer.parse("4.2").is_err());
    assert_eq!(ConfigValueKind::Float.parse("4.2"), Ok(json!(4.2)));
    assert_eq!(ConfigValueKind::Array.parse("[\"a\", 2]"), Ok(json!(["a", 2])));
    assert!(ConfigValueKind::Array.parse("{\"a\": 2}").is_err());
    assert_eq!(ConfigValueKind::Object.parse("{\"enabled\": true}"), Ok(json!({"enabled": true})));
}

#[test]
fn config_editor_enforces_the_shared_cli_options() {
    let editor = super::ConfigEditor {
        path: "provider.openai.service_tier".into(),
        kind: ConfigValueKind::String,
        target: super::protocol::ConfigTarget::User,
        expected_layer_version: None,
        options: vec!["auto".into(), "fast".into()],
        option_select: None,
    };
    assert_eq!(editor.parse("fast"), Ok(json!("fast")));
    assert!(editor.parse("ultrafast").is_err());
}

#[test]
fn unset_documented_fields_keep_the_cli_type_and_read_only_schema_is_not_editable() {
    let optional_string = super::protocol::ConfigSetting {
        path: "provider.openai.service_tier".into(),
        value: json!(null),
        origin: None,
        label: Some("Service tier".into()),
        type_name: Some("Option<String>".into()),
        default_value: Some("\"auto\"".into()),
        description: Some("Choose a service tier.".into()),
        options: vec!["auto".into(), "fast".into()],
        group: Some("Model & Provider".into()),
        group_description: None,
        editable: true,
    };
    assert_eq!(ConfigValueKind::for_setting(&optional_string), Some(ConfigValueKind::String));
    let optional_enum = super::protocol::ConfigSetting {
        type_name: Some("ReasoningEffortLevel | null".into()),
        options: vec!["low".into(), "high".into()],
        ..optional_string.clone()
    };
    assert_eq!(ConfigValueKind::for_setting(&optional_enum), Some(ConfigValueKind::String));
    let unknown_struct = super::protocol::ConfigSetting {
        type_name: Some("PointConfig".into()),
        options: Vec::new(),
        ..optional_string.clone()
    };
    assert_eq!(ConfigValueKind::for_setting(&unknown_struct), None);

    let read_only = super::protocol::ConfigSetting {
        path: "mcp.providers.*.command".into(),
        editable: false,
        ..optional_string.clone()
    };
    let snapshot = super::protocol::ConfigSettingsSnapshot {
        settings: vec![optional_string, read_only],
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    };
    let groups = config_palette_groups(&snapshot);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, "Model & Provider");
    assert_eq!(groups[0].1.len(), 1);
    assert_eq!(groups[0].1[0].label, "Service tier");
}

fn deep_link_fixture() -> Vec<super::protocol::ConfigSetting> {
    vec![
        super::protocol::ConfigSetting {
            path: "agent.default_model".into(),
            value: json!("model-a"),
            origin: Some("workspace".into()),
            label: Some("Default model".into()),
            type_name: Some("string".into()),
            default_value: None,
            description: Some("Model used for new conversations.".into()),
            options: Vec::new(),
            group: Some("Model & Provider".into()),
            group_description: None,
            editable: true,
        },
        super::protocol::ConfigSetting {
            path: "provider.openai.service_tier".into(),
            value: json!("auto"),
            origin: Some("user".into()),
            label: Some("Service tier".into()),
            type_name: Some("string".into()),
            default_value: Some("\"auto\"".into()),
            description: Some("Choose the request service tier.".into()),
            options: vec!["auto".into(), "fast".into()],
            group: Some("Tools".into()),
            group_description: None,
            editable: true,
        },
    ]
}

#[test]
fn config_deep_links_prefer_exact_paths_then_groups_then_search() {
    let settings = deep_link_fixture();
    assert_eq!(
        resolve_config_target(&settings, "agent.default_model"),
        ConfigTargetResolution::Setting {
            group: "Model & Provider".into(),
            path: "agent.default_model".into(),
        }
    );
    assert_eq!(
        resolve_config_target(&settings, "  tools  "),
        ConfigTargetResolution::Group { name: "Tools".into() }
    );
    // Paths are case-sensitive; groups are not. Near-misses fall
    // through to search instead of selecting the wrong destination.
    assert_eq!(
        resolve_config_target(&settings, "Agent.Default_Model"),
        ConfigTargetResolution::Search { query: "Agent.Default_Model".into() }
    );
    assert_eq!(resolve_config_target(&settings, "tool"), ConfigTargetResolution::Search { query: "tool".into() });
    assert_eq!(
        resolve_config_target(&settings, "agent.retries"),
        ConfigTargetResolution::Search { query: "agent.retries".into() }
    );
    assert_eq!(
        resolve_config_target(&[], "agent.default_model"),
        ConfigTargetResolution::Search { query: "agent.default_model".into() }
    );
}

#[test]
fn config_search_requires_every_term_across_documented_fields() {
    let settings = deep_link_fixture();
    let model = &settings[0];
    let tier = &settings[1];
    assert!(setting_matches_query(model, &config_search_terms("")));
    for (setting, term) in [
        (model, "agent.default_model"),
        (model, "Default model"),
        (model, "model-a"),
        (model, "conversations"),
        (model, "workspace"),
        (model, "Model & Provider"),
        (tier, "fast"),
        (tier, "service tier"),
        (tier, "\"auto\""),
    ] {
        assert!(
            setting_matches_query(setting, &config_search_terms(term)),
            "{term:?} must match its documented field"
        );
    }
    // Every term must match: the tier row carries both "service" and
    // "fast", while the model row carries neither "tier" term.
    assert!(setting_matches_query(tier, &config_search_terms("service fast")));
    assert!(!setting_matches_query(model, &config_search_terms("model zebra")));
    assert!(!setting_matches_query(model, &config_search_terms("service")));
    assert!(setting_matches_query(model, &config_search_terms("MODEL")));
}
