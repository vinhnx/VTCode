use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{
    CustomProviderApiFormat, CustomProviderCommandAuthConfig, CustomProviderConfig, CustomProviderProfileConfig,
    ResolvedCustomProviderProfile, default_auth_refresh_interval_ms, default_auth_timeout_ms,
};

#[test]
fn validate_accepts_lowercase_provider_name() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles: BTreeMap::new(),
    };

    assert!(config.validate().is_ok());
    assert_eq!(config.resolved_api_key_env(), "MYCORP_API_KEY");
}

#[test]
fn validate_rejects_invalid_provider_name() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "My Corp".to_string(),
        display_name: "My Corp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles: BTreeMap::new(),
    };

    let err = config.validate().expect_err("invalid name should fail");
    assert!(err.contains("must use lowercase letters, digits, hyphens, or underscores"));
}

#[test]
fn validate_rejects_auth_and_api_key_env_together() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: "MYCORP_API_KEY".to_string(),
        auth: Some(CustomProviderCommandAuthConfig {
            command: "print-token".to_string(),
            args: Vec::new(),
            cwd: None,
            timeout_ms: default_auth_timeout_ms(),
            refresh_interval_ms: default_auth_refresh_interval_ms(),
        }),
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles: BTreeMap::new(),
    };

    let err = config.validate().expect_err("conflicting auth should fail");
    assert!(err.contains("`auth` cannot be combined with `api_key_env`"));
}

#[test]
fn validate_accepts_command_auth_without_static_env_key() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: Some(CustomProviderCommandAuthConfig {
            command: "print-token".to_string(),
            args: vec!["--json".to_string()],
            cwd: Some(PathBuf::from("/tmp")),
            timeout_ms: 1_000,
            refresh_interval_ms: 60_000,
        }),
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles: BTreeMap::new(),
    };

    assert!(config.validate().is_ok());
    assert!(config.uses_command_auth());
}

#[test]
fn validate_rejects_empty_model_entry_in_models_list() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: "MYCORP_API_KEY".to_string(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: vec!["valid-model".to_string(), "   ".to_string()],
        profiles: BTreeMap::new(),
    };

    let err = config.validate().expect_err("blank models entry should fail");
    assert!(err.contains("`models` entries must not be empty"));
}

#[test]
fn validate_rejects_zero_context_window() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: Some(0),
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles: BTreeMap::new(),
    };

    let err = config.validate().expect_err("zero context window should fail");
    assert!(err.contains("`context_window` must be greater than 0"));
}

#[test]
fn validate_rejects_malformed_profile_key() {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        " gpt-5-mini ".to_string(),
        CustomProviderProfileConfig {
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            api_format: CustomProviderApiFormat::Auto,
            context_window: Some(128_000),
            supports_tools: None,
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
    );

    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles,
    };

    let err = config.validate().expect_err("profile key with whitespace should fail");
    assert!(err.contains("profile key"));
}

#[test]
fn effective_models_uses_models_list_when_present() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "atlascloud".to_string(),
        display_name: "Atlas Cloud".to_string(),
        base_url: "https://api.atlascloud.ai/v1".to_string(),
        api_format: CustomProviderApiFormat::Auto,
        context_window: None,
        supports_tools: None,
        supports_reasoning: None,
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: "ATLASCLOUD_API_KEY".to_string(),
        auth: None,
        model: "deepseek-ai/deepseek-flash".to_string(),
        models: vec![
            "deepseek-ai/deepseek-flash".to_string(),
            "deepseek-ai/deepseek-v4-pro".to_string(),
            "deepseek-ai/DeepSeek-V3-0324".to_string(),
            "qwen/qwen3.6-35b-a3b".to_string(),
            "moonshotai/kimi-k2.7-code".to_string(),
            "moonshotai/kimi-k2.6".to_string(),
            "zai-org/glm-5.2".to_string(),
            "minimaxai/minimax-m3".to_string(),
        ],
        profiles: BTreeMap::new(),
    };

    assert_eq!(
        config.effective_models(),
        vec![
            "deepseek-ai/deepseek-flash".to_string(),
            "deepseek-ai/deepseek-v4-pro".to_string(),
            "deepseek-ai/DeepSeek-V3-0324".to_string(),
            "qwen/qwen3.6-35b-a3b".to_string(),
            "moonshotai/kimi-k2.7-code".to_string(),
            "moonshotai/kimi-k2.6".to_string(),
            "zai-org/glm-5.2".to_string(),
            "minimaxai/minimax-m3".to_string(),
        ]
    );
}

