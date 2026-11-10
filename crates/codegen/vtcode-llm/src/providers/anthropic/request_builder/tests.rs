use super::*;
use crate::provider::{AnthropicRequestOverrides, AnthropicThinkingModeOverride, FallbackModel, Message, ToolChoice};
use vtcode_config::constants::models::anthropic;
use vtcode_config::core::AdvisorConfig;

fn convert(request: &LLMRequest) -> Value {
    convert_with(request, &AnthropicConfig::default(), false)
}

/// Converts against the first-party API, where server-side fallbacks apply.
fn convert_first_party(request: &LLMRequest) -> Value {
    convert_with(request, &AnthropicConfig::default(), true)
}

fn convert_with(request: &LLMRequest, anthropic_config: &AnthropicConfig, first_party: bool) -> Value {
    let prompt_cache_settings = AnthropicPromptCacheSettings::default();
    let ctx = RequestBuilderContext {
        prompt_cache_enabled: false,
        prompt_cache_settings: &prompt_cache_settings,
        anthropic_config,
        model: anthropic::DEFAULT_MODEL,
        server_side_fallbacks_available: first_party,
    };
    convert_to_anthropic_format(request, &ctx).expect("payload conversion")
}

fn user_request(model: &str) -> LLMRequest {
    LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    }
}

fn config_with_fallbacks(fallbacks: AnthropicFallbacks) -> AnthropicConfig {
    AnthropicConfig { fallbacks, ..AnthropicConfig::default() }
}

#[test]
fn default_config_requests_default_fallbacks_on_supporting_models() {
    for model in [
        anthropic::CLAUDE_OPUS_5_5,
        anthropic::CLAUDE_OPUS_5,
        anthropic::CLAUDE_FABLE_5,
        anthropic::CLAUDE_FABLE_5_1,
        anthropic::CLAUDE_FABLE_5_2,
    ] {
        let payload = convert_with(&user_request(model), &AnthropicConfig::default(), true);
        assert_eq!(payload["fallbacks"], json!("default"), "{model}");
    }
}

#[test]
fn config_fallbacks_are_omitted_for_unsupported_models_off_and_third_party_endpoints() {
    let default_config = AnthropicConfig::default();
    for model in [anthropic::CLAUDE_SONNET_5, "claude-3-5-haiku-latest", "MiniMax-M2"] {
        let payload = convert_with(&user_request(model), &default_config, true);
        assert!(payload.get("fallbacks").is_none(), "{model} has no fallback profile");
    }

    let off = config_with_fallbacks(AnthropicFallbacks::Mode(AnthropicFallbackMode::Off));
    let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &off, true);
    assert!(payload.get("fallbacks").is_none(), "\"off\" sends nothing");

    let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &default_config, false);
    assert!(payload.get("fallbacks").is_none(), "non-first-party endpoints send nothing");
}

#[test]
fn credit_token_retry_does_not_request_further_fallbacks() {
    let mut request = user_request(anthropic::CLAUDE_OPUS_5);
    request.fallback_credit_token = Some("tok".to_string());
    let payload = convert_with(&request, &AnthropicConfig::default(), true);
    assert!(payload.get("fallbacks").is_none());
    assert_eq!(payload["fallback_credit_token"], "tok");
}

#[test]
fn configured_fallback_list_is_sanitized_per_model() {
    use vtcode_config::core::AnthropicFallbackTarget;
    let config = config_with_fallbacks(AnthropicFallbacks::Models(vec![
        AnthropicFallbackTarget {
            model: " claude-opus-4-8 ".to_string(),
            max_tokens: Some(32_000),
        },
        AnthropicFallbackTarget {
            model: anthropic::CLAUDE_OPUS_5.to_string(),
            max_tokens: None,
        },
    ]));
    let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &config, true);

    // Opus 5.5 defaults to `display: "updates"`, which neither fallback
    // model accepts, so each entry gets an explicit valid override.
    assert_eq!(payload["thinking"], json!({ "type": "adaptive", "display": "updates" }));
    assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
    assert_eq!(payload["fallbacks"][0]["max_tokens"], 32_000);
    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
    assert_eq!(payload["fallbacks"][1]["model"], anthropic::CLAUDE_OPUS_5);
    assert!(payload["fallbacks"][1].get("max_tokens").is_none());
    assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
}

