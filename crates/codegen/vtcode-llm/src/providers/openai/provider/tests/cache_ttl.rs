use super::*;
use vtcode_config::core::prompt_cache::PromptCacheTtl;

#[test]
fn prompt_cache_ttl_native_payload_and_negative_routes() {
    let mut provider = test_provider("https://api.openai.com/v1", models::openai::GPT_6_LUNA);
    let mut request = sample_request(models::openai::GPT_6_LUNA);
    let payload = provider.convert_to_openai_responses_format(&request).unwrap();
    assert_eq!(payload.pointer("/prompt_cache_options/ttl"), Some(&json!("30m")));
    provider.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    for stream in [false, true] {
        request.stream = stream;
        let payload = provider.convert_to_openai_responses_format(&request).unwrap();
        assert_eq!(payload.pointer("/prompt_cache_options/ttl"), Some(&json!("12h")));
        assert_eq!(provider.requested_prompt_cache_ttl(&request.model), Some(PromptCacheTtl::H12));
    }
    request.prompt_cache_profile = Some(provider::PromptCacheProfile::StablePrefix);
    request.system_prompt = Some("Reusable instructions".into());
    let payload = provider.convert_to_openai_responses_format(&request).unwrap();
    assert_eq!(payload.pointer("/prompt_cache_options/mode"), Some(&json!("explicit")));
    assert_eq!(payload.pointer("/input/0/content/0/prompt_cache_breakpoint/mode"), Some(&json!("explicit")));
    assert!(payload.pointer("/input/0/content/0/prompt_cache_breakpoint/ttl").is_none());
    assert!(
        provider
            .convert_to_openai_format(&request)
            .unwrap()
            .get("prompt_cache_options")
            .is_none()
    );
    provider.prompt_cache_hints_enabled = false;
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .get("prompt_cache_options")
            .is_none()
    );
    assert_eq!(provider.requested_prompt_cache_ttl(&request.model), None);
    provider.prompt_cache_hints_enabled = true;
    request.model = models::openai::GPT_5_1_MINI.into();
    provider.prompt_cache_settings.prompt_cache_retention = Some(PromptCacheRetention::H24);
    let legacy = provider.convert_to_openai_responses_format(&request).unwrap();
    assert!(legacy.get("prompt_cache_options").is_none());
    assert_eq!(legacy["prompt_cache_retention"], "24h");
    request.model = models::openai::GPT_6_LUNA.into();
    let mut subscription = chatgpt_backend_provider(&request.model);
    subscription.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    assert!(
        subscription
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .get("prompt_cache_options")
            .is_none()
    );
}

#[test]
fn prompt_cache_ttl_custom_responses_requires_explicit_capability() {
    let mut provider = test_provider("https://compatible.example/v1", "opaque-model")
        .with_api_format_override(Some(CustomProviderApiFormat::OpenAIResponses));
    provider.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    let mut request = sample_request("opaque-model");
    request.prompt_cache_profile = Some(provider::PromptCacheProfile::StablePrefix);
    request.system_prompt = Some("Reusable instructions".into());
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .get("prompt_cache_options")
            .is_none()
    );
    provider = provider.with_prompt_cache_options_models(vec!["opaque-model".into()]);
    let payload = provider.convert_to_openai_responses_format(&request).unwrap();
    assert_eq!(payload["prompt_cache_options"], json!({"ttl": "12h"}));
    assert!(!payload.to_string().contains("prompt_cache_breakpoint"));
    assert_eq!(provider.requested_prompt_cache_ttl(&request.model), Some(PromptCacheTtl::H12));
    request.model = "unconfigured-model".into();
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .get("prompt_cache_options")
            .is_none()
    );
}