#[test]
fn effective_models_falls_back_to_single_model_field() {
    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        model: "gpt-5-mini".to_string(),
        ..CustomProviderConfig::default()
    };

    assert_eq!(config.effective_models(), vec!["gpt-5-mini".to_string()]);
}

#[test]
fn resolved_profile_prefers_exact_model_key() {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "gpt-5-mini".to_string(),
        CustomProviderProfileConfig {
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            api_format: CustomProviderApiFormat::OpenAIResponses,
            context_window: Some(128_000),
            supports_tools: Some(true),
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
    );

    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::OpenAIChat,
        context_window: Some(256_000),
        supports_tools: Some(true),
        supports_reasoning: Some(true),
        supports_reasoning_effort: None,
        supports_vision: None,
        supports_structured_output: None,
        supports_parallel_tool_calls: None,
        supports_context_caching: None,
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles,
    };

    let resolved = config.resolved_profile("gpt-5-mini");
    assert_eq!(
        resolved,
        ResolvedCustomProviderProfile {
            api_format: Some(CustomProviderApiFormat::OpenAIResponses),
            context_window: Some(128_000),
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            supports_tools: Some(true),
            supports_reasoning: Some(true),
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
    assert!(config.profile("gpt-5").is_none());
}

#[test]
fn sparse_inheritance_preserves_provider_defaults() {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "gpt-5-mini".to_string(),
        CustomProviderProfileConfig {
            temperature: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            max_tokens: None,
            reasoning_effort: None,
            api_format: CustomProviderApiFormat::Auto,
            context_window: None,
            supports_tools: Some(false),
            supports_reasoning: None,
            supports_reasoning_effort: Some(true),
            supports_vision: None,
            supports_structured_output: None,
            supports_parallel_tool_calls: None,
            supports_context_caching: None,
            supports_responses_compaction: None,
            supports_prompt_cache_options: None,
            supports_context_edits: None,
        },
    );

    let config = CustomProviderConfig {
        temperature: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        max_tokens: None,
        reasoning_effort: None,
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.example/v1".to_string(),
        api_format: CustomProviderApiFormat::OpenAIChat,
        context_window: Some(256_000),
        supports_tools: Some(true),
        supports_reasoning: Some(false),
        supports_reasoning_effort: None,
        supports_vision: Some(true),
        supports_structured_output: None,
        supports_parallel_tool_calls: Some(true),
        supports_context_caching: Some(false),
        supports_responses_compaction: None,
        supports_prompt_cache_options: None,
        supports_context_edits: None,
        api_key_env: String::new(),
        auth: None,
        model: "gpt-5-mini".to_string(),
        models: Vec::new(),
        profiles,
    };

    let resolved = config.resolved_profile("gpt-5-mini");
    assert_eq!(resolved.api_format, Some(CustomProviderApiFormat::OpenAIChat));
    assert_eq!(resolved.context_window, Some(256_000));
    assert_eq!(resolved.supports_tools, Some(false));
    assert_eq!(resolved.supports_reasoning, Some(false));
    assert_eq!(resolved.supports_reasoning_effort, Some(true));
    assert_eq!(resolved.supports_vision, Some(true));
    assert_eq!(resolved.supports_parallel_tool_calls, Some(true));
    assert_eq!(resolved.supports_context_caching, Some(false));
    assert_eq!(resolved.supports_responses_compaction, None);
    assert_eq!(resolved.supports_context_edits, None);
}

#[test]
fn legacy_default_behavior_retains_auto_and_empty_profiles() {
    let parsed: CustomProviderConfig = toml::from_str(
        r#"
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.example/v1"
model = "gpt-5-mini"
"#,
    )
    .expect("legacy custom provider config should parse");

    assert_eq!(parsed.api_format, CustomProviderApiFormat::Auto);
    assert!(parsed.profiles.is_empty());
    assert_eq!(parsed.resolved_profile("gpt-5-mini"), ResolvedCustomProviderProfile::default());
}

#[test]
fn deserialize_rejects_invalid_api_format() {
    let err = toml::from_str::<CustomProviderConfig>(
        r#"
name = "mycorp"
display_name = "MyCorp"
base_url = "https://llm.example/v1"
api_format = "openai-chatty"
"#,
    )
    .expect_err("invalid api_format should fail");

    assert!(err.to_string().contains("openai-chatty"));
}
