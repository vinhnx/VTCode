use super::{
    AnthropicConfig, AnthropicFallbackMode, AnthropicFallbackTarget, AnthropicFallbacks, OpenAIConfig,
    OpenAIHostedShellConfig, OpenAIHostedShellDomainSecret, OpenAIHostedShellEnvironment,
    OpenAIHostedShellNetworkPolicy, OpenAIHostedShellNetworkPolicyType, OpenAIHostedSkill, OpenAIHostedSkillVersion,
    OpenAIManualCompactionConfig, OpenAIServiceTier, OpenRouterAttributionConfig, OpenRouterConfig,
    ToolSearchAlgorithm,
};

#[test]
fn anthropic_effort_is_unset_unless_configured() {
    assert_eq!(AnthropicConfig::default().effort, None);

    let parsed: AnthropicConfig = toml::from_str("").expect("empty anthropic config");
    assert_eq!(parsed.effort, None);

    let parsed: AnthropicConfig = toml::from_str("effort = \"high\"").expect("explicit effort");
    assert_eq!(parsed.effort, Some(super::ReasoningEffortLevel::High));
}

/// `OpenAIServiceTier` keeps its `as_str()`/`parse()`/serde paths in
/// lockstep; see [`crate::test_support::assert_string_enum_lockstep`].
#[test]
fn openai_service_tier_string_paths_stay_in_lockstep() {
    use crate::test_support::assert_string_enum_lockstep;

    assert_string_enum_lockstep!(
        OpenAIServiceTier,
        [
            OpenAIServiceTier::Flex,
            OpenAIServiceTier::Priority,
            OpenAIServiceTier::Ultrafast,
        ]
    );
}

#[test]
fn anthropic_fallbacks_default_to_default_mode() {
    let config = AnthropicConfig::default();
    assert_eq!(config.fallbacks, AnthropicFallbacks::Mode(AnthropicFallbackMode::Default));
    let parsed: AnthropicConfig = toml::from_str("").expect("config should parse");
    assert_eq!(parsed.fallbacks, AnthropicFallbacks::Mode(AnthropicFallbackMode::Default));
}

#[test]
fn anthropic_fallbacks_parse_all_three_forms() {
    let parsed: AnthropicConfig = toml::from_str("fallbacks = \"default\"").expect("default form");
    assert_eq!(parsed.fallbacks, AnthropicFallbacks::Mode(AnthropicFallbackMode::Default));

    let parsed: AnthropicConfig = toml::from_str("fallbacks = \"off\"").expect("off form");
    assert_eq!(parsed.fallbacks, AnthropicFallbacks::Mode(AnthropicFallbackMode::Off));

    let parsed: AnthropicConfig = toml::from_str(
        "fallbacks = [{ model = \"claude-opus-4-8\", max_tokens = 32000 }, { model = \"claude-opus-5\" }]",
    )
    .expect("list form");
    assert_eq!(
        parsed.fallbacks,
        AnthropicFallbacks::Models(vec![
            AnthropicFallbackTarget {
                model: "claude-opus-4-8".to_string(),
                max_tokens: Some(32_000),
            },
            AnthropicFallbackTarget {
                model: "claude-opus-5".to_string(),
                max_tokens: None,
            },
        ])
    );
    assert_eq!(parsed.fallbacks.validation_error("fallbacks"), None);

    assert!(toml::from_str::<AnthropicConfig>("fallbacks = \"sometimes\"").is_err());
}

#[test]
fn anthropic_fallbacks_round_trip_through_serde() {
    for fallbacks in [
        AnthropicFallbacks::Mode(AnthropicFallbackMode::Default),
        AnthropicFallbacks::Mode(AnthropicFallbackMode::Off),
        AnthropicFallbacks::Models(vec![AnthropicFallbackTarget {
            model: "claude-opus-4-8".to_string(),
            max_tokens: None,
        }]),
    ] {
        let json = serde_json::to_value(&fallbacks).expect("serialize");
        let back: AnthropicFallbacks = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, fallbacks);
    }
    assert_eq!(
        serde_json::to_value(AnthropicFallbacks::default()).expect("serialize"),
        serde_json::json!("default")
    );
}