#[test]
fn invalid_configured_fallback_list_sends_nothing() {
    use vtcode_config::core::AnthropicFallbackTarget;
    let target = AnthropicFallbackTarget {
        model: "claude-opus-4-8".to_string(),
        max_tokens: None,
    };
    let config = config_with_fallbacks(AnthropicFallbacks::Models(vec![target.clone(), target]));
    let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &config, true);
    assert!(payload.get("fallbacks").is_none());
}

#[test]
fn request_fallbacks_override_config_fallbacks() {
    let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback("claude-opus-4-8", None)]);
    let payload = convert_with(&request, &AnthropicConfig::default(), true);
    assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
}

fn fallback(model: &str, thinking: Option<AnthropicThinkingConfig>) -> FallbackModel {
    FallbackModel {
        model: model.to_string(),
        max_tokens: None,
        thinking,
    }
}

fn request_with_fallbacks(model: &str, fallbacks: Vec<FallbackModel>) -> LLMRequest {
    LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        fallbacks: Some(fallbacks),
        ..Default::default()
    }
}

fn fallback_thinking(payload: &Value, index: usize) -> &Value {
    &payload["fallbacks"][index]["thinking"]
}

#[test]
fn inherited_updates_display_is_dropped_for_fallbacks_without_progress_updates() {
    // Opus 5.5 defaults to `display: "updates"`; Sonnet 5 rejects it.
    let request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5_5,
        vec![
            fallback(anthropic::CLAUDE_SONNET_5, None),
            fallback(anthropic::CLAUDE_FABLE_5, None),
        ],
    );
    let payload = convert_first_party(&request);

    assert_eq!(payload["thinking"], json!({ "type": "adaptive", "display": "updates" }));
    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
    assert!(payload["fallbacks"][1].get("thinking").is_none(), "Fable 5 accepts the inherited display");
}

#[test]
fn explicit_updates_display_is_parsed_for_fallbacks() {
    let request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5,
        vec![
            fallback(
                anthropic::CLAUDE_OPUS_5_5,
                Some(AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
            ),
            fallback(
                anthropic::CLAUDE_OPUS_5,
                Some(AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
            ),
        ],
    );
    let payload = convert_first_party(&request);

    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive", "display": "updates" }));
    assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
}

#[test]
fn fallback_manual_budget_becomes_adaptive_for_models_without_budget_support() {
    let request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5,
        vec![fallback(
            anthropic::CLAUDE_OPUS_5_5,
            Some(AnthropicThinkingConfig::Enabled {
                budget_tokens: 8192,
                display: Some("summarized".to_string()),
            }),
        )],
    );
    let payload = convert_first_party(&request);

    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive", "display": "summarized" }));
}

#[test]
fn fallback_disabled_thinking_becomes_adaptive_for_adaptive_only_models() {
    let request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5,
        vec![
            fallback(anthropic::CLAUDE_OPUS_5_5, Some(AnthropicThinkingConfig::Disabled)),
            fallback(anthropic::CLAUDE_FABLE_5_1, Some(AnthropicThinkingConfig::Disabled)),
            fallback(anthropic::CLAUDE_SONNET_5, Some(AnthropicThinkingConfig::Disabled)),
        ],
    );
    let payload = convert_first_party(&request);

    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
    assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
    // Sonnet 5 accepts disabled thinking, so the override is kept.
    assert_eq!(fallback_thinking(&payload, 2), &json!({ "type": "disabled" }));
}

#[test]
fn fallback_inheriting_rejected_disabled_thinking_gets_explicit_adaptive() {
    let mut request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5,
        vec![
            fallback(anthropic::CLAUDE_OPUS_5_5, None),
            fallback(anthropic::CLAUDE_OPUS_5, None),
        ],
    );
    request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
        thinking_mode: AnthropicThinkingModeOverride::Disabled,
        ..Default::default()
    });
    let payload = convert_first_party(&request);

    assert_eq!(payload["thinking"], json!({ "type": "disabled" }));
    // Opus 5.5 would inherit the rejected `disabled` config.
    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
    // Opus 5 accepts disabled thinking at its default effort, so it inherits.
    assert!(payload["fallbacks"][1].get("thinking").is_none());
}

