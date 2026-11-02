use super::*;

#[test]
fn effective_context_size_uses_catalog_capacity_and_explicit_override() {
    let provider = OpenAIProvider::new("offline-fixture".to_string());
    assert_eq!(provider.effective_context_size("gpt-6-astra"), 1_050_000);
    assert_eq!(provider.effective_context_size("unknown-model"), 128_000);
    assert_eq!(provider.with_context_window(Some(32_000)).effective_context_size("gpt-6-astra"), 32_000);
}

#[test]
fn openai_responses_native_emits_store_false_without_previous_response_id() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.previous_response_id = Some("resp_previous_123".to_string());

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("native payload should build");

    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_absent(&payload, "previous_response_id");
}

#[test]
fn chatgpt_responses_emits_store_false_without_previous_response_id() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.previous_response_id = Some("resp_previous_123".to_string());

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("chatgpt payload should build");

    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_absent(&payload, "previous_response_id");
}

#[test]
fn openai_and_chatgpt_share_responses_payload_builder_except_backend() {
    let native_provider = native_openai_provider(models::openai::GPT_5_6);
    let chatgpt_provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    let mut request = provider::LLMRequest {
        messages: vec![
            provider::Message::system("Follow the local policy.".to_owned()),
            provider::Message::user("Summarise the repository state.".to_owned()),
            provider::Message::assistant("There is one modified file.".to_owned()),
            provider::Message::user("Continue.".to_owned()),
        ]
        .into(),
        model: models::openai::GPT_5_6.to_string(),
        stream: true,
        ..Default::default()
    };
    request.previous_response_id = Some("resp_previous_123".to_owned());
    request.response_store = Some(true);
    request.prompt_cache_key = Some("vtcode:session".to_owned());

    let native_payload = native_provider
        .convert_to_openai_responses_format(&request)
        .expect("native payload should build");
    let chatgpt_payload = chatgpt_provider
        .convert_to_openai_responses_format(&request)
        .expect("chatgpt payload should build");

    for (payload, expected_role) in [(&native_payload, "developer"), (&chatgpt_payload, "user")] {
        let reminder = payload["input"].as_array().unwrap().last().unwrap();
        assert_eq!(reminder["role"], expected_role);
        assert!(
            reminder["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Follow the local policy.")
        );
    }
    assert_eq!(
        without_native_cache_projection(without_responses_backend_capability_fields(native_payload.clone())),
        without_native_cache_projection(without_responses_backend_capability_fields(chatgpt_payload.clone()))
    );
    assert!(
        count_input_breakpoints(&native_payload) > 0,
        "native API-key requests keep explicit cache breakpoints"
    );
    assert_eq!(
        count_input_breakpoints(&chatgpt_payload),
        0,
        "ChatGPT subscription backend rejects prompt_cache_breakpoint (400), so it must be omitted"
    );
}

#[test]
fn chat_completions_applies_gpt_5_6_addendum_only_for_gpt_5_6() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages should exist");
    let system_content = messages
        .first()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .expect("system content should be a string");
    assert!(system_content.contains("You are a helpful assistant."));
    assert!(system_content.contains("## GPT-5.6 OpenAI Addendum"));

    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages should exist");
    let system_content = messages
        .first()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .expect("system content should be a string");
    assert!(!system_content.contains("## GPT-5.6 OpenAI Addendum"));
}

#[test]
fn chat_completions_uses_max_completion_tokens_field() {
    let provider = compatible_endpoint_provider(models::openai::DEFAULT_MODEL, "https://api.openai.com/v1");
    let mut request = sample_request(models::openai::DEFAULT_MODEL);
    request.max_tokens = Some(512);
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    assert_eq!(payload.get("max_completion_tokens").and_then(Value::as_u64), Some(512));
    assert!(payload.get("max_tokens").is_none());
}

#[test]
fn custom_openai_provider_uses_compatible_max_tokens_field_even_on_openai_url() {
    let provider = OpenAIProvider::from_custom_config(
        "openai-compatible".to_string(),
        "OpenAI-compatible".to_string(),
        Some(String::new()),
        Some(models::openai::DEFAULT_MODEL.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let mut request = sample_request(models::openai::DEFAULT_MODEL);
    request.max_tokens = Some(512);

    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");

    assert_eq!(payload.get("max_tokens").and_then(Value::as_u64), Some(512));
    assert!(payload.get("max_completion_tokens").is_none());
}

#[test]
fn chat_completions_applies_temperature_independent_of_max_tokens() {
    let provider = native_openai_provider(models::openai::GPT_5_6);
    let mut request = sample_request(models::openai::GPT_5_6);
    request.temperature = Some(0.4);
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    assert!(payload.get("max_completion_tokens").is_none());
    let temp = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .expect("temperature should be present");
    assert!((temp - 0.4).abs() < 1e-6);
}

#[test]
fn chat_completions_omits_temperature_for_gpt_5_6_with_reasoning() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::Medium);
    request.temperature = Some(0.4);
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    assert!(payload.get("temperature").is_none());
}