#[test]
fn anthropic_fallback_list_validation_rejects_bad_lists() {
    let target = |model: &str| AnthropicFallbackTarget { model: model.to_string(), max_tokens: None };
    for bad in [
        AnthropicFallbacks::Models(vec![]),
        AnthropicFallbacks::Models(vec![target("a"), target("b"), target("c"), target("d")]),
        AnthropicFallbacks::Models(vec![target("  ")]),
        AnthropicFallbacks::Models(vec![target("a"), target(" a ")]),
        AnthropicFallbacks::Models(vec![AnthropicFallbackTarget { model: "a".to_string(), max_tokens: Some(0) }]),
    ] {
        assert!(bad.validation_error("fallbacks").is_some(), "{bad:?}");
    }
}

#[test]
fn openai_config_defaults_to_websocket_mode_disabled() {
    let config = OpenAIConfig::default();
    assert!(!config.websocket_mode);
    assert_eq!(config.responses_store, None);
    assert!(config.responses_include.is_empty());
    assert_eq!(config.service_tier, None);
    assert_eq!(config.manual_compaction, OpenAIManualCompactionConfig::default());
    assert_eq!(config.hosted_shell, OpenAIHostedShellConfig::default());
    assert!(config.tool_search.enabled);
    assert!(config.tool_search.defer_by_default);
    assert!(config.tool_search.always_available_tools.is_empty());
}

#[test]
fn anthropic_config_defaults_native_memory_to_disabled() {
    let config = AnthropicConfig::default();
    assert!(!config.memory.enabled);
}

#[test]
fn anthropic_config_parses_native_memory_opt_in() {
    let parsed: AnthropicConfig = toml::from_str("[memory]\nenabled = true").expect("config should parse");
    assert!(parsed.memory.enabled);
}

#[test]
fn openai_config_parses_websocket_mode_opt_in() {
    let parsed: OpenAIConfig = toml::from_str("websocket_mode = true").expect("config should parse");
    assert!(parsed.websocket_mode);
    assert_eq!(parsed.responses_store, None);
    assert!(parsed.responses_include.is_empty());
    assert_eq!(parsed.service_tier, None);
    assert_eq!(parsed.manual_compaction, OpenAIManualCompactionConfig::default());
    assert_eq!(parsed.hosted_shell, OpenAIHostedShellConfig::default());
    assert_eq!(parsed.tool_search, super::OpenAIToolSearchConfig::default());
}

#[test]
fn openai_config_parses_responses_options() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
responses_store = false
responses_include = ["reasoning.encrypted_content", "output_text.annotations"]
"#,
    )
    .expect("config should parse");
    assert_eq!(parsed.responses_store, Some(false));
    assert_eq!(
        parsed.responses_include,
        vec![
            "reasoning.encrypted_content".to_string(),
            "output_text.annotations".to_string()
        ]
    );
    assert_eq!(parsed.service_tier, None);
    assert_eq!(parsed.manual_compaction, OpenAIManualCompactionConfig::default());
    assert_eq!(parsed.hosted_shell, OpenAIHostedShellConfig::default());
}

#[test]
fn openai_config_parses_manual_compaction_defaults() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
[manual_compaction]
instructions = "Preserve the bug reproduction steps."
"#,
    )
    .expect("config should parse");

    assert_eq!(parsed.manual_compaction.instructions.as_deref(), Some("Preserve the bug reproduction steps."));
}

