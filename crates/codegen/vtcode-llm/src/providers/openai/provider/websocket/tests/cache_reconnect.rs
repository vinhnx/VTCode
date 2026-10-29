use super::*;

#[test]
fn websocket_cache_does_not_affect_normal_http_sse_payload() {
    let provider = websocket_test_provider("https://api.openai.com/v1".to_string());
    let mut request = websocket_test_request();
    request.previous_response_id = Some("resp_http_caller".to_string());
    request.response_store = Some(true);
    seed_continuation_cache(&provider, &request, "resp_cached", false);

    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("normal responses payload should build");

    assert!(payload.get("previous_response_id").is_none());
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
}
#[tokio::test]
async fn websocket_completion_tier_is_logged_once_without_replay_or_unrecognized_tier_records() {
    use crate::provider::NormalizedStreamEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing::instrument::WithSubscriber;
    use tracing::{Event, Metadata, Subscriber, span};

    struct CompletionRecorder {
        tiers: Arc<AtomicUsize>,
        cache: Arc<AtomicUsize>,
    }

    impl Subscriber for CompletionRecorder {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }
        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn event(&self, event: &Event<'_>) {
            if event.metadata().fields().field("service_tier").is_some() {
                self.tiers.fetch_add(1, Ordering::Relaxed);
            }
            if event.metadata().fields().field("cache_comparison_type").is_some() {
                self.cache.fetch_add(1, Ordering::Relaxed);
            }
        }
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }

    let records = Arc::new(AtomicUsize::new(0));
    let cache_records = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(
        CompletionRecorder {
            tiers: Arc::clone(&records),
            cache: Arc::clone(&cache_records),
        },
        || {
            for (tier, expected_records) in [("ultrafast", 1), ("untrusted\nfixture-tier", 1), ("priority", 2)] {
                let completion = json!({
                    "id": "resp_tier_evidence",
                    "service_tier": tier,
                    "prompt_cache_diagnostics": {"type": "cache_hit"},
                    "output": [{"type": "message", "content": [{"type": "output_text", "text": "primary answer"}]}]
                });
                let response = super::super::super::super::responses_api::parse_responses_completion(
                    completion.clone(),
                    "gpt-5.6".into(),
                    false,
                )
                .expect("server completion");
                assert_eq!(response.content.as_deref(), Some("primary answer"));
                let replay = super::super::completed_replay_output(&completion, "gpt-5.6").expect("canonical replay");
                assert_eq!(replay.len(), 1);
                assert_eq!(
                    records.load(Ordering::Relaxed),
                    expected_records,
                    "replay must not log another completion; unknown tiers must not be logged"
                );
            }
        },
    );

    assert_eq!(cache_records.load(Ordering::Relaxed), 3, "replay does not duplicate cache diagnostics");
    enum EntryPoint {
        Generate,
        Normalized,
        Legacy,
    }
    for (index, entry_point) in [EntryPoint::Generate, EntryPoint::Normalized, EntryPoint::Legacy]
        .into_iter()
        .enumerate()
    {
        let (base_url, requests, server) =
            spawn_scripted_websocket_server(vec![vec![ScriptedReply::Events(vec![json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_transport_tier",
                    "service_tier": "ultrafast",
                    "prompt_cache_diagnostics": {"type": "cache_miss", "reason": "tools_changed", "cache_missed_tokens": 5710},
                    "output": [{"type": "message", "content": [{"type": "output_text", "text": "transport answer"}]}]
                }
            })])]])
            .await
            .expect("WebSocket fixture");
        let mut provider = websocket_test_provider(base_url);
        // Keep transport local while exercising first-party wire eligibility.
        provider.backend_setup =
            crate::providers::openai::backend_setup::OpenAIBackendSetup::api_key("https://api.openai.com/v1".into());
        provider.prompt_cache_enabled = true;
        provider.prompt_cache_settings.surface_metrics = true;
        let mut request = websocket_test_request();
        request.system_prompt = Some(Arc::from("Stable developer instructions"));
        provider
            .cache_comparison_tracker(&request)
            .unwrap()
            .record_completion(&crate::provider::LLMResponse {
                request_id: Some("resp_baseline".into()),
                ..Default::default()
            });
        seed_connected_continuation_cache(&provider, &request, "resp_seed", false).await;
        let baseline_request = request.clone();
        let completion = async {
            match entry_point {
                EntryPoint::Generate => provider.generate(request).await.expect("generation"),
                EntryPoint::Normalized => {
                    let mut stream = provider.stream_normalized(request).await.expect("normalized stream");
                    loop {
                        if let NormalizedStreamEvent::Done { response } =
                            stream.next().await.expect("event").expect("valid event")
                        {
                            break *response;
                        }
                    }
                }
                EntryPoint::Legacy => {
                    let mut stream = provider.stream(request).await.expect("legacy stream");
                    loop {
                        if let crate::provider::LLMStreamEvent::Completed { response } =
                            stream.next().await.expect("event").expect("valid event")
                        {
                            break *response;
                        }
                    }
                }
            }
        };
        let response = tokio::time::timeout(std::time::Duration::from_secs(3), completion)
            .with_subscriber(CompletionRecorder {
                tiers: Arc::clone(&records),
                cache: Arc::clone(&cache_records),
            })
            .await
            .expect("completion deadline");
        assert_eq!(response.content.as_deref(), Some("transport answer"));
        assert_eq!(requests.lock().expect("requests").len(), 1);
        assert_eq!(records.load(Ordering::Relaxed), index + 3, "each entry point must record one served tier");
        assert_eq!(cache_records.load(Ordering::Relaxed), index + 4);
        assert_eq!(
            requests.lock().unwrap()[0].pointer("/prompt_cache_options/comparison_response_id"),
            Some(&json!("resp_baseline"))
        );
        assert_eq!(
            provider
                .convert_to_openai_responses_format(&baseline_request)
                .unwrap()
                .pointer("/prompt_cache_options/comparison_response_id"),
            Some(&json!("resp_transport_tier"))
        );
        server.await.expect("server task");
    }
    // Both stream adapters accept deltas when authoritative output is empty.
    // Metadata must still be recorded once and the completed ID may advance
    // diagnostics without becoming a replay-safe WebSocket continuation.
    for websocket in [false, true] {
        for legacy in [false, true] {
            let before = cache_records.load(Ordering::Relaxed);
            let mut completed = completion("resp_empty_output", json!([]));
            completed["response"]["prompt_cache_diagnostics"] = json!({
                "type": "cache_miss", "reason": "context_compacted", "cache_missed_tokens": 6003
            });
            let events = vec![text_delta("delta-only reply"), completed];
            let (mut provider, ws_server) = if websocket {
                let (base, _, server) =
                    spawn_scripted_websocket_server(vec![vec![ScriptedReply::Events(events.clone())]])
                        .await
                        .expect("WebSocket fixture");
                let mut provider = websocket_test_provider(base);
                provider.backend_setup = crate::providers::openai::backend_setup::OpenAIBackendSetup::api_key(
                    "https://api.openai.com/v1".into(),
                );
                seed_connected_continuation_cache(&provider, &websocket_test_request(), "resp_seed", false).await;
                (provider, Some(server))
            } else {
                (websocket_test_provider("https://api.openai.com/v1".into()), None)
            };
            let http = wiremock::MockServer::start().await;
            if !websocket {
                provider.websocket_mode = false;
                provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
                let body = events.iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
                wiremock::Mock::given(wiremock::matchers::method("POST"))
                    .and(wiremock::matchers::path("/responses"))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
                    .mount(&http)
                    .await;
            }
            provider.prompt_cache_enabled = true;
            provider.prompt_cache_settings.surface_metrics = true;
            let response = async {
                if legacy {
                    let mut stream = provider.stream(websocket_test_request()).await.unwrap();
                    loop {
                        if let crate::provider::LLMStreamEvent::Completed { response } =
                            stream.next().await.unwrap().unwrap()
                        {
                            break *response;
                        }
                    }
                } else {
                    final_normalized(provider.stream_normalized(websocket_test_request()).await.unwrap()).await
                }
            }
            .with_subscriber(CompletionRecorder {
                tiers: Arc::clone(&records),
                cache: Arc::clone(&cache_records),
            })
            .await;
            assert_eq!(response.content.as_deref(), Some("delta-only reply"));
            assert_eq!(response.request_id.as_deref(), Some("resp_empty_output"));
            assert_eq!(
                cache_records.load(Ordering::Relaxed),
                before + 1,
                "empty final output retains cache evidence exactly once"
            );
            assert_eq!(
                provider
                    .convert_to_openai_responses_format(&websocket_test_request())
                    .unwrap()
                    .pointer("/prompt_cache_options/comparison_response_id"),
                Some(&json!("resp_empty_output"))
            );
            if let Some(server) = ws_server {
                assert!(provider.websocket_continuation_snapshot().is_none(), "empty output is not replay-safe");
                server.await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn websocket_cache_usage_survives_disabled_controls() {
    for enabled in [false, true] {
        for surface_metrics in [false, true] {
            for entrypoint in ["generate", "legacy", "normalized"] {
                let output = if entrypoint == "generate" {
                    json!([{"type": "message", "content": [{"type": "output_text", "text": "accounted"}]}])
                } else {
                    json!([])
                };
                let (base_url, recorded, server) =
                    spawn_scripted_websocket_server(vec![vec![ScriptedReply::Events(vec![
                        json!({
                        "type": "response.output_text.delta", "delta": "accounted",
                        "item_id": "msg_usage", "output_index": 0, "content_index": 0, "sequence_number": 1
                        }),
                        json!({
                            "type": "response.completed", "response": {
                            "id": "resp_cache_usage", "output": output,
                                "usage": {"input_tokens": 200, "output_tokens": 19, "total_tokens": 219,
                                    "input_tokens_details": {"cached_tokens": 73, "cache_write_tokens": 41}}
                            }
                        }),
                    ])]])
                    .await
                    .expect("local WebSocket fixture");
                let mut provider = websocket_test_provider(base_url);
                provider.prompt_cache_enabled = enabled;
                provider.prompt_cache_settings.surface_metrics = surface_metrics;
                let request = websocket_test_request();
                seed_connected_continuation_cache(&provider, &request, "resp_seed", false).await;
                let completion = async {
                    match entrypoint {
                        "generate" => provider.generate(request).await.unwrap(),
                        "legacy" => {
                            let mut stream = provider.stream(request).await.unwrap();
                            loop {
                                if let crate::provider::LLMStreamEvent::Completed { response } =
                                    stream.next().await.expect("completion").unwrap()
                                {
                                    break *response;
                                }
                            }
                        }
                        _ => {
                            let mut stream = provider.stream_normalized(request).await.unwrap();
                            loop {
                                if let crate::provider::NormalizedStreamEvent::Done { response } =
                                    stream.next().await.expect("completion").unwrap()
                                {
                                    break *response;
                                }
                            }
                        }
                    }
                };
                let response = tokio::time::timeout(std::time::Duration::from_secs(3), completion)
                    .await
                    .expect("bounded local completion");
                assert_eq!(response.content.as_deref(), Some("accounted"));
                let usage = response.usage.expect("billing usage");
                assert_eq!(usage.prompt_tokens, 200);
                assert_eq!(usage.cached_prompt_tokens, Some(73), "{entrypoint}/{enabled}/{surface_metrics}");
                assert_eq!(usage.cache_creation_tokens, Some(41));
                server.await.expect("fixture completed");
                assert_eq!(recorded.lock().unwrap().len(), 1, "one WebSocket generation, no HTTP fallback");
            }
        }
    }
}
#[tokio::test]
async fn websocket_auxiliary_http_success_and_timeout_preserve_primary_continuation() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    for auxiliary_model in ["gpt-6-luna", "gpt-5.6-luna"] {
        for cancelled in [false, true] {
            let (base_url, recorded, handle) = spawn_scripted_websocket_server(vec![vec![ScriptedReply::Completed {
                response_id: "resp_followup",
                text: "primary continued",
            }]])
            .await
            .expect("WebSocket fixture");
            let server = MockServer::start().await;
            let completed = json!({
                "id": "resp_auxiliary", "status": "completed",
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "SAFE"}]}]
            });
            let response = if auxiliary_model == "gpt-5.6-luna" {
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(format!(
                        "data: {}\n\n",
                        json!({"type":"response.completed", "response":completed})
                    ))
            } else {
                ResponseTemplate::new(200).set_body_json(completed)
            };
            let response = if cancelled {
                response.set_delay(std::time::Duration::from_secs(1))
            } else {
                response
            };
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
            let mut provider = websocket_test_provider(base_url);
            provider.responses_url = format!("{}/responses", server.uri()).into();
            provider.service_tier = Some(vtcode_config::OpenAIServiceTier::Ultrafast);
            let primary = LLMRequest {
                model: "gpt-6.1-sol".into(),
                ..websocket_test_request()
            };
            seed_connected_continuation_cache(&provider, &primary, "resp_primary", false).await;
            let cached = provider.websocket_continuation_snapshot().expect("primary continuation");
            let auxiliary = LLMRequest {
                model: auxiliary_model.into(),
                messages: vec![ProviderMessage::user("independent probe".into())].into(),
                max_tokens: Some(256),
                ..Default::default()
            };
            let outcome = tokio::time::timeout(
                std::time::Duration::from_millis(if cancelled { 100 } else { 3000 }),
                provider.generate_auxiliary(auxiliary),
            )
            .await;
            if cancelled {
                assert!(outcome.is_err());
            } else {
                assert_eq!(
                    outcome.expect("auxiliary deadline").expect("HTTP auxiliary").content.as_deref(),
                    Some("SAFE")
                );
            }
            assert_eq!(provider.websocket_continuation_snapshot(), Some(cached));
            assert!(provider.websocket_session.lock().await.is_some());
            let followup = LLMRequest {
                messages: vec![
                    ProviderMessage::user("hello".into()),
                    ProviderMessage::user("new primary input".into()),
                ]
                .into(),
                ..primary
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(3), provider.generate(followup))
                .await
                .expect("primary deadline")
                .expect("primary response");
            assert_eq!(result.content.as_deref(), Some("primary continued"));
            {
                let events = recorded.lock().expect("recorded events");
                assert_eq!(events.len(), 1, "auxiliary must not enter the primary WebSocket or force a warmup");
                assert_eq!(events[0]["previous_response_id"], "resp_primary");
                assert_eq!(events[0]["service_tier"], "ultrafast");
                assert_eq!(events[0]["input"].as_array().expect("incremental input").len(), 1);
            }
            handle.await.expect("fixture task");
        }
    }
}
#[tokio::test]
async fn websocket_reconnects_after_connection_limit_error() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Error {
            code: WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE,
            message: "Responses websocket connection limit reached (60 minutes).",
        }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed { response_id: "resp_reconnected", text: "ok" },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", true).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("websocket retry should succeed");

    assert_eq!(response.content.as_deref(), Some("ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].get("previous_response_id").and_then(Value::as_str), Some("resp_cached"));
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
        assert!(
            recorded
                .iter()
                .all(|payload| payload.get("store").and_then(Value::as_bool) == Some(false)),
            "websocket requests must enforce store=false"
        );
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_custom_provider_uses_compatible_transport() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
        ScriptedReply::Completed { response_id: "resp_final", text: "compat ok" },
    ]])
    .await
    else {
        return;
    };
    let compatible_base_url = base_url.replacen("api.openai.com", "compat.example", 1);
    let provider = websocket_test_custom_provider(compatible_base_url);

    let response = LLMProvider::generate(&provider, websocket_test_request())
        .await
        .expect("custom compatible provider should use websocket mode");

    assert_eq!(response.content.as_deref(), Some("compat ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].get("generate").and_then(Value::as_bool), Some(false));
        assert!(recorded[0].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_reconnects_after_active_response_error() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Error {
            code: "invalid_request_error",
            message: "Conversation already has an active response in progress: resp_active.",
        }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed {
                response_id: "resp_after_active",
                text: "retried ok",
            },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", true).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("active-response retry should succeed");

    assert_eq!(response.content.as_deref(), Some("retried ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].get("previous_response_id").and_then(Value::as_str), Some("resp_cached"));
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_failed_turn_drops_nonpersistent_continuation_before_retry() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Error {
            code: "invalid_request_error",
            message: "Conversation already has an active response in progress: resp_active.",
        }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed { response_id: "resp_after_reset", text: "reset ok" },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", false).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("failed turn should restart nonpersistent continuation cleanly");

    assert_eq!(response.content.as_deref(), Some("reset ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].get("previous_response_id").and_then(Value::as_str), Some("resp_cached"));
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_previous_response_not_found_restarts_new_chain() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Error {
            code: PREVIOUS_RESPONSE_NOT_FOUND_CODE,
            message: "Previous response with id 'resp_cached' not found.",
        }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed { response_id: "resp_final", text: "new chain ok" },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", true).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should recover by starting a new chain");

    assert_eq!(response.content.as_deref(), Some("new chain ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].get("previous_response_id").and_then(Value::as_str), Some("resp_cached"));
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_reconnects_after_close_frame() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Close { reason: "limit reached" }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed {
                response_id: "resp_after_close",
                text: "closed then ok",
            },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", true).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("close-frame retry should succeed");

    assert_eq!(response.content.as_deref(), Some("closed then ok"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
#[tokio::test]
async fn websocket_reconnect_drops_nonpersistent_continuation_before_retry() {
    let Some((base_url, recorded, handle)) = spawn_scripted_websocket_server(vec![
        vec![ScriptedReply::Close { reason: "network reset" }],
        vec![
            ScriptedReply::Completed { response_id: "resp_warmup", text: "" },
            ScriptedReply::Completed {
                response_id: "resp_final",
                text: "restarted cleanly",
            },
        ],
    ])
    .await
    else {
        return;
    };
    let provider = websocket_test_provider(base_url);
    let request = websocket_test_request();
    seed_connected_continuation_cache(&provider, &request, "resp_cached", false).await;

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("nonpersistent reconnect should restart the chain");

    assert_eq!(response.content.as_deref(), Some("restarted cleanly"));
    {
        let recorded = recorded.lock().expect("recorded lock");
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].get("previous_response_id").and_then(Value::as_str), Some("resp_cached"));
        assert!(recorded[1].get("previous_response_id").is_none());
        assert_eq!(recorded[1].get("generate").and_then(Value::as_bool), Some(false));
        assert_eq!(recorded[2].get("previous_response_id").and_then(Value::as_str), Some("resp_warmup"));
    }
    handle.await.expect("server task");
}