#[test]
fn chat_completions_keeps_temperature_for_gpt_5_6_without_reasoning() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::None);
    request.temperature = Some(0.4);
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    let temp = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .expect("temperature should be present");
    assert!((temp - 0.4).abs() < 1e-6);
}

#[test]
fn chat_payload_uses_provider_level_service_tier_for_native_openai() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::DEFAULT_MODEL.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let payload = chat_payload_for(models::openai::DEFAULT_MODEL, &provider);
    assert_eq!(payload.get("service_tier").and_then(Value::as_str), Some("priority"));
}

#[test]
fn chat_payload_uses_flex_service_tier_for_native_openai() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::DEFAULT_MODEL.to_string()),
        None,
        None,
        None,
        None,
        Some(flex_openai_config()),
        None,
    );
    let payload = chat_payload_for(models::openai::DEFAULT_MODEL, &provider);
    assert_eq!(payload.get("service_tier").and_then(Value::as_str), Some("flex"));
}

#[test]
fn chat_payload_omits_service_tier_for_models_without_service_tier_support() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::GPT_OSS_20B.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let payload = chat_payload_for(models::openai::GPT_OSS_20B, &provider);
    assert!(payload.get("service_tier").is_none());
}

#[test]
fn chat_payload_omits_service_tier_for_non_native_openai_base_url() {
    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::DEFAULT_MODEL.to_string()),
        Some("https://example.local/v1".to_string()),
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let payload = chat_payload_for(models::openai::DEFAULT_MODEL, &provider);
    assert!(payload.get("service_tier").is_none());
}

#[test]
fn responses_payload_omits_default_verbosity_for_gpt_5_codex() {
    let provider = native_openai_provider(models::openai::GPT_5_CODEX);
    let payload = responses_payload_for(models::openai::GPT_5_CODEX, &provider);
    assert_absent(&payload, "text");
}

#[test]
fn responses_payload_keeps_configured_verbosity_medium_for_gpt_5_codex() {
    let provider = native_openai_provider(models::openai::GPT_5_CODEX);
    let mut request = sample_request(models::openai::GPT_5_CODEX);
    request.verbosity = Some(vtcode_config::types::VerbosityLevel::Medium);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_eq!(payload.get("text").and_then(|t| t.get("verbosity")).and_then(Value::as_str), Some("medium"));
}

#[test]
fn responses_payload_keeps_configured_verbosity_for_gpt_5_codex() {
    let provider = native_openai_provider(models::openai::GPT_5_CODEX);
    let mut request = sample_request(models::openai::GPT_5_CODEX);
    request.verbosity = Some(vtcode_config::types::VerbosityLevel::High);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_eq!(payload.get("text").and_then(|t| t.get("verbosity")).and_then(Value::as_str), Some("high"));
}

#[test]
fn responses_payload_keeps_configured_verbosity_for_gpt_5_6_sol() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.verbosity = Some(vtcode_config::types::VerbosityLevel::High);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_eq!(payload.get("text").and_then(|t| t.get("verbosity")).and_then(Value::as_str), Some("high"));
}

#[test]
fn responses_payload_passes_context_management() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.context_management = Some(json!([{"type": "compaction", "compact_threshold": 200000}]));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let mgmt = payload
        .get("context_management")
        .and_then(Value::as_array)
        .expect("context_management should be present");
    assert_eq!(mgmt.len(), 1);
    assert_eq!(mgmt[0].get("type").and_then(Value::as_str), Some("compaction"));
}

#[test]
fn responses_payload_sets_instructions_from_system_prompt() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_str_field(&payload, "instructions", "You are a helpful assistant.");
    let input = get_input_array(&payload);
    assert_eq!(input.first().and_then(|v| v.get("role")).and_then(Value::as_str), Some("user"));
}

#[test]
fn responses_payload_applies_gpt_5_6_addendum_only_for_gpt_5_6() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("instructions should exist");
    assert!(instructions.contains("You are a helpful assistant."));
    assert!(instructions.contains("## GPT-5.6 OpenAI Addendum"));

    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let instructions = payload
        .get("instructions")
        .and_then(Value::as_str)
        .expect("instructions should exist");
    assert!(!instructions.contains("## GPT-5.6 OpenAI Addendum"));
}