#[tokio::test]
async fn prompt_cache_ttl_http_entrypoints_preserve_requested_policy() {
    use crate::providers::custom_provider::CustomProviderBackendRouter;
    use vtcode_config::core::{CustomProviderConfig, CustomProviderProfileConfig};
    for native in [false, true] {
        for entrypoint in ["generate", "legacy", "normalized"] {
            let server = MockServer::start().await;
            let response = json!({"id": "resp_ttl", "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}]});
            let template = if entrypoint == "generate" {
                ResponseTemplate::new(200).set_body_json(response)
            } else {
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(format!(
                        "data: {}\n\n",
                        json!({"type": "response.completed", "response": response})
                    ))
            };
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(template)
                .expect(1)
                .mount(&server)
                .await;
            let mut config = PromptCachingConfig::default();
            config.providers.openai.prompt_cache_ttl = Some(PromptCacheTtl::H12);
            let provider: Box<dyn LLMProvider> = if native {
                let mut provider = test_provider(&server.uri(), models::openai::GPT_6_LUNA);
                provider.backend_setup = OpenAIBackendSetup::api_key("https://api.openai.com/v1".into());
                provider.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
                Box::new(provider)
            } else {
                Box::new(CustomProviderBackendRouter::from_config(
                    CustomProviderConfig {
                        name: "ttl-fixture".into(),
                        model: "opaque-model".into(),
                        models: vec![
                            "opaque-model".into(),
                            "disabled-model".into(),
                            "chat-model".into(),
                            "anthropic-model".into(),
                        ],
                        api_format: CustomProviderApiFormat::OpenAIResponses,
                        supports_prompt_cache_options: Some(true),
                        profiles: std::collections::BTreeMap::from([
                            (
                                "disabled-model".into(),
                                CustomProviderProfileConfig {
                                    supports_prompt_cache_options: Some(false),
                                    ..Default::default()
                                },
                            ),
                            (
                                "chat-model".into(),
                                CustomProviderProfileConfig {
                                    api_format: CustomProviderApiFormat::OpenAIChat,
                                    ..Default::default()
                                },
                            ),
                            (
                                "anthropic-model".into(),
                                CustomProviderProfileConfig {
                                    api_format: CustomProviderApiFormat::AnthropicMessages,
                                    ..Default::default()
                                },
                            ),
                        ]),
                        ..Default::default()
                    },
                    Some("fixture-key".into()),
                    None,
                    server.uri(),
                    Some(config),
                    None,
                    None,
                    None,
                    None,
                    None,
                ))
            };
            let model = if native {
                models::openai::GPT_6_LUNA
            } else {
                "opaque-model"
            };
            if !native {
                for model in ["disabled-model", "chat-model", "anthropic-model"] {
                    assert_eq!(provider.requested_prompt_cache_ttl(model), None, "{model}");
                }
            }
            let request = sample_request(model);
            match entrypoint {
                "generate" => {
                    provider.generate(request).await.unwrap();
                }
                "legacy" => {
                    let mut stream = provider.stream(request).await.unwrap();
                    while let Some(event) = stream.next().await {
                        event.unwrap();
                    }
                }
                _ => {
                    let mut stream = provider.stream_normalized(request).await.unwrap();
                    while let Some(event) = stream.next().await {
                        event.unwrap();
                    }
                }
            }
            let recorded = server.received_requests().await.unwrap();
            let payload: Value = recorded[0].body_json().unwrap();
            assert_eq!(payload.pointer("/prompt_cache_options/ttl"), Some(&json!("12h")), "{native}/{entrypoint}");
        }
    }
}

#[tokio::test]
async fn prompt_cache_ttl_rejection_is_visible_without_downgrade() {
    for entrypoint in ["generate", "legacy", "normalized"] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": {
                "message": "Unsupported prompt_cache_options.ttl: 12h", "type": "invalid_request_error",
                "param": "prompt_cache_options.ttl", "code": "invalid_value"
            }})))
            .expect(1)
            .mount(&server)
            .await;
        let mut provider = test_provider(&server.uri(), "opaque-model")
            .with_api_format_override(Some(CustomProviderApiFormat::OpenAIResponses))
            .with_prompt_cache_options_models(vec!["opaque-model".into()]);
        provider.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
        let request = sample_request("opaque-model");
        let error = match entrypoint {
            "generate" => provider.generate(request).await.err(),
            "legacy" => provider.stream(request).await.err(),
            _ => provider.stream_normalized(request).await.err(),
        }
        .expect("provider rejection must remain visible");
        assert!(error.to_string().contains("prompt_cache_options.ttl"));
        let recorded = server.received_requests().await.unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].body_json::<Value>().unwrap().pointer("/prompt_cache_options/ttl"), Some(&json!("12h")));
    }
}

#[test]
fn prompt_cache_gap_preserves_subscription_baseline_without_applying_configured_ttl() {
    let model = models::openai::GPT_6_LUNA;
    let mut provider = chatgpt_backend_provider(model);
    let mut config = PromptCachingConfig::default();
    config.providers.openai.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    provider.prompt_cache_settings.prompt_cache_ttl = Some(PromptCacheTtl::H12);
    let requested_ttl = provider.requested_prompt_cache_ttl(model);
    assert_eq!(requested_ttl, None);
    assert_eq!(config.gap_threshold_secs_for_route("openai", model, requested_ttl), Some(1_800));
}