#[test]
fn openai_config_parses_service_tier() {
    let parsed: OpenAIConfig = toml::from_str(r#"service_tier = "priority""#).expect("config should parse");
    assert_eq!(parsed.service_tier, Some(OpenAIServiceTier::Priority));
}

#[test]
fn openai_config_parses_flex_service_tier() {
    let parsed: OpenAIConfig = toml::from_str(r#"service_tier = "flex""#).expect("config should parse");
    assert_eq!(parsed.service_tier, Some(OpenAIServiceTier::Flex));
}

#[test]
fn openai_config_parses_ultrafast_service_tier() {
    let parsed: OpenAIConfig = toml::from_str(r#"service_tier = "ultrafast""#).expect("config should parse");
    assert_eq!(parsed.service_tier, Some(OpenAIServiceTier::Ultrafast));
    assert_eq!(OpenAIServiceTier::Ultrafast.as_str(), "ultrafast");
    assert_eq!(OpenAIServiceTier::parse("ULTRAFAST"), Some(OpenAIServiceTier::Ultrafast));
}

#[test]
fn openai_config_parses_hosted_shell() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
[hosted_shell]
enabled = true
environment = "container_auto"
file_ids = ["file_123"]

[[hosted_shell.skills]]
type = "skill_reference"
skill_id = "skill_123"
"#,
    )
    .expect("config should parse");

    assert!(parsed.hosted_shell.enabled);
    assert_eq!(parsed.hosted_shell.environment, OpenAIHostedShellEnvironment::ContainerAuto);
    assert_eq!(parsed.hosted_shell.file_ids, vec!["file_123".to_string()]);
    assert_eq!(
        parsed.hosted_shell.skills,
        vec![OpenAIHostedSkill::SkillReference {
            skill_id: "skill_123".to_string(),
            version: OpenAIHostedSkillVersion::default(),
        }]
    );
}

#[test]
fn openai_config_parses_hosted_shell_pinned_version_and_inline_bundle() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
[hosted_shell]
enabled = true

[[hosted_shell.skills]]
type = "skill_reference"
skill_id = "skill_123"
version = 2

[[hosted_shell.skills]]
type = "inline"
bundle_b64 = "UEsFBgAAAAAAAA=="
sha256 = "deadbeef"
"#,
    )
    .expect("config should parse");

    assert_eq!(
        parsed.hosted_shell.skills,
        vec![
            OpenAIHostedSkill::SkillReference {
                skill_id: "skill_123".to_string(),
                version: OpenAIHostedSkillVersion::Number(2),
            },
            OpenAIHostedSkill::Inline {
                bundle_b64: "UEsFBgAAAAAAAA==".to_string(),
                sha256: Some("deadbeef".to_string()),
            },
        ]
    );
}

#[test]
fn openai_config_parses_hosted_shell_network_policy() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
[hosted_shell]
enabled = true

[hosted_shell.network_policy]
type = "allowlist"
allowed_domains = ["httpbin.org"]

[[hosted_shell.network_policy.domain_secrets]]
domain = "httpbin.org"
name = "API_KEY"
value = "debug-secret-123"
"#,
    )
    .expect("config should parse");

    assert_eq!(
        parsed.hosted_shell.network_policy,
        OpenAIHostedShellNetworkPolicy {
            policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
            allowed_domains: vec!["httpbin.org".to_string()],
            domain_secrets: vec![OpenAIHostedShellDomainSecret {
                domain: "httpbin.org".to_string(),
                name: "API_KEY".to_string(),
                value: "debug-secret-123".to_string(),
            }],
        }
    );
}

#[test]
fn openai_config_parses_tool_search() {
    let parsed: OpenAIConfig = toml::from_str(
        r#"
[tool_search]
enabled = false
defer_by_default = false
always_available_tools = ["code_search", "custom_tool"]
"#,
    )
    .expect("config should parse");

    assert!(!parsed.tool_search.enabled);
    assert!(!parsed.tool_search.defer_by_default);
    assert_eq!(
        parsed.tool_search.always_available_tools,
        vec!["code_search".to_string(), "custom_tool".to_string()]
    );
}

#[test]
fn anthropic_tool_search_defaults_to_enabled() {
    let config = AnthropicConfig::default();

    assert!(config.tool_search.enabled);
    assert!(config.tool_search.defer_by_default);
    assert_eq!(config.tool_search.algorithm, ToolSearchAlgorithm::Regex);
    assert!(config.tool_search.always_available_tools.is_empty());
}