#[test]
fn fallback_without_override_inherits_valid_primary_thinking() {
    let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)]);
    let payload = convert_first_party(&request);

    assert_eq!(payload["thinking"]["type"], "adaptive");
    assert!(payload["fallbacks"][0].get("thinking").is_none());
}

#[test]
fn fallback_thinking_is_kept_for_unprofiled_models() {
    let request = request_with_fallbacks(
        anthropic::CLAUDE_OPUS_5,
        vec![fallback(
            "claude-unlisted-model",
            Some(AnthropicThinkingConfig::Enabled { budget_tokens: 4096, display: None }),
        )],
    );
    let payload = convert_first_party(&request);

    assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "enabled", "budget_tokens": 4096 }));
}

#[test]
fn request_fallbacks_are_gated_like_config_fallbacks() {
    let fallbacks = || vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)];

    // Models without server-side fallback support never send them.
    for model in ["claude-unlisted-model", anthropic::CLAUDE_SONNET_5] {
        let request = request_with_fallbacks(model, fallbacks());
        let payload = convert_first_party(&request);
        assert!(payload.get("fallbacks").is_none(), "{model}: {payload}");
    }

    let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks());
    assert!(convert(&request).get("fallbacks").is_none(), "non-first-party endpoints send nothing");

    let mut request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks());
    request.fallback_credit_token = Some("tok".to_string());
    assert!(convert_first_party(&request).get("fallbacks").is_none(), "credit-token retries send nothing");
}

#[test]
fn empty_request_fallbacks_send_nothing() {
    let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, Vec::new());
    let payload = convert_first_party(&request);

    assert!(payload.get("fallbacks").is_none(), "payload: {payload}");
}

#[test]
fn invalid_request_fallbacks_send_nothing() {
    let too_many = vec![
        fallback("claude-opus-4-8", None),
        fallback(anthropic::CLAUDE_OPUS_5_5, None),
        fallback(anthropic::CLAUDE_FABLE_5, None),
        fallback(anthropic::CLAUDE_FABLE_5_1, None),
    ];
    let duplicated = vec![fallback("claude-opus-4-8", None), fallback(" claude-opus-4-8 ", None)];
    let blank = vec![fallback("  ", None)];
    let zero_max_tokens = vec![FallbackModel {
        max_tokens: Some(0),
        ..fallback("claude-opus-4-8", None)
    }];
    for fallbacks in [too_many, duplicated, blank, zero_max_tokens] {
        let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks);
        let payload = convert_first_party(&request);
        assert!(payload.get("fallbacks").is_none(), "payload: {payload}");
    }
}

#[test]
fn request_fallback_models_are_trimmed() {
    let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback(" claude-opus-4-8 ", None)]);
    let payload = convert_first_party(&request);

    assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
}

#[test]
fn sampling_parameters_are_never_sent() {
    // API version 2026-10-21 rejects `temperature`, `top_p`, and `top_k`
    // outright, so the wire request must never carry them.
    let mut request = request_with_fallbacks("claude-unlisted-model", vec![fallback("claude-other-model", None)]);
    request.temperature = Some(0.2);
    request.top_p = Some(0.9);
    request.top_k = Some(40);
    let payload = convert(&request);

    for field in ["temperature", "top_p", "top_k"] {
        assert!(payload.get(field).is_none(), "{field} must be omitted: {payload}");
    }
}

fn plain_request(model: &str) -> LLMRequest {
    LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    }
}

#[test]
fn claude_5_models_default_to_64k_max_tokens() {
    for model in [
        anthropic::CLAUDE_OPUS_5_5,
        anthropic::CLAUDE_SONNET_5,
        anthropic::CLAUDE_OPUS_5,
    ] {
        let payload = convert(&plain_request(model));
        assert_eq!(payload["max_tokens"], 64_000, "{model}: {payload}");
    }
}