#[test]
fn responses_payload_applies_gpt_5_6_addendum_with_priority_tier() {
    assert!(OpenAIProvider::is_responses_api_model(models::openai::GPT_5_6_SOL));

    let provider = OpenAIProvider::from_config(
        Some("key".to_owned()),
        None,
        Some(models::openai::GPT_5_6_SOL.to_string()),
        None,
        None,
        None,
        None,
        Some(priority_openai_config()),
        None,
    );
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.system_prompt = Some(Arc::from("You are a helpful assistant."));
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");

    assert_eq!(payload.get("service_tier").and_then(Value::as_str), Some("priority"));
    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("instructions should exist");
    assert!(instructions.contains("## GPT-5.6 OpenAI Addendum"));
}

#[test]
fn responses_payload_omits_previous_response_and_includes_optional_fields() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.previous_response_id = Some("resp_previous_123".to_string());
    request.response_store = Some(false);
    request.responses_include = Some(vec![
        "reasoning.encrypted_content".to_string(),
        "output_text.annotations".to_string(),
    ]);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_absent(&payload, "previous_response_id");
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    let include = payload
        .get("include")
        .and_then(Value::as_array)
        .expect("include should be present");
    assert_eq!(include.len(), 2);
    assert_eq!(include.first().and_then(Value::as_str), Some("reasoning.encrypted_content"));
}

#[test]
fn compatible_responses_payload_omits_previous_response_id() {
    let provider = compatible_endpoint_provider(models::openai::GPT_5, "https://compat.example/v1");
    let mut request = sample_request(models::openai::GPT_5);
    request.previous_response_id = Some("resp_previous_123".to_string());
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_absent(&payload, "previous_response_id");
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
}

#[test]
fn chatgpt_backend_omits_previous_response_id_from_responses_payload() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5_CODEX);
    let mut request = sample_request(models::openai::GPT_5_CODEX);
    request.previous_response_id = Some("resp_previous_123".to_string());
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    assert_absent(&payload, "previous_response_id");
}

#[test]
fn chatgpt_backend_forces_store_false_and_omits_output_sampling_cache() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(sample_chatgpt_auth_handle()),
        Some(models::openai::GPT_5_6.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig { responses_store: Some(true), ..Default::default() }),
        None,
    );
    let payload = responses_payload_for(models::openai::GPT_5_6, &provider);
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert_absent(&payload, "output_types");
    assert_absent(&payload, "sampling_parameters");
    assert_absent(&payload, "prompt_cache_retention");

    // With temperature/top_p set, still omitted
    let mut request = sample_request(models::openai::GPT_5_6);
    request.temperature = Some(0.4);
    request.top_p = Some(0.8);
    let payload2 = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    assert_absent(&payload2, "sampling_parameters");
}

#[test]
fn chatgpt_backend_includes_encrypted_reasoning_and_preserves_configured_includes() {
    let provider = chatgpt_backend_provider(models::openai::GPT_5_6_SOL);
    let payload = responses_payload_for(models::openai::GPT_5_6_SOL, &provider);
    assert_eq!(payload.get("include").and_then(Value::as_array), Some(&vec![json!("reasoning.encrypted_content")]));

    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        Some(sample_chatgpt_auth_handle()),
        Some(models::openai::GPT_5_6_SOL.to_string()),
        None,
        None,
        None,
        None,
        Some(OpenAIConfig {
            responses_include: vec![
                "output_text.annotations".to_string(),
                " reasoning.encrypted_content ".to_string(),
            ],
            ..Default::default()
        }),
        None,
    );
    let payload = responses_payload_for(models::openai::GPT_5_6_SOL, &provider);
    assert_eq!(
        payload.get("include").and_then(Value::as_array),
        Some(&vec![json!("output_text.annotations"), json!("reasoning.encrypted_content"),])
    );
}

#[test]
fn supported_models_include_current_reasoning_models() {
    let supported = OpenAIProvider::new("key".to_owned()).supported_models();
    // Current reasoning models must be in the supported list.
    assert!(supported.contains(&"gpt-5.6-sol".to_string()));
    assert!(supported.contains(&models::openai::DEFAULT_MODEL.to_string()));
    // Deprecated o-series models are removed from the picker and from
    // REASONING_MODELS (the pruned catalog keeps only current generations).
    // They remain as remap constants for DEPRECATED_MODEL_REMAPPINGS.
    assert!(!supported.contains(&models::openai::O3.to_string()));
    assert!(!supported.contains(&models::openai::O4_MINI.to_string()));
    assert!(!models::openai::REASONING_MODELS.contains(&models::openai::O3));
    assert!(!models::openai::REASONING_MODELS.contains(&models::openai::O4_MINI));
}

#[test]
fn harmony_detection_handles_common_variants() {
    assert!(OpenAIProvider::uses_harmony("gpt-oss-20b"));
    assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b:free"));
    assert!(OpenAIProvider::uses_harmony("OPENAI/GPT-OSS-120B"));
    assert!(!OpenAIProvider::uses_harmony("gpt-5"));
    assert!(!OpenAIProvider::uses_harmony("gpt-oss:20b"));
}

