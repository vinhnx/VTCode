use super::*;

#[test]
fn test_insert_dotted_key() {
    let mut table = toml::Table::new();
    ConfigBuilder::insert_dotted_key(&mut table, "a.b.c", toml::Value::String("value".to_string()))
        .expect("insert_dotted_key should succeed for a.b.c");

    let a = table.get("a").unwrap().as_table().unwrap();
    let b = a.get("b").unwrap().as_table().unwrap();
    let c = b.get("c").unwrap().as_str().unwrap();
    assert_eq!(c, "value");
}

#[test]
fn test_merge_toml_values() {
    let mut base = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "openai"
            [tools]
            default_policy = "prompt"
        "#,
    )
    .unwrap();

    let overlay = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "anthropic"
            default_model = "claude-3"
        "#,
    )
    .unwrap();

    merge_toml_values(&mut base, &overlay);

    let agent = base.get("agent").unwrap().as_table().unwrap();
    assert_eq!(agent.get("provider").unwrap().as_str().unwrap(), "anthropic");
    assert_eq!(agent.get("default_model").unwrap().as_str().unwrap(), "claude-3");

    let tools = base.get("tools").unwrap().as_table().unwrap();
    assert_eq!(tools.get("default_policy").unwrap().as_str().unwrap(), "prompt");
}

#[test]
fn test_merge_toml_values_with_origins_tracks_winning_layer() {
    use crate::loader::layers::ConfigLayerMetadata;

    let mut base = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "openai"
        "#,
    )
    .unwrap();

    let overlay = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "anthropic"
        "#,
    )
    .unwrap();

    let layer = ConfigLayerMetadata {
        name: "workspace:/tmp/vtcode.toml".to_string(),
        version: "abc123".to_string(),
    };
    let mut origins = hashbrown::HashMap::new();
    merge_toml_values_with_origins(&mut base, &overlay, &mut origins, &layer);

    assert_eq!(
        base.get("agent")
            .and_then(|agent| agent.get("provider"))
            .and_then(toml::Value::as_str),
        Some("anthropic")
    );
    assert_eq!(origins.get("agent.provider"), Some(&layer));
}

#[test]
#[serial]
fn syntax_highlighting_defaults_are_valid() {
    let config = SyntaxHighlightingConfig::default();
    config.validate().expect("default syntax highlighting config should be valid");
    // Default is empty — all syntect grammars enabled
    assert!(
        config.enabled_languages.is_empty(),
        "default enabled_languages should be empty to allow all syntect grammars"
    );
}

#[test]
fn vtcode_config_validation_fails_for_invalid_highlight_timeout() {
    let mut config = VTCodeConfig::default();
    config.syntax_highlighting.highlight_timeout_ms = 0;
    let error = config
        .validate()
        .expect_err("validation should fail for zero highlight timeout");
    assert!(format!("{error:#}").contains("highlight"), "expected error to mention highlight, got: {error:#}");
}

#[test]
fn load_from_file_rejects_invalid_syntax_highlighting() {
    let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
    writeln!(temp_file, "[syntax_highlighting]\nhighlight_timeout_ms = 0\n").expect("failed to write temp config");

    let result = ConfigManager::load_from_file(temp_file.path());
    assert!(result.is_err(), "expected validation error");
    let error = format!("{:?}", result.err().unwrap());
    assert!(error.contains("validate"), "expected validation context in error, got: {error}");
}

