use super::*;

#[test]
fn openai_and_chatgpt_preserve_prompt_cache_key() {
    let native_provider = native_openai_provider(models::openai::GPT_5_6);
    let chatgpt_provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.prompt_cache_key = Some("vtcode:session".to_owned());

    let native_payload = native_provider
        .convert_to_openai_responses_format(&request)
        .expect("native payload should build");
    let chatgpt_payload = chatgpt_provider
        .convert_to_openai_responses_format(&request)
        .expect("chatgpt payload should build");

    assert_eq!(native_payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:session"));
    assert_eq!(chatgpt_payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:session"));
}

#[test]
fn chat_payload_includes_prompt_cache_key_for_native_openai() {
    let provider = native_openai_provider(models::openai::DEFAULT_MODEL);
    let mut request = sample_request(models::openai::DEFAULT_MODEL);
    request.prompt_cache_key = Some("vtcode:openai:session-abc".to_string());
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:openai:session-abc"));
}

#[test]
fn responses_payload_includes_prompt_cache_retention_for_native_openai() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::InMemory);
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
        None,
    );
    // Responses API model (non-GPT-5.6: uses the legacy `prompt_cache_retention`
    // field; GPT-5.6+ instead emits `prompt_cache_options.ttl`).
    let payload = responses_payload_for(models::openai::GPT_5_1_MINI, &provider);
    assert_eq!(payload.get("prompt_cache_retention").and_then(Value::as_str), Some("in_memory"));
    // Chat Completions model - should NOT have it
    let chat_payload = chat_payload_for(models::openai::GPT_5, &provider);
    assert_absent(&chat_payload, "prompt_cache_retention");
}

#[test]
fn responses_payload_defaults_gpt56_cache_ttl_to_30m() {
    // No retention configured: GPT-5.6-family Responses requests must still
    // declare `prompt_cache_options.ttl` explicitly with the default "30m"
    // instead of relying on the implicit default alone.
    // Asymmetric pair: LUNA (gpt56) gets the default, CODEX (non-gpt56) does not.
    let provider = native_openai_provider(models::openai::GPT_5_6_LUNA);
    let payload = responses_payload_for(models::openai::GPT_5_6_LUNA, &provider);
    assert_eq!(
        payload
            .get("prompt_cache_options")
            .and_then(|v| v.get("ttl"))
            .and_then(Value::as_str),
        Some("30m")
    );
    assert_absent(&payload, "prompt_cache_retention");

    let codex_payload = responses_payload_for(models::openai::GPT_5_CODEX, &provider);
    assert_absent(&codex_payload, "prompt_cache_options");
}

#[test]
fn responses_payload_includes_prompt_cache_key_for_native_openai() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.prompt_cache_key = Some("vtcode:openai:session-123".to_string());
    let payload = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:openai:session-123"));
}

#[test]
fn responses_payload_omits_prompt_cache_key_for_non_native() {
    let provider = compatible_endpoint_provider(models::openai::GPT_5_6, "https://example.local/v1");
    let mut request = sample_request(models::openai::GPT_5_6);
    request.prompt_cache_key = Some("vtcode:openai:session-xyz".to_string());
    assert_absent(&provider.convert_to_openai_responses_format(&request).expect("should succeed"), "prompt_cache_key");
}

#[test]
fn prompt_cache_retention_excluded_when_not_set_and_for_unsupported_models() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = None;
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        None,
        Some(pc),
        None,
        None,
        None,
        None,
    );
    let mut request = sample_request(models::openai::GPT_5_6);
    request.stream = true;
    assert_absent(
        &provider.convert_to_openai_responses_format(&request).expect("should succeed"),
        "prompt_cache_retention",
    );

    // Unsupported model also omits it
    let mut pc2 = PromptCachingConfig::default();
    pc2.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::H24);
    let provider2 = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_OSS_20B.to_string()),
        None,
        Some(pc2),
        None,
        None,
        None,
        None,
    );
    assert_absent(&responses_payload_for(models::openai::GPT_OSS_20B, &provider2), "prompt_cache_retention");
}

#[test]
fn provider_from_config_respects_prompt_cache_and_websocket_gating() {
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some(PromptCacheRetention::InMemory);
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        None,
        Some(pc.clone()),
        None,
        None,
        None,
        None,
    );
    assert_eq!(provider.prompt_cache_settings.prompt_cache_retention, Some(PromptCacheRetention::InMemory));

    let native_ws = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert!(native_ws.websocket_mode_enabled(models::openai::GPT_5_6));

    let compatible_ws = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        Some("https://compat.example/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert!(compatible_ws.websocket_mode_enabled(models::openai::GPT_5_6));

    let custom_ws = OpenAIProvider::from_custom_config(
        "mycorp".to_string(),
        "MyCorp".to_string(),
        Some("key".to_string()),
        Some(models::openai::GPT_5_6.to_string()),
        Some("https://compat.example/v1".to_string()),
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
        None,
        None,
    );
    assert!(custom_ws.websocket_mode_enabled(models::openai::GPT_5_6));

    let chatgpt_ws = OpenAIProvider::from_config(
        Some(String::new()),
        Some(sample_chatgpt_auth_handle()),
        Some(models::openai::GPT_5_6.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert!(!chatgpt_ws.websocket_mode_enabled(models::openai::GPT_5_6));

    let xai_ws = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
        Some(models::openai::GPT_5_6.to_string()),
        Some("https://api.x.ai/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert!(!xai_ws.websocket_mode_enabled(models::openai::GPT_5_6));
}