#[test]
fn hosted_shell_container_reference_requires_non_empty_container_id() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerReference,
        container_id: Some("   ".to_string()),
        file_ids: Vec::new(),
        skills: Vec::new(),
        network_policy: OpenAIHostedShellNetworkPolicy::default(),
    };

    assert!(!config.has_valid_reference_target());
    assert!(config.container_id_ref().is_none());
}

#[test]
fn hosted_shell_reports_invalid_skill_reference_mounts() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerAuto,
        container_id: None,
        file_ids: Vec::new(),
        skills: vec![OpenAIHostedSkill::SkillReference {
            skill_id: "   ".to_string(),
            version: OpenAIHostedSkillVersion::default(),
        }],
        network_policy: OpenAIHostedShellNetworkPolicy::default(),
    };

    let message = config.first_invalid_skill_message().expect("invalid mount should be reported");

    assert!(message.contains("provider.openai.hosted_shell.skills[0].skill_id"));
    assert!(!config.has_valid_skill_mounts());
    assert!(!config.is_valid_for_runtime());
}

#[test]
fn hosted_shell_ignores_skill_validation_for_container_reference() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerReference,
        container_id: Some("cntr_123".to_string()),
        file_ids: Vec::new(),
        skills: vec![OpenAIHostedSkill::Inline { bundle_b64: "   ".to_string(), sha256: None }],
        network_policy: OpenAIHostedShellNetworkPolicy::default(),
    };

    assert!(config.first_invalid_skill_message().is_none());
    assert!(config.has_valid_skill_mounts());
    assert!(config.is_valid_for_runtime());
}

#[test]
fn hosted_shell_reports_invalid_allowlist_without_domains() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerAuto,
        container_id: None,
        file_ids: Vec::new(),
        skills: Vec::new(),
        network_policy: OpenAIHostedShellNetworkPolicy {
            policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
            allowed_domains: Vec::new(),
            domain_secrets: Vec::new(),
        },
    };

    let message = config
        .first_invalid_network_policy_message()
        .expect("invalid network policy should be reported");

    assert!(message.contains("network_policy.allowed_domains"));
    assert!(!config.has_valid_network_policy());
    assert!(!config.is_valid_for_runtime());
}

#[test]
fn hosted_shell_reports_domain_secret_outside_allowlist() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerAuto,
        container_id: None,
        file_ids: Vec::new(),
        skills: Vec::new(),
        network_policy: OpenAIHostedShellNetworkPolicy {
            policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
            allowed_domains: vec!["pypi.org".to_string()],
            domain_secrets: vec![OpenAIHostedShellDomainSecret {
                domain: "httpbin.org".to_string(),
                name: "API_KEY".to_string(),
                value: "secret".to_string(),
            }],
        },
    };

    let message = config
        .first_invalid_network_policy_message()
        .expect("invalid domain secret should be reported");

    assert!(message.contains("domain_secrets[0].domain"));
    assert!(!config.has_valid_network_policy());
}

#[test]
fn hosted_shell_ignores_network_policy_validation_for_container_reference() {
    let config = OpenAIHostedShellConfig {
        enabled: true,
        environment: OpenAIHostedShellEnvironment::ContainerReference,
        container_id: Some("cntr_123".to_string()),
        file_ids: Vec::new(),
        skills: Vec::new(),
        network_policy: OpenAIHostedShellNetworkPolicy {
            policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
            allowed_domains: Vec::new(),
            domain_secrets: Vec::new(),
        },
    };

    assert!(config.first_invalid_network_policy_message().is_none());
    assert!(config.has_valid_network_policy());
}

/// Stash `VTCODE_OPENROUTER_ATTRIBUTION` for hermetic attribution tests
/// (serialized via `serial` + the process-wide env lock; restored after).
fn stash_attribution_env() -> (vtcode_commons::env_lock::EnvGuard, Option<std::ffi::OsString>) {
    let guard = vtcode_commons::env_lock::lock();
    let prior = std::env::var_os(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION);
    guard.remove_var(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION);
    (guard, prior)
}