#[test]
fn custom_providers_fields_round_trip_through_toml() {
    let mut config = VTCodeConfig::default();
    config.custom_providers.push(CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.corp.example/v1".to_string(),
        api_format: CustomProviderApiFormat::OpenAIChat,
        context_window: Some(256_000),
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        supports_tools: Some(true),
        supports_reasoning: Some(false),
        supports_reasoning_effort: Some(true),
        supports_vision: Some(false),
        supports_structured_output: Some(true),
        supports_parallel_tool_calls: Some(false),
        supports_context_caching: Some(true),
        supports_responses_compaction: Some(false),
        supports_prompt_cache_options: None,
        supports_context_edits: Some(true),
        api_key_env: "MYCORP_API_KEY".to_string(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: vec!["gpt-5-mini".to_string(), "gpt-5-large".to_string()],
        profiles: std::collections::BTreeMap::from([(
            "gpt-5-mini".to_string(),
            CustomProviderProfileConfig {
                api_format: CustomProviderApiFormat::OpenAIResponses,
                context_window: None,
                temperature: Some(0.2),
                top_p: Some(0.9),
                top_k: Some(40),
                presence_penalty: Some(0.1),
                frequency_penalty: Some(-0.5),
                max_tokens: Some(4_096),
                reasoning_effort: Some(ReasoningEffortLevel::Low),
                supports_tools: Some(false),
                supports_reasoning: None,
                supports_reasoning_effort: None,
                supports_vision: None,
                supports_structured_output: None,
                supports_parallel_tool_calls: None,
                supports_context_caching: None,
                supports_responses_compaction: None,
                supports_prompt_cache_options: None,
                supports_context_edits: None,
            },
        )]),
    });

    let serialized = toml::to_string(&config).expect("serialize config");
    let parsed: VTCodeConfig = toml::from_str(&serialized).expect("parse config");

    parsed.validate().expect("custom provider config should validate");
    assert_eq!(parsed.custom_providers.len(), 1);

    let provider = &parsed.custom_providers[0];
    assert_eq!(provider.name, "mycorp");
    assert_eq!(provider.display_name, "MyCorp");
    assert_eq!(provider.base_url, "https://llm.corp.example/v1");
    assert_eq!(provider.api_format, CustomProviderApiFormat::OpenAIChat);
    assert_eq!(provider.context_window, Some(256_000));
    assert_eq!(provider.supports_tools, Some(true));
    assert_eq!(provider.supports_reasoning, Some(false));
    assert_eq!(provider.supports_reasoning_effort, Some(true));
    assert_eq!(provider.supports_vision, Some(false));
    assert_eq!(provider.supports_structured_output, Some(true));
    assert_eq!(provider.supports_parallel_tool_calls, Some(false));
    assert_eq!(provider.supports_context_caching, Some(true));
    assert_eq!(provider.supports_responses_compaction, Some(false));
    assert_eq!(provider.supports_context_edits, Some(true));
    assert_eq!(provider.api_key_env, "MYCORP_API_KEY");
    assert_eq!(provider.model, "gpt-5-mini");
    assert_eq!(provider.models, vec!["gpt-5-mini".to_string(), "gpt-5-large".to_string()]);
    assert_eq!(
        provider.profiles["gpt-5-mini"],
        CustomProviderProfileConfig {
            api_format: CustomProviderApiFormat::OpenAIResponses,
            context_window: None,
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(-0.5),
            max_tokens: Some(4_096),
            reasoning_effort: Some(ReasoningEffortLevel::Low),
            supports_tools: Some(false),
            supports_reasoning: None,
            supports_reasoning_effort: None,
            supports_vision: None,
            supports_structured_output: None,
            supports_parallel_tool_calls: None,
            supports_context_caching: None,
            supports_responses_compaction: None,
            supports_prompt_cache_options: None,
            supports_context_edits: None,
        }
    );
    let profile = &provider.profiles["gpt-5-mini"];
    assert_eq!(profile.temperature, Some(0.2));
    assert_eq!(profile.top_p, Some(0.9));
    assert_eq!(profile.top_k, Some(40));
    assert_eq!(profile.presence_penalty, Some(0.1));
    assert_eq!(profile.frequency_penalty, Some(-0.5));
    assert_eq!(profile.max_tokens, Some(4_096));
    assert_eq!(profile.reasoning_effort, Some(ReasoningEffortLevel::Low));
}

#[test]
fn custom_providers_nested_profiles_parse_exactly() {
    let config: VTCodeConfig = toml::from_str(
        r#"
[[custom_providers]]
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.corp.example/v1"
api_format = "openai-chat"
context_window = 256000
supports_tools = true

[custom_providers.profiles."gpt-5-mini"]
supports_tools = false
supports_parallel_tool_calls = true
"#,
    )
    .expect("nested custom provider config should parse");

    let provider = config.custom_providers.first().expect("provider should exist");
    assert_eq!(provider.api_format, CustomProviderApiFormat::OpenAIChat);
    assert_eq!(provider.profile("gpt-5-mini").unwrap().supports_tools, Some(false));
    assert!(provider.profile("gpt-5").is_none());
    assert_eq!(
        provider.resolved_profile("gpt-5-mini"),
        crate::core::ResolvedCustomProviderProfile {
            api_format: Some(CustomProviderApiFormat::OpenAIChat),
            context_window: Some(256_000),
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            supports_tools: Some(false),
            supports_reasoning: None,
            supports_reasoning_effort: None,
            supports_vision: None,
            supports_structured_output: None,
            supports_parallel_tool_calls: Some(true),
            supports_context_caching: None,
            supports_responses_compaction: None,
            supports_prompt_cache_options: None,
            supports_context_edits: None,
        }
    );
}