#[test]
fn opus_5_5_keeps_64k_max_tokens_when_thinking_field_is_omitted() {
    // Opus 5.5 rejects `disabled`, so a Disabled override omits the field;
    // the model still thinks adaptively and needs the full default budget.
    let mut request = plain_request(anthropic::CLAUDE_OPUS_5_5);
    request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
        thinking_mode: AnthropicThinkingModeOverride::Disabled,
        ..Default::default()
    });
    let payload = convert(&request);

    assert!(payload.get("thinking").is_none(), "payload: {payload}");
    assert_eq!(payload["max_tokens"], 64_000, "payload: {payload}");
}

#[test]
fn explicit_max_tokens_is_kept_for_claude_5_models() {
    let mut request = plain_request(anthropic::CLAUDE_OPUS_5_5);
    request.max_tokens = Some(2048);
    let payload = convert(&request);

    assert_eq!(payload["max_tokens"], 2048, "payload: {payload}");
}

#[test]
fn unprofiled_model_without_thinking_keeps_legacy_max_tokens_default() {
    let payload = convert(&plain_request("claude-unlisted-model"));

    assert!(payload.get("thinking").is_none(), "payload: {payload}");
    assert_eq!(payload["max_tokens"], 4096, "payload: {payload}");
}

#[test]
fn coding_agent_settings_never_inject_prompt_scaffolding() {
    // Settings serialized by older builds still deserialize; the removed
    // role / XML-tag / <thinking>-<answer> scaffolding fields are ignored.
    let settings: crate::provider::CodingAgentSettings = serde_json::from_value(json!({
        "force_xml_tags": true,
        "role_specialization": "Senior Software Architect",
        "enforce_structured_thought": true,
        "long_context_optimization": false
    }))
    .expect("legacy settings deserialize");
    let mut request = plain_request(anthropic::CLAUDE_SONNET_5);
    request.system_prompt = Some(std::sync::Arc::from("Base prompt"));
    request.coding_agent_settings = Some(Box::new(settings));
    let payload = convert(&request);

    let system = payload["system"].to_string();
    assert!(system.contains("Base prompt"), "system: {system}");
    for scaffold in [
        "You are Senior Software Architect",
        "XML tags",
        "<thinking>",
        "<answer>",
    ] {
        assert!(!system.contains(scaffold), "unexpected {scaffold:?} in system: {system}");
    }
}

fn forced_tool_request(model: &str, disable_thinking: bool) -> LLMRequest {
    let mut request = plain_request(model);
    request.tool_choice = Some(ToolChoice::any());
    if disable_thinking {
        request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
            thinking_mode: AnthropicThinkingModeOverride::Disabled,
            ..Default::default()
        });
    }
    request
}

#[test]
fn forced_tool_choice_is_downgraded_for_models_that_reject_it_even_without_thinking() {
    for model in [
        anthropic::CLAUDE_OPUS_5_5,
        anthropic::CLAUDE_FABLE_5_1,
        anthropic::CLAUDE_FABLE_5_2,
    ] {
        let payload = convert(&forced_tool_request(model, true));

        assert!(payload.get("thinking").is_none(), "{model}: {payload}");
        assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "{model}: {payload}");
    }
}

#[test]
fn forced_tool_choice_is_kept_when_thinking_is_disabled() {
    let payload = convert(&forced_tool_request(anthropic::CLAUDE_SONNET_5, true));

    assert_eq!(payload["thinking"], json!({"type": "disabled"}), "payload: {payload}");
    assert_eq!(payload["tool_choice"], json!({"type": "any"}), "payload: {payload}");
}

#[test]
fn forced_tool_choice_is_downgraded_when_thinking_is_on() {
    let payload = convert(&forced_tool_request(anthropic::CLAUDE_SONNET_5, false));

    assert_eq!(payload["thinking"]["type"], "adaptive", "payload: {payload}");
    assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
}

#[test]
fn forced_tool_choice_is_kept_for_unprofiled_models_without_thinking() {
    let mut request = plain_request("claude-unlisted-model");
    request.tool_choice = Some(ToolChoice::function("get_weather".to_string()));
    let payload = convert(&request);

    assert_eq!(payload["tool_choice"], json!({"type": "tool", "name": "get_weather"}), "payload: {payload}");
}