fn restore_attribution_env(guard: &vtcode_commons::env_lock::EnvGuard, prior: Option<std::ffi::OsString>) {
    guard.restore_var(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION, prior);
}

#[test]
#[serial_test::serial]
fn openrouter_attribution_defaults_to_disabled() {
    let (env, prior) = stash_attribution_env();
    let config = OpenRouterAttributionConfig::default();
    assert!(!config.enabled);
    assert!(!config.is_enabled());
    assert_eq!(config.resolved_referer(), OpenRouterAttributionConfig::DEFAULT_REFERER);
    assert_eq!(config.resolved_title(), OpenRouterAttributionConfig::DEFAULT_TITLE);
    assert_eq!(OpenRouterAttributionConfig::CATEGORIES, "cli-agent");

    let parsed: OpenRouterConfig = toml::from_str("").expect("empty openrouter config");
    assert!(!parsed.attribution.enabled);
    assert!(!parsed.attribution.is_enabled());
    restore_attribution_env(&env, prior);
}

#[test]
#[serial_test::serial]
fn openrouter_attribution_parses_opt_in_with_overrides() {
    let (env, prior) = stash_attribution_env();
    let parsed: OpenRouterConfig = toml::from_str(
        r#"
[attribution]
enabled = true
referer = "https://example.com/my-fork"
title = "My Fork"
"#,
    )
    .expect("config should parse");
    assert!(parsed.attribution.enabled);
    assert!(parsed.attribution.is_enabled());
    assert_eq!(parsed.attribution.resolved_referer(), "https://example.com/my-fork");
    assert_eq!(parsed.attribution.resolved_title(), "My Fork");

    // Blank overrides fall back to defaults rather than sending empty headers.
    let blank = OpenRouterAttributionConfig {
        enabled: true,
        referer: Some("   ".to_string()),
        title: Some("".to_string()),
    };
    assert_eq!(blank.resolved_referer(), OpenRouterAttributionConfig::DEFAULT_REFERER);
    assert_eq!(blank.resolved_title(), OpenRouterAttributionConfig::DEFAULT_TITLE);
    restore_attribution_env(&env, prior);
}

#[test]
#[serial_test::serial]
fn openrouter_attribution_env_layer_wins_over_file() {
    // Asymmetric both directions: env enables a disabled file, and env
    // disables an enabled file (env > file precedence).
    let (env, prior) = stash_attribution_env();
    let disabled = OpenRouterAttributionConfig::default();
    env.set_var(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION, "yes");
    assert!(disabled.is_enabled());
    env.set_var(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION, "0");
    let enabled = OpenRouterAttributionConfig { enabled: true, referer: None, title: None };
    assert!(!enabled.is_enabled());
    // Unrecognized or blank env falls through to the file value.
    env.set_var(crate::constants::env_vars::VTCODE_OPENROUTER_ATTRIBUTION, "maybe");
    assert!(enabled.is_enabled());
    assert!(!disabled.is_enabled());
    restore_attribution_env(&env, prior);
}

#[test]
fn openrouter_attribution_rejects_header_unsafe_overrides() {
    // CRLF/CTL injection attempts fall back to defaults instead of
    // reaching reqwest header construction.
    let injected = OpenRouterAttributionConfig {
        enabled: true,
        referer: Some("https://example.com/\r\nX-Injected: 1".to_string()),
        title: Some("bad\ttitle\x7F".to_string()),
    };
    assert_eq!(injected.resolved_referer(), OpenRouterAttributionConfig::DEFAULT_REFERER);
    assert_eq!(injected.resolved_title(), OpenRouterAttributionConfig::DEFAULT_TITLE);

    let oversized = OpenRouterAttributionConfig {
        enabled: true,
        referer: Some(format!("https://example.com/{}", "a".repeat(600))),
        title: None,
    };
    assert_eq!(oversized.resolved_referer(), OpenRouterAttributionConfig::DEFAULT_REFERER);
}