#[test]
fn responses_payload_uses_max_output_tokens_field() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let mut request = sample_request(models::openai::GPT_5);
    request.max_tokens = Some(512);
    let payload = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    assert_eq!(payload.get("max_output_tokens").and_then(Value::as_u64), Some(512));
    assert_absent(&payload, "max_completion_tokens");
}

#[test]
fn chatgpt_backend_omits_max_output_tokens_and_blocks_unsupported_minimal_reasoning() {
    // `gpt-5.6-sol` is the current Codex-family model (the deprecation
    // replacement for `gpt-5-codex`); it advertises low/medium/high/xhigh/max,
    // so `minimal` must fail closed rather than degrade silently.
    let provider = chatgpt_backend_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.max_tokens = Some(512);
    assert_absent(&provider.convert_to_openai_responses_format(&request).expect("should succeed"), "max_output_tokens");

    request.max_tokens = None;
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::Minimal);
    let error = provider
        .convert_to_openai_responses_format(&request)
        .expect_err("unsupported effort must be blocked without explicit degradation");
    assert!(error.to_string().contains("unsupported by this route"));
}

#[test]
fn responses_payload_defaults_gpt_5_6_sol_reasoning_to_none() {
    let payload =
        responses_payload_for(models::openai::GPT_5_6_SOL, &native_openai_provider(models::openai::GPT_5_6_SOL));
    assert_eq!(payload.get("reasoning").and_then(|r| r.get("effort")).and_then(Value::as_str), Some("none"));
}

#[test]
fn responses_payload_defaults_gpt_5_6_terra_reasoning_to_high() {
    let payload =
        responses_payload_for(models::openai::GPT_5_6_TERRA, &native_openai_provider(models::openai::GPT_5_6_TERRA));
    assert_eq!(payload.get("reasoning").and_then(|r| r.get("effort")).and_then(Value::as_str), Some("high"));
}

#[test]
fn responses_payload_omits_sampling_parameters_for_gpt_5_6_sol_high_reasoning() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::High);
    request.temperature = Some(0.4);
    request.top_p = Some(0.9);
    assert_absent(
        &provider.convert_to_openai_responses_format(&request).expect("should succeed"),
        "sampling_parameters",
    );
}

#[test]
fn responses_payload_omits_penalties_when_sampling_gate_closed() {
    let provider = native_openai_provider(models::openai::GPT_5_6_SOL);
    let mut request = sample_request(models::openai::GPT_5_6_SOL);
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::High);
    request.presence_penalty = Some(0.1);
    request.frequency_penalty = Some(-0.5);
    let payload = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    assert_absent(&payload, "sampling_parameters");
}

#[test]
fn parse_harmony_tool_names_and_calls() {
    assert_eq!(
        OpenAIProvider::parse_harmony_tool_name("functions.code_search"),
        vtcode_config::constants::tools::CODE_SEARCH
    );
    assert_eq!(OpenAIProvider::parse_harmony_tool_name("grep"), "grep");
    assert_eq!(OpenAIProvider::parse_harmony_tool_name("container.exec"), "exec_command");
    assert_eq!(OpenAIProvider::parse_harmony_tool_name("unknown.tool"), "tool");
    assert!(!OpenAIProvider::uses_harmony("gpt-oss:20b"));

    let (name, args) = OpenAIProvider::parse_harmony_tool_call_from_text(
        r#"to=functions.code_search {"query":"Widget", "path":"src"}"#,
    )
    .expect("should parse");
    assert_eq!(name, vtcode_config::constants::tools::CODE_SEARCH);
    assert_eq!(args["query"], json!("Widget"));
    assert_eq!(args["path"], json!("src"));

    let (name2, args2) =
        OpenAIProvider::parse_harmony_tool_call_from_text(r#"to=container.exec {"cmd":["ls", "-la"]}"#)
            .expect("should parse");
    assert_eq!(name2, "exec_command");
    assert_eq!(args2["cmd"], json!(["ls", "-la"]));

    let text = r#"<|start|>assistant to=functions.lookup_weather<|channel|>commentary <|constrain|>json<|message|>{"location":"San Francisco"}<|call|>"#;
    let (name3, args3) = OpenAIProvider::parse_harmony_tool_call_from_text(text).expect("should parse");
    assert_eq!(name3, "lookup_weather");
    assert_eq!(args3["location"], json!("San Francisco"));

    let text2 = r#"<|start|>assistant to=functions.lookup_weather<|channel|>commentary <|constrain|>json<|message|>{'location':'San Francisco'}<|call|>"#;
    let (name4, _) = OpenAIProvider::parse_harmony_tool_call_from_text(text2).expect("should parse");
    assert_eq!(name4, "lookup_weather");
}
