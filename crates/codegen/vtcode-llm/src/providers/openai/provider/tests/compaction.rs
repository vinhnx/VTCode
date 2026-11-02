use super::*;

#[test]
fn supports_responses_compaction_tracks_responses_api_availability() {
    let openai = native_openai_provider(models::openai::GPT_5);
    assert!(openai.supports_responses_compaction(models::openai::GPT_5));
    let compatible = compatible_endpoint_provider(models::openai::GPT_5, "https://compat.example/v1");
    assert!(compatible.supports_responses_compaction(models::openai::GPT_5));
    let xai = compatible_endpoint_provider(models::openai::GPT_5, "https://api.x.ai/v1");
    assert!(!xai.supports_responses_compaction(models::openai::GPT_5));
}

#[test]
fn supports_manual_openai_compaction_is_native_only() {
    let openai = native_openai_provider(models::openai::GPT_5);
    assert!(openai.supports_manual_openai_compaction(models::openai::GPT_5));
    assert!(
        !compatible_endpoint_provider(models::openai::GPT_5, "https://compat.example/v1")
            .supports_manual_openai_compaction(models::openai::GPT_5)
    );
    assert!(
        !OpenAIProvider::from_custom_config(
            "custom".to_string(),
            "Custom".to_string(),
            Some(String::new()),
            Some(models::openai::GPT_5.to_string()),
            Some("https://api.openai.com/v1".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .supports_manual_openai_compaction(models::openai::GPT_5)
    );
    assert!(!chatgpt_backend_provider(models::openai::GPT_5).supports_manual_openai_compaction(models::openai::GPT_5));
    assert!(!openai.supports_manual_openai_compaction("gpt-4.1"));
}

#[tokio::test]
async fn compact_history_rejects_responses_only_compatible_endpoint() {
    let provider = compatible_endpoint_provider(models::openai::GPT_5, "https://compat.example/v1");
    let error = provider
        .compact_history(models::openai::GPT_5, &[provider::Message::user("compact this".to_string())])
        .await
        .expect_err("standalone compaction must be gated separately from Responses support");

    assert!(error.to_string().contains("not supported"));
}

#[test]
fn manual_openai_compaction_unavailable_message_mentions_backend() {
    let chatgpt = chatgpt_backend_provider(models::openai::GPT_5);
    let msg = chatgpt.manual_openai_compaction_unavailable_message(models::openai::GPT_5);
    assert!(msg.contains("ChatGPT subscription auth via chatgpt.com backend"));
    let compatible = compatible_endpoint_provider(models::openai::GPT_5, "https://compat.example/v1");
    let msg2 = compatible.manual_openai_compaction_unavailable_message(models::openai::GPT_5);
    assert!(msg2.contains("configured OpenAI-compatible endpoint (https://compat.example/v1)"));
    let openai = native_openai_provider(models::openai::GPT_5);
    let msg3 = openai.manual_openai_compaction_unavailable_message("gpt-4.1");
    assert!(msg3.contains("native OpenAI API (api.openai.com)"));
    assert!(msg3.contains("openai / native OpenAI API (api.openai.com) / gpt-4.1"));
}

#[tokio::test]
async fn standalone_compaction_omits_generation_fields_and_preserves_replay() {
    let server = MockServer::start().await;
    let mut provider = test_provider(&native_openai_mock_base_url(&server), models::openai::GPT_5_6_SOL);
    provider.prompt_cache_enabled = true;
    provider.prompt_cache_settings.surface_metrics = true;
    let mut baseline = sample_request(models::openai::GPT_5_6_SOL);
    baseline.prompt_cache_key = Some("lineage-key".into());
    provider
        .cache_comparison_tracker(&baseline)
        .unwrap()
        .record_completion(&provider::LLMResponse {
            request_id: Some("resp_generation_baseline".into()),
            ..Default::default()
        });
    let compaction_item = json!({
        "id": "cmp_fixture",
        "type": "compaction",
        "encrypted_content": "opaque-compacted-state"
    });
    let retained_item = json!({
        "type": "message", "role": "user",
        "content": [{"type": "input_text", "text": "Summarize."}]
    });
    let returned_items = json!([retained_item, compaction_item]);
    let returned_items_for_mock = returned_items.clone();
    Mock::given(method("POST"))
        .and(path("/responses/compact"))
        .respond_with(move |req: &wiremock::Request| {
            let payload: Value = serde_json::from_slice(&req.body).expect("valid json");
            assert!(
                payload.pointer("/prompt_cache_options/comparison_response_id").is_none(),
                "standalone compaction must omit generation comparisons"
            );
            // Independently enforce the documented endpoint schema, including
            // absence of stream rather than accepting stream=false.
            let allowed_fields = [
                "model",
                "input",
                "instructions",
                "previous_response_id",
                "service_tier",
                "prompt_cache_key",
                "prompt_cache_options",
                "prompt_cache_retention",
            ];
            if let Some(key) = payload
                .as_object()
                .expect("payload object")
                .keys()
                .find(|key| !allowed_fields.contains(&key.as_str()))
            {
                return ResponseTemplate::new(400).set_body_json(json!({
                    "error": {"message": format!("Unknown parameter: '{key}'."),
                        "type": "invalid_request_error", "param": key, "code": "unknown_parameter"}
                }));
            }
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "resp_compact", "object": "response.compaction",
                "output": returned_items_for_mock,
            }))
        })
        .expect(2)
        .mount(&server)
        .await;

    let history = [
        provider::Message::system("Preserve decisions.".to_string()),
        provider::Message::user("Summarize.".to_string()),
    ];
    let options = provider::ResponsesCompactionOptions {
        instructions: Some("Terse.".to_string()),
        max_output_tokens: Some(321),
        // Generation-only overrides must not be validated or sent here.
        reasoning_effort: Some(vtcode_config::types::ReasoningEffortLevel::Minimal),
        verbosity: Some(vtcode_config::types::VerbosityLevel::High),
        responses_include: Some(vec!["reasoning.encrypted_content".to_string()]),
        response_store: Some(true),
        service_tier: Some("priority".to_string()),
        prompt_cache_key: Some("lineage-key".to_string()),
    };
    for manual in [false, true] {
        let compacted = if manual {
            provider
                .compact_history_with_options(models::openai::GPT_5_6_SOL, &history, &options)
                .await
        } else {
            provider.compact_history(models::openai::GPT_5_6_SOL, &history).await
        }
        .expect("documented standalone compaction payload should succeed");
        assert_eq!(compacted.len(), 2);
        let replay = provider
            .convert_to_openai_responses_format(&provider::LLMRequest {
                model: models::openai::GPT_5_6_SOL.to_string(),
                messages: compacted.into(),
                ..Default::default()
            })
            .expect("compacted history must be replayable");
        let replay_items = replay["input"].as_array().expect("replay input array");
        assert_eq!(replay_items.len(), 2);
        assert_eq!(replay_items[0]["role"], json!("user"));
        assert_eq!(replay_items[0]["content"][0]["text"], json!("Summarize."));
        assert_eq!(replay_items[1], returned_items[1], "opaque compaction state must survive unchanged");
        assert_eq!(history[1].content.as_text(), "Summarize.", "source history remains intact");
    }

    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 2);
    let default_payload: Value = serde_json::from_slice(&requests[0].body).expect("valid JSON");
    let p: Value = serde_json::from_slice(&requests[1].body).expect("valid JSON");
    assert_eq!(default_payload["model"], json!(models::openai::GPT_5_6_SOL));
    assert!(default_payload.get("service_tier").is_none());
    assert!(default_payload.get("prompt_cache_key").is_none());
    assert_eq!(p["model"], json!(models::openai::GPT_5_6_SOL));
    assert_eq!(p["service_tier"], json!("priority"));
    assert_eq!(p["prompt_cache_key"], json!("lineage-key"));
    assert_eq!(p["prompt_cache_options"]["ttl"], json!("30m"));
    for payload in [&default_payload, &p] {
        for key in ["stream", "max_output_tokens", "store", "include", "reasoning", "text"] {
            assert!(payload.get(key).is_none(), "standalone compaction must omit {key}");
        }
    }
    assert!(p.get("previous_response_id").is_none() && p.get("output_types").is_none());
    let instr = p["instructions"].as_str().expect("instructions required");
    assert!(
        instr.contains("[Manual Compaction Instructions]") && instr.contains("Terse."),
        "manual compaction instructions stay in the cached prefix"
    );
    assert!(
        !instr.contains("Preserve decisions."),
        "history system text must not rewrite the cached instructions prefix"
    );
    let input_items = p["input"].as_array().expect("input items required");
    let reminder = input_items.last().expect("trailing runtime reminder");
    assert!(
        reminder
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("Preserve decisions.")),
        "history system text rides the trailing reminder so it stays visible to the compactor"
    );
    assert_eq!(
        provider
            .convert_to_openai_responses_format(&baseline)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_generation_baseline")),
        "compaction must not advance the foreground baseline"
    );
}