#[test]
fn custom_providers_profile_sampling_overrides_resolve_with_provider_defaults() {
    let config: VTCodeConfig = toml::from_str(
        r#"
[[custom_providers]]
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.corp.example/v1"
temperature = 0.5
top_p = 0.95

[custom_providers.profiles."cold-model"]
temperature = 0.0
reasoning_effort = "low"

[custom_providers.profiles."hot-model"]
max_tokens = 8192
"#,
    )
    .expect("nested custom provider sampling overrides should parse");

    let provider = config.custom_providers.first().expect("provider should exist");
    let cold = provider.resolved_profile("cold-model");
    assert_eq!(cold.temperature, Some(0.0));
    // Provider-level top_p applies where the model profile does not override it.
    assert_eq!(cold.top_p, Some(0.95));
    assert_eq!(cold.reasoning_effort, Some(ReasoningEffortLevel::Low));
    assert_eq!(cold.max_tokens, None);

    let hot = provider.resolved_profile("hot-model");
    assert_eq!(hot.temperature, Some(0.5));
    assert_eq!(hot.max_tokens, Some(8_192));
}

#[test]
fn custom_providers_reject_out_of_range_sampling_overrides() {
    for (field, value) in [
        ("temperature", "2.5"),
        ("temperature", "-0.1"),
        ("top_p", "1.5"),
        ("presence_penalty", "-3.0"),
        ("frequency_penalty", "2.1"),
    ] {
        let config: Result<VTCodeConfig, _> = toml::from_str(&format!(
            r#"
[[custom_providers]]
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.corp.example/v1"

[custom_providers.profiles."m"]
{field} = {value}
"#
        ));
        let config = config.expect("config with out-of-range value should still parse");
        let error = config.custom_providers[0]
            .validate()
            .err()
            .unwrap_or_else(|| panic!("{field}={value} should fail validation"));
        assert!(error.contains(field), "validation error should mention {field}: {error}");
    }

    let zero_max: VTCodeConfig = toml::from_str(
        r#"
[[custom_providers]]
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.corp.example/v1"

[custom_providers.profiles."m"]
max_tokens = 0
"#,
    )
    .expect("zero max_tokens should parse");
    assert!(zero_max.validate().is_err(), "max_tokens=0 must fail validation");
}

#[test]
fn loader_loads_prompt_cache_retention_from_toml() {
    use std::fs::File;
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vtcode.toml");
    let mut file = File::create(&path).unwrap();
    let contents = r#"
[prompt_cache]
enabled = true
cache_friendly_prompt_shaping = true
[prompt_cache.providers.openai]
prompt_cache_retention = "24h"
prompt_cache_key_mode = "off"
"#;
    file.write_all(contents.as_bytes()).unwrap();

    let manager = ConfigManager::load_from_file(&path).unwrap();
    let config = manager.config();
    assert_eq!(config.prompt_cache.providers.openai.prompt_cache_retention, Some(PromptCacheRetention::H24));
    assert_eq!(
        config.prompt_cache.providers.openai.prompt_cache_key_mode,
        crate::core::OpenAIPromptCacheKeyMode::Off
    );
    assert!(config.prompt_cache.cache_friendly_prompt_shaping);
}

#[test]
fn loader_loads_tools_editor_config_from_toml() {
    use std::fs::File;
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vtcode.toml");
    let mut file = File::create(&path).unwrap();
    let contents = r#"
[tools.editor]
enabled = true
preferred_editor = "code --wait"
suspend_tui = false
"#;
    file.write_all(contents.as_bytes()).unwrap();

    let manager = ConfigManager::load_from_file(&path).unwrap();
    let config = manager.config();
    assert!(config.tools.editor.enabled);
    assert_eq!(config.tools.editor.preferred_editor, "code --wait");
    assert!(!config.tools.editor.suspend_tui);
}