#[test]
fn forced_tool_choice_is_downgraded_when_a_fallback_model_rejects_it() {
    let mut request = forced_tool_request(anthropic::CLAUDE_OPUS_5, true);
    let payload = convert_first_party(&request);
    assert_eq!(payload["tool_choice"], json!({"type": "any"}), "baseline payload: {payload}");

    request.fallbacks = Some(vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)]);
    let payload = convert_first_party(&request);

    assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
}

#[test]
fn forced_tool_choice_is_downgraded_when_a_fallback_thinks() {
    let mut request = forced_tool_request(anthropic::CLAUDE_OPUS_5, true);
    request.fallbacks = Some(vec![fallback(
        "claude-opus-4-8",
        Some(AnthropicThinkingConfig::Adaptive { display: None }),
    )]);
    let payload = convert_first_party(&request);

    assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
}

fn advisor_config(enabled: bool, model: &str, max_uses: Option<u32>) -> AdvisorConfig {
    AdvisorConfig {
        enabled,
        model: model.to_string(),
        max_uses,
        max_tokens: None,
        caching: None,
    }
}

#[test]
fn resolve_advisor_tool_disabled_returns_none() {
    let cfg = advisor_config(false, "", None);
    assert!(resolve_advisor_tool("claude-sonnet-5", &cfg).is_none());
}

#[test]
fn resolve_advisor_tool_non_anthropic_executor_returns_none() {
    let cfg = advisor_config(true, "", None);
    assert!(resolve_advisor_tool("gpt-4o", &cfg).is_none());
}

#[test]
fn resolve_advisor_tool_defaults_to_valid_pair() {
    let cfg = advisor_config(true, "", None);
    let tool = resolve_advisor_tool("claude-sonnet-5", &cfg);
    assert!(matches!(tool, Some(AnthropicTool::Advisor(_))));
    if let Some(AnthropicTool::Advisor(t)) = tool {
        assert_eq!(t.model, "claude-opus-5");
        assert_eq!(t.name, "advisor");
        assert_eq!(t.tool_type, "advisor_20260301");
    }
}

#[test]
fn resolve_advisor_tool_invalid_pair_returns_none() {
    // Advisor less capable than the executor must be rejected.
    let cfg = advisor_config(true, "claude-sonnet-5", None);
    assert!(resolve_advisor_tool("claude-opus-5", &cfg).is_none());
}

#[test]
fn resolve_advisor_tool_accepts_self_advising_model() {
    let cfg = advisor_config(true, "claude-fable-5", None);
    assert!(resolve_advisor_tool("claude-fable-5", &cfg).is_some());
    // Fable may only advise Fable.
    assert!(resolve_advisor_tool("claude-opus-5", &cfg).is_none());
}

#[test]
fn resolve_advisor_tool_supports_dated_executor_suffix() {
    let cfg = advisor_config(true, "", None);
    // A dated pin for a currently supported model must not break the
    // supported-model check.
    assert!(resolve_advisor_tool("claude-sonnet-5-20251001", &cfg).is_some());
}

#[test]
fn resolve_advisor_tool_enforces_fable_5_2_advisor_rules() {
    // Default: a Fable 5.2 executor self-advises, because the API accepts only
    // Fable 5.2 or Mythos 5.2 advisors (and Mythos 5.2 is not offered here).
    let default_cfg = advisor_config(true, "", None);
    assert!(anthropic::default_advisor_model("claude-fable-5-2") == "claude-fable-5-2");
    assert!(resolve_advisor_tool("claude-fable-5-2", &default_cfg).is_some());

    // Any other advisor, Fable 5.1 included, is rejected for a Fable 5.2
    // executor.
    for advisor in ["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5"] {
        let cfg = advisor_config(true, advisor, None);
        assert!(
            resolve_advisor_tool("claude-fable-5-2", &cfg).is_none(),
            "{advisor} must not advise a Fable 5.2 executor"
        );
    }

    // A Fable 5.2 advisor is accepted by every executor model.
    let cfg = advisor_config(true, "claude-fable-5-2", None);
    assert!(resolve_advisor_tool("claude-sonnet-5", &cfg).is_some());
}
