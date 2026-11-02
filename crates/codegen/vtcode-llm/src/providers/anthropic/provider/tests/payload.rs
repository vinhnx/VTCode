use super::*;

/// The wire request is pinned to the current Messages API version and never
/// carries the sampling parameters that version removed.
#[tokio::test]
async fn generate_sends_current_api_version_without_sampling_parameters() {
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("anthropic-version", "2026-10-21"))
        .and(body_partial_json(json!({"model": models::anthropic::CLAUDE_SONNET_5_5})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::anthropic::CLAUDE_SONNET_5_5.to_string(),
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let request = LLMRequest {
        model: models::anthropic::CLAUDE_SONNET_5_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    for field in ["temperature", "top_p", "top_k"] {
        assert!(payload.get(field).is_none(), "{field} must be omitted: {payload}");
    }

    let response = LLMProvider::generate(&provider, request).await.expect("request should succeed");
    assert_eq!(response.content.as_deref(), Some("ok"));
}
#[test]
fn opus_5_5_default_payload_requests_default_fallbacks_with_matching_beta() {
    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = first_party_provider(model);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["fallbacks"], json!("default"));
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "server-side-fallback-2026-07-01"), "{betas:?}");
    assert!(!betas.iter().any(|beta| beta == "server-side-fallback-2026-06-01"), "{betas:?}");
    // The default-form beta already grants the fallback-credit fields.
    assert!(!betas.iter().any(|beta| beta == "fallback-credit-2026-07-01"), "{betas:?}");
}
/// The profile promotion and its beta header must travel together: a
/// budget-continuation request keeps 5m configured TTLs but emits a 1h
/// messages breakpoint from the profile TTL, so the request must also ask
/// for the extended-cache-TTL beta.
#[test]
fn budget_continuation_breakpoint_and_extended_ttl_beta_stay_paired() {
    let model = models::anthropic::DEFAULT_MODEL;
    let mut provider = first_party_provider(model);
    provider.prompt_cache_enabled = true;
    provider.prompt_cache_settings.tools_ttl_seconds = 300;
    provider.prompt_cache_settings.messages_ttl_seconds = 300;
    // `extended_ttl_seconds` keeps its 1h default: the profile TTL.

    let request = LLMRequest {
        model: model.to_string(),
        system_prompt: Some(std::sync::Arc::from("stable system instructions")),
        messages: vec![Message::user("resume ".repeat(60))].into(),
        prompt_cache_profile: Some(crate::provider::PromptCacheProfile::BudgetContinuation),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(
        payload["messages"][0]["content"][0]["cache_control"]["ttl"], "1h",
        "the profile TTL promotes the messages breakpoint"
    );
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "extended-cache-ttl-2025-04-11"), "{betas:?}");
}
#[test]
fn configured_fallback_list_uses_list_form_and_credit_betas() {
    let model = models::anthropic::CLAUDE_OPUS_5;
    let mut provider = first_party_provider(model);
    provider.anthropic_config.fallbacks =
        vtcode_config::core::AnthropicFallbacks::Models(vec![vtcode_config::core::AnthropicFallbackTarget {
            model: "claude-opus-4-8".to_string(),
            max_tokens: None,
        }]);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "server-side-fallback-2026-06-01"), "{betas:?}");
    assert!(!betas.iter().any(|beta| beta == "server-side-fallback-2026-07-01"), "{betas:?}");
    // The list form does not grant the credit fields, so the original
    // request carries the credit beta for a refusal to return a token.
    assert!(betas.iter().any(|beta| beta == "fallback-credit-2026-07-01"), "{betas:?}");
}
#[test]
fn unprofiled_and_off_payloads_send_no_fallbacks_or_beta() {
    let unprofiled = first_party_provider("claude-3-5-haiku-latest");
    let request = LLMRequest {
        model: "claude-3-5-haiku-latest".to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let payload = unprofiled.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload.get("fallbacks").is_none());
    let betas = split_betas(unprofiled.beta_header_for_request(&request, &payload, false, None));
    assert!(!betas.iter().any(|beta| beta.starts_with("server-side-fallback")), "{betas:?}");

    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let mut off = first_party_provider(model);
    off.anthropic_config.fallbacks =
        vtcode_config::core::AnthropicFallbacks::Mode(vtcode_config::core::AnthropicFallbackMode::Off);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let payload = off.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload.get("fallbacks").is_none());
    let betas = split_betas(off.beta_header_for_request(&request, &payload, false, None));
    assert!(!betas.iter().any(|beta| beta.starts_with("server-side-fallback")), "{betas:?}");
}
