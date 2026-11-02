use super::*;

#[test]
fn test_filter_tools_no_network_policy() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: None,
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
        ToolDefinition::web_search(serde_json::json!({})),
        ToolDefinition::function("web_search".to_string(), "Search".to_string(), serde_json::json!({})),
    ];
    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function.as_ref().unwrap().name, "read_file");
}
#[test]
fn test_filter_tools_with_network_policy_updates_native_web_search() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec!["blocked.example.com".to_string()],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![ToolDefinition::web_search(serde_json::json!({
        "user_location": "US"
    }))];
    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].tool_type, "web_search");
    assert_eq!(
        filtered[0].web_search.as_ref(),
        Some(&serde_json::json!({
            "user_location": "US",
            "allowed_domains": ["api.example.com"],
            "blocked_domains": ["blocked.example.com"]
        }))
    );
}
#[test]
fn test_filter_tools_no_network_policy_removes_gemini_native_network_tools() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: None,
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::google_maps(serde_json::json!({})),
        ToolDefinition::url_context(serde_json::json!({})),
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
    ];

    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function_name(), "read_file");
}
#[test]
fn test_filter_tools_with_network_policy_drops_gemini_native_network_tools() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["example.com".to_string()],
                denied_domains: vec![],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let filtered = filter_tools_for_skill(
        &skill,
        vec![
            ToolDefinition::google_maps(serde_json::json!({})),
            ToolDefinition::url_context(serde_json::json!({})),
        ],
    );

    assert!(filtered.is_empty());
}
#[test]
fn test_filter_tools_drops_function_style_network_tools_when_policy_is_present() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec![],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::function("read_web_page".to_string(), "Read web page".to_string(), serde_json::json!({})),
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
    ];
    let filtered = filter_tools_for_skill(&skill, tools);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function_name(), "read_file");
}
#[test]
fn test_filter_tools_fails_closed_for_unrepresentable_web_search_policy() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["docs.rs".to_string()],
                denied_domains: vec!["example.com".to_string()],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let mut anthropic_web_search = ToolDefinition::web_search(serde_json::json!({}));
    anthropic_web_search.tool_type = "web_search_20250305".to_string();

    let filtered = filter_tools_for_skill(&skill, vec![anthropic_web_search]);

    assert!(filtered.is_empty());
}
