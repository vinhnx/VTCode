use super::*;

#[tokio::test]
async fn auxiliary_explicit_cache_is_opt_in_and_native_only() {
    for native in [false, true] {
        for opted_in in [false, true] {
            for auxiliary in [false, true] {
                let server = MockServer::start().await;
                Mock::given(method("POST"))
                    .and(path("/responses"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "id": "resp_auxiliary", "output": [{"type": "message",
                            "content": [{"type": "output_text", "text": "ok"}]}]
                    })))
                    .expect(1)
                    .mount(&server)
                    .await;
                let mut provider = test_provider(&server.uri(), models::openai::GPT_6_LUNA);
                if native {
                    provider.backend_setup = OpenAIBackendSetup::api_key("https://api.openai.com/v1".into());
                }
                provider.prompt_cache_settings.auxiliary_explicit_cache = opted_in;
                let mut request = sample_request(models::openai::GPT_6_LUNA);
                request.system_prompt = Some("Stable developer instructions".into());
                if auxiliary {
                    provider.generate_auxiliary(request).await.unwrap();
                } else {
                    provider.generate(request).await.unwrap();
                }
                let requests = server.received_requests().await.unwrap();
                let payload: Value = serde_json::from_slice(&requests[0].body).unwrap();
                assert_eq!(
                    payload.pointer("/prompt_cache_options/mode").and_then(Value::as_str),
                    (native && opted_in && auxiliary).then_some("explicit")
                );
            }
        }
    }
}

#[tokio::test]
async fn cache_usage_survives_disabled_controls_across_http_entrypoints() {
    for responses in [false, true] {
        for enabled in [false, true] {
            for surface_metrics in [false, true] {
                for entrypoint in ["generate", "legacy", "normalized"] {
                    let server = MockServer::start().await;
                    let model = if responses {
                        models::openai::GPT_6_LUNA
                    } else {
                        "gpt-4o"
                    };
                    let usage = if responses {
                        json!({"input_tokens": 200, "output_tokens": 19, "total_tokens": 219,
                            "input_tokens_details": {"cached_tokens": 73, "cache_write_tokens": 41}})
                    } else {
                        json!({"prompt_tokens": 200, "completion_tokens": 19, "total_tokens": 219,
                            "prompt_tokens_details": {"cached_tokens": 73, "cache_write_tokens": 41}})
                    };
                    let body = if responses {
                        json!({"id": "resp_cache_usage", "output": [{"type": "message",
                            "content": [{"type": "output_text", "text": "accounted"}]}], "usage": usage})
                    } else {
                        json!({"id": "chat_cache_usage", "choices": [{"index": 0,
                            "message": {"role": "assistant", "content": "accounted"},
                            "finish_reason": "stop"}], "usage": usage})
                    };
                    let template = if entrypoint == "generate" {
                        ResponseTemplate::new(200).set_body_json(body)
                    } else {
                        let events = if responses {
                            // Empty final output exercises the streamed-delta metadata overlay.
                            let mut completion = body;
                            completion["output"] = json!([]);
                            format!(
                                "data: {}\n\ndata: {}\n\n",
                                json!({"type": "response.output_text.delta", "delta": "accounted",
                                    "item_id": "msg_usage", "output_index": 0, "content_index": 0, "sequence_number": 1}),
                                json!({"type": "response.completed", "response": completion})
                            )
                        } else {
                            format!(
                                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                                json!({"choices": [{"index": 0, "delta": {"content": "accounted"}}]}),
                                json!({"choices": [], "usage": usage})
                            )
                        };
                        ResponseTemplate::new(200)
                            .insert_header("content-type", "text/event-stream")
                            .set_body_string(events)
                    };
                    Mock::given(method("POST"))
                        .and(path(if responses { "/responses" } else { "/chat/completions" }))
                        .respond_with(template)
                        .expect(1)
                        .mount(&server)
                        .await;
                    let mut provider = test_provider(&server.uri(), model);
                    provider.prompt_cache_enabled = enabled;
                    provider.prompt_cache_settings.surface_metrics = surface_metrics;
                    let request = sample_request(model);
                    let response = match entrypoint {
                        "generate" => provider.generate(request).await.unwrap(),
                        "legacy" => {
                            let mut stream = provider.stream(request).await.unwrap();
                            loop {
                                if let provider::LLMStreamEvent::Completed { response } =
                                    stream.next().await.expect("completion").unwrap()
                                {
                                    break *response;
                                }
                            }
                        }
                        _ => {
                            let mut stream = provider.stream_normalized(request).await.unwrap();
                            loop {
                                if let NormalizedStreamEvent::Done { response } =
                                    stream.next().await.expect("completion").unwrap()
                                {
                                    break *response;
                                }
                            }
                        }
                    };
                    assert_eq!(response.content.as_deref(), Some("accounted"));
                    let usage = response.usage.expect("billing usage");
                    assert_eq!((usage.prompt_tokens, usage.completion_tokens, usage.total_tokens), (200, 19, 219));
                    assert_eq!(
                        usage.cached_prompt_tokens,
                        Some(73),
                        "{responses}/{entrypoint}/{enabled}/{surface_metrics}"
                    );
                    assert_eq!(usage.cache_creation_tokens, Some(41));
                    let normalized = crate::usage_cost::normalized_turn_usage("openai", &usage);
                    let cost = crate::usage_cost::estimate_session_costs_with_pricing(
                        vtcode_config::models::ModelPricing {
                            input: Some(0.01),
                            output: Some(0.02),
                            cache_read: Some(0.001),
                            cache_write: Some(0.0125),
                        },
                        &normalized,
                    )
                    .unwrap();
                    // 86 ordinary inputs, 73 reads, 41 writes, and 19 output tokens.
                    assert!((cost.effective_usd - 1.8255).abs() < 1e-12);
                }
            }
        }
    }
}

#[tokio::test]
async fn cache_usage_distinguishes_missing_counters_from_reported_zero() {
    let provider = test_provider("http://127.0.0.1:1", models::openai::GPT_6_LUNA);
    for details in [json!({}), json!({"cached_tokens": 0, "cache_write_tokens": 0})] {
        let expected = details.get("cached_tokens").map(|_| 0);
        let responses = provider
            .parse_openai_responses_response(
                json!({
                    "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}],
                    "usage": {"input_tokens": 200, "input_tokens_details": details}
                }),
                models::openai::GPT_6_LUNA.into(),
            )
            .unwrap();
        let chat = provider
            .parse_openai_response(
                json!({
                    "choices": [{"message": {"content": "ok"}}],
                    "usage": {"prompt_tokens": 200, "prompt_tokens_details": details}
                }),
                "gpt-4o".into(),
            )
            .unwrap();
        for response in [responses, chat] {
            let usage = response.usage.unwrap();
            assert_eq!(usage.cached_prompt_tokens, expected);
            assert_eq!(usage.cache_creation_tokens, expected);
        }
    }
}
