use super::{
    OPENAI_BETA_RESPONSES_WEBSOCKET_V2, OpenAIResponsesWebSocketContinuationCache, PREVIOUS_RESPONSE_NOT_FOUND_CODE,
    WEBSOCKET_ACTIVE_RESPONSE_ERROR_PREFIX, WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE, apply_generate_mode,
    input_is_incremental, is_websocket_active_response_error, is_websocket_connection_limit_error,
    is_websocket_previous_response_not_found_error, prepare_websocket_event, responses_websocket_url,
};
use crate::provider::LLMError;
use crate::provider::{LLMProvider, LLMRequest, Message as ProviderMessage};
use crate::providers::openai::OpenAIProvider;
use futures::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use vtcode_config::core::OpenAIConfig;

#[test]
fn websocket_url_is_derived_from_http_base() {
    let ws = responses_websocket_url("https://api.openai.com/v1").expect("websocket url should be built");
    assert_eq!(ws, "wss://api.openai.com/v1/responses");
}

#[tokio::test]
async fn websocket_tls_startup_returns_error_without_crypto_provider_panic() {
    use tokio::io::AsyncReadExt;

    assert!(
        rustls::crypto::CryptoProvider::get_default().is_none(),
        "fixture must start without a global provider"
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind TLS fixture");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        for _attempt in 0..2 {
            let (mut socket, _) = listener.accept().await.expect("accept TLS connection");
            let mut record_type = [0];
            socket.read_exact(&mut record_type).await.expect("read TLS ClientHello");
            assert_eq!(record_type[0], 0x16, "client must initiate TLS, never plaintext HTTP");
            // Close during TLS negotiation; production must surface a network error.
        }
    });
    let provider = websocket_test_provider(format!("https://{address}/v1"));
    let mut session = None;
    let request = websocket_test_request();
    for _attempt in 0..2 {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            provider.ensure_websocket_session(&mut session, &request),
        )
        .await
        .expect("TLS startup must terminate");
        assert!(matches!(result, Err(LLMError::Network { .. })));
        assert!(session.is_none(), "failed TLS connection must not be retained");
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_none(),
            "connection must select its provider without changing process-wide TLS policy"
        );
    }
    server.await.expect("TLS fixture task");
}

#[test]
fn websocket_beta_header_prefers_v2_protocol() {
    assert_eq!(OPENAI_BETA_RESPONSES_WEBSOCKET_V2, "responses=v2");
}

#[test]
fn websocket_connection_limit_error_is_detected() {
    let err = LLMError::Network {
        message: format!("OpenAI error: {WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE}: limit reached"),
        metadata: None,
    };
    assert!(is_websocket_connection_limit_error(&err));
}

#[test]
fn websocket_non_connection_limit_error_is_not_detected() {
    let err = LLMError::Provider {
        message: "OpenAI error: invalid_request".to_string(),
        metadata: None,
    };
    assert!(!is_websocket_connection_limit_error(&err));
}

#[test]
fn websocket_active_response_error_is_detected() {
    let err = LLMError::Provider {
        message: format!("OpenAI error: invalid_request_error: {WEBSOCKET_ACTIVE_RESPONSE_ERROR_PREFIX} resp_active"),
        metadata: None,
    };
    assert!(is_websocket_active_response_error(&err));
}

#[test]
fn websocket_non_active_response_error_is_not_detected() {
    let err = LLMError::Provider {
        message: "OpenAI error: invalid_request_error: something else".to_string(),
        metadata: None,
    };
    assert!(!is_websocket_active_response_error(&err));
}

#[test]
fn websocket_previous_response_not_found_error_is_detected() {
    let err = LLMError::Provider {
        message: format!("OpenAI error: {PREVIOUS_RESPONSE_NOT_FOUND_CODE}: previous response missing"),
        metadata: None,
    };
    assert!(is_websocket_previous_response_not_found_error(&err));
}

#[test]
fn websocket_incremental_input_allows_empty_delta_for_v2_chaining() {
    let input = vec![Value::String("a".to_string())];
    assert!(input_is_incremental(&input, &input));
}

#[test]
fn websocket_incremental_input_requires_prefix_match() {
    let previous = vec![Value::String("a".to_string())];
    let current = vec![Value::String("b".to_string())];
    assert!(!input_is_incremental(&previous, &current));
}

#[test]
fn websocket_warmup_sets_generate_false() {
    let mut obj = Map::new();
    apply_generate_mode(&mut obj, true);
    assert_eq!(obj.get("generate"), Some(&Value::Bool(false)));
}

#[test]
fn websocket_non_warmup_removes_generate_flag() {
    let mut obj = Map::new();
    obj.insert("generate".to_string(), Value::Bool(false));
    apply_generate_mode(&mut obj, false);
    assert!(obj.get("generate").is_none());
}

#[test]
fn websocket_prepare_event_starts_new_chain_without_continuation() {
    let payload = json!({
        "model": "gpt-5.6",
        "input": [{"role": "user", "content": "hello"}],
        "previous_response_id": "resp_prev",
        "store": true,
        "stream": false,
        "background": false,
    });

    let prepared = prepare_websocket_event(&payload, None, false).expect("event should prepare");

    assert_eq!(
        prepared.event.get("previous_response_id"),
        None,
        "fresh websocket chain should not reuse caller-supplied response ids"
    );
    assert_eq!(prepared.event.get("input").and_then(Value::as_array), Some(&prepared.full_input));
    assert_eq!(prepared.event.get("store").and_then(Value::as_bool), Some(false));
    assert!(!prepared.used_previous_response_id);
}

#[test]
fn websocket_prepare_event_forwards_ultrafast_service_tier_on_every_event() {
    for warmup in [false, true] {
        let payload = json!({
            "model": "gpt-6-astra",
            "input": [{"role": "user", "content": "hello"}],
            "service_tier": "ultrafast",
            "store": true,
            "stream": true,
            "background": true,
        });

        let prepared = prepare_websocket_event(&payload, None, warmup).expect("event should prepare");

        assert_eq!(
            prepared.event.get("service_tier").and_then(Value::as_str),
            Some("ultrafast"),
            "warmup={warmup}: websocket response.create must carry service_tier per event"
        );
        assert_eq!(prepared.event.get("type").and_then(Value::as_str), Some("response.create"));
        assert!(prepared.event.get("stream").is_none());
        assert!(prepared.event.get("background").is_none());
    }
}

#[test]
fn websocket_prepare_event_reuses_matching_continuation_incrementally() {
    let full_input = vec![
        json!({"role": "user", "content": "hello"}),
        json!({"role": "user", "content": "continue"}),
    ];
    let continuation = OpenAIResponsesWebSocketContinuationCache {
        response_id: "resp_prev".to_string(),
        full_input: vec![full_input[0].clone()],
        model: "gpt-5.6".to_string(),
        instructions: None,
        tools: None,
    };
    let payload = json!({
        "model": "gpt-5.6",
        "input": full_input,
    });

    let prepared = prepare_websocket_event(&payload, Some(&continuation), false).expect("event");

    assert_eq!(prepared.event.get("previous_response_id").and_then(Value::as_str), Some("resp_prev"));
    assert_eq!(
        prepared.event.get("input").and_then(Value::as_array),
        Some(&vec![json!({"role": "user", "content": "continue"})])
    );
    assert!(prepared.used_previous_response_id);
}

#[test]
fn websocket_prepare_event_starts_new_chain_on_non_prefix_input() {
    let continuation = OpenAIResponsesWebSocketContinuationCache {
        response_id: "resp_prev".to_string(),
        full_input: vec![json!({"role": "user", "content": "hello"})],
        model: "gpt-5.6".to_string(),
        instructions: None,
        tools: None,
    };
    let payload = json!({
        "model": "gpt-5.6",
        "input": [json!({"role": "user", "content": "different"})],
        "previous_response_id": "resp_prev",
    });

    let prepared = prepare_websocket_event(&payload, Some(&continuation), false).expect("event");

    assert!(prepared.event.get("previous_response_id").is_none());
    assert_eq!(prepared.event.get("input").and_then(Value::as_array), Some(&prepared.full_input));
    assert_eq!(prepared.event.get("store").and_then(Value::as_bool), Some(false));
    assert!(!prepared.used_previous_response_id);
}

#[derive(Clone)]
enum ScriptedReply {
    Events(Vec<Value>),
    Frames(Vec<Message>),
    Paused {
        events: Vec<Value>,
        gate: Arc<tokio::sync::Notify>,
        completion: Value,
    },
    Completed {
        response_id: &'static str,
        text: &'static str,
    },
    Error {
        code: &'static str,
        message: &'static str,
    },
    Close {
        reason: &'static str,
    },
}

async fn spawn_scripted_websocket_server(
    sessions: Vec<Vec<ScriptedReply>>,
) -> Option<(String, Arc<StdMutex<Vec<Value>>>, tokio::task::JoinHandle<()>)> {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return None,
        Err(err) => panic!("listener should bind: {err}"),
    };
    let addr = listener.local_addr().expect("listener addr");
    let recorded = Arc::new(StdMutex::new(Vec::new()));
    let recorded_handle = Arc::clone(&recorded);
    let (ready_tx, ready_rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let _ = ready_tx.send(());
        for session_script in sessions {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut websocket = accept_async(stream).await.expect("handshake");
            for reply in session_script {
                let message = websocket
                    .next()
                    .await
                    .expect("request should be present")
                    .expect("request should parse");
                let Message::Text(text) = message else {
                    panic!("expected text websocket payload");
                };
                let payload: Value = serde_json::from_str(text.as_ref()).expect("payload should be valid json");
                recorded_handle.lock().expect("recorded lock").push(payload);

                match reply {
                    ScriptedReply::Events(events) => {
                        for event in events {
                            websocket.send(Message::Text(event.to_string().into())).await.expect("event");
                        }
                    }
                    ScriptedReply::Frames(frames) => {
                        for frame in frames {
                            websocket.send(frame).await.expect("frame");
                        }
                    }
                    ScriptedReply::Paused { events, gate, completion } => {
                        for event in events {
                            websocket.send(Message::Text(event.to_string().into())).await.expect("event");
                        }
                        gate.notified().await;
                        let _ = websocket.send(Message::Text(completion.to_string().into())).await;
                    }
                    ScriptedReply::Completed { response_id, text } => {
                        let event = json!({
                            "type": "response.completed",
                            "response": {
                                "id": response_id,
                                "output": [{
                                    "type": "message",
                                    "content": [{
                                        "type": "output_text",
                                        "text": text,
                                    }]
                                }]
                            }
                        });
                        websocket
                            .send(Message::Text(event.to_string().into()))
                            .await
                            .expect("response event");
                    }
                    ScriptedReply::Error { code, message } => {
                        let event = json!({
                            "type": "error",
                            "status": 400,
                            "error": {
                                "code": code,
                                "message": message,
                            }
                        });
                        websocket
                            .send(Message::Text(event.to_string().into()))
                            .await
                            .expect("error event");
                        break;
                    }
                    ScriptedReply::Close { reason } => {
                        websocket
                            .send(Message::Close(Some(CloseFrame {
                                code: CloseCode::Away,
                                reason: reason.to_string().into(),
                            })))
                            .await
                            .expect("close frame");
                        break;
                    }
                }
            }
        }
    });

    ready_rx.await.expect("server ready");
    Some((format!("http://api.openai.com@127.0.0.1:{}/v1", addr.port()), recorded, handle))
}

fn websocket_test_provider(base_url: String) -> OpenAIProvider {
    OpenAIProvider::from_config(
        Some("test-key".to_string()),
        None,
        Some("gpt-5.6".to_string()),
        Some(base_url),
        None,
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    )
}

fn websocket_test_custom_provider(base_url: String) -> OpenAIProvider {
    OpenAIProvider::from_custom_config(
        "mycorp".to_string(),
        "MyCorp".to_string(),
        Some("test-key".to_string()),
        Some("gpt-5.6".to_string()),
        Some(base_url),
        None,
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
        None,
        None,
    )
}

fn websocket_test_request() -> LLMRequest {
    LLMRequest {
        model: "gpt-5.6".to_string(),
        messages: vec![ProviderMessage::user("hello".to_string())].into(),
        ..Default::default()
    }
}

fn seed_continuation_cache(provider: &OpenAIProvider, request: &LLMRequest, response_id: &str, store: bool) {
    let payload = provider
        .convert_to_openai_responses_format(request)
        .expect("payload should serialize");
    let full_input = payload.get("input").and_then(Value::as_array).cloned().expect("full input");
    let mut event = payload;
    event["store"] = Value::Bool(store);
    event["generate"] = Value::Bool(false);

    provider.update_websocket_continuation(
        &json!({ "id": response_id }),
        request,
        &super::PreparedWebSocketEvent {
            event,
            full_input,
            used_previous_response_id: false,
        },
        "gpt-5.6",
    );
}

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
                let response = super::super::super::responses_api::parse_responses_completion(
                    completion.clone(),
                    "gpt-5.6".into(),
                    false,
                )
                .expect("server completion");
                assert_eq!(response.content.as_deref(), Some("primary answer"));
                let replay = super::completed_replay_output(&completion, "gpt-5.6").expect("canonical replay");
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

async fn seed_connected_continuation_cache(
    provider: &OpenAIProvider,
    request: &LLMRequest,
    response_id: &str,
    store: bool,
) {
    let mut slot = provider.websocket_session.lock().await;
    provider
        .ensure_websocket_session(&mut slot, request)
        .await
        .expect("seed connection");
    seed_continuation_cache(provider, request, response_id, store);
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
fn completion(id: &str, output: Value) -> Value {
    json!({"type":"response.completed", "response":{
        "id":id, "status":"completed", "output":output,
        "usage":{"input_tokens":17,"output_tokens":9,"total_tokens":26,"input_tokens_details":{"cached_tokens":4}}
    }})
}

fn text_delta(text: &str) -> Value {
    json!({"type":"response.output_text.delta", "delta":text, "item_id":"msg_1", "output_index":0, "content_index":0, "sequence_number":1})
}

fn text_output(text: &str) -> Value {
    json!([{"id":"msg_1", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":text,"annotations":[]}]}])
}

async fn final_normalized(mut stream: crate::provider::LLMNormalizedStream) -> crate::provider::LLMResponse {
    while let Some(event) = stream.next().await {
        if let crate::provider::NormalizedStreamEvent::Done { response } = event.expect("stream event") {
            return *response;
        }
    }
    panic!("missing completion");
}

#[tokio::test]
async fn websocket_stream_deltas_precede_completion_and_legacy_preserves_empty_output() {
    use crate::provider::{LLMStreamEvent, NormalizedStreamEvent};
    for legacy in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Paused {
                events: vec![text_delta("before")],
                gate: Arc::clone(&gate),
                completion: completion("final", json!([])),
            },
        ]])
        .await
        else {
            panic!("local server unavailable");
        };
        let provider = websocket_test_provider(base);
        let response = if legacy {
            let mut stream = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                provider.stream_request(websocket_test_request()),
            )
            .await
            .expect("startup before completion")
            .expect("stream");
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), LLMStreamEvent::Token { delta } if delta == "before")
            );
            gate.notify_one();
            match stream.next().await.unwrap().unwrap() {
                LLMStreamEvent::Completed { response } => *response,
                event => panic!("unexpected event: {event:?}"),
            }
        } else {
            let mut stream = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                provider.stream_normalized_request(websocket_test_request()),
            )
            .await
            .expect("startup before completion")
            .expect("stream");
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::TextDelta { delta } if delta == "before")
            );
            gate.notify_one();
            final_normalized(stream).await
        };
        assert_eq!(response.content.as_deref(), Some("before"));
        assert_eq!(response.request_id.as_deref(), Some("final"));
        assert_eq!(response.usage.unwrap().total_tokens, 26);
        assert_eq!(recorded.lock().unwrap().len(), 2);
        assert!(provider.websocket_session.lock().await.is_some());
        assert!(
            provider.websocket_continuation_snapshot().is_none(),
            "empty authoritative output cannot safely match replay"
        );
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn websocket_stream_reasoning_refusal_and_tools_preserve_ids_and_usage() {
    use crate::provider::{NormalizedStreamEvent, ReasoningSource};
    let tool = json!({"type":"function_call","id":"fc_wire","call_id":"call_real","name":"lookup","arguments":"{\"city\":\"Hue\"}","status":"completed"});
    let events = vec![
        json!({"type":"response.reasoning_summary_text.delta","delta":"summary","item_id":"rs_1","output_index":0,"summary_index":0,"sequence_number":1}),
        json!({"type":"response.reasoning_text.delta","delta":"private","item_id":"rs_1","output_index":0,"content_index":0,"sequence_number":2}),
        json!({"type":"response.refusal.delta","delta":"refused","item_id":"msg_1","output_index":1,"content_index":0,"sequence_number":3}),
        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_wire","call_id":"call_real","name":"lookup","arguments":"","status":"in_progress"},"sequence_number":4}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_wire","output_index":2,"delta":"{\"city\":","sequence_number":5}),
        json!({"type":"response.function_call_arguments.delta","item_id":"fc_wire","output_index":2,"delta":"\"Hue\"}","sequence_number":6}),
        completion("tools", json!([tool])),
    ];
    let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Events(events.clone()),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    let mut request = websocket_test_request();
    // Exercise the existing reasoning-emission policy with an o-series
    // fixture; GPT families deliberately suppress public summaries.
    request.model = "o3".to_string();
    provider.set_responses_api_state("o3", super::super::super::types::ResponsesApiState::Allowed);
    let mut stream = provider.stream_normalized_request(request).await.unwrap();
    let mut seen = Vec::new();
    while let Some(event) = stream.next().await {
        seen.push(event.unwrap());
    }
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ReasoningDelta { delta, source:ReasoningSource::ProviderSummary } if delta == "summary")));
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ReasoningDelta { delta, source:ReasoningSource::Continuation } if delta == "private")));
    assert!(
        seen.iter()
            .any(|event| matches!(event, NormalizedStreamEvent::TextDelta { delta } if delta == "refused"))
    );
    assert!(
        seen.iter().any(
            |event| matches!(event, NormalizedStreamEvent::ToolCallStart { call_id, .. } if call_id == "call_real")
        )
    );
    assert!(seen.iter().any(|event| matches!(event, NormalizedStreamEvent::ToolCallDelta { call_id, delta } if call_id == "call_real" && delta == "\"Hue\"}")));
    let NormalizedStreamEvent::Done { response } = seen.last().unwrap() else {
        panic!("done");
    };
    assert_eq!(response.content.as_deref(), Some("refused"));
    let calls = response.tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_real");
    assert_eq!(calls[0].function.as_ref().unwrap().arguments, "{\"city\":\"Hue\"}");
    assert_eq!(response.usage.as_ref().unwrap().prompt_tokens, 17);
    handle.await.unwrap();

    let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Events(events),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    provider.set_responses_api_state("o3", super::super::super::types::ResponsesApiState::Allowed);
    let mut request = websocket_test_request();
    request.model = "o3".to_string();
    let mut stream = provider.stream_request(request).await.unwrap();
    let mut reasoning = String::new();
    let mut response = None;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            crate::provider::LLMStreamEvent::Reasoning { delta } => reasoning.push_str(&delta),
            crate::provider::LLMStreamEvent::Completed { response: done } => response = Some(done),
            _ => {}
        }
    }
    assert_eq!(reasoning, "summaryprivate");
    let response = response.unwrap();
    assert_eq!(response.content.as_deref(), Some("refused"));
    assert_eq!(response.tool_calls.as_ref().unwrap()[0].id, "call_real");
    assert_eq!(response.tool_calls.as_ref().unwrap()[0].function.as_ref().unwrap().arguments, "{\"city\":\"Hue\"}");
    assert_eq!(response.usage.as_ref().unwrap().total_tokens, 26);
    handle.await.unwrap();
}

#[tokio::test]
async fn websocket_stream_tool_followup_sends_only_new_input_on_one_connection() {
    for tier in ["flex", "ultrafast"] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("tool_turn", json!([{"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"key\":1}"}]))]),
            ScriptedReply::Events(vec![completion("reply", text_output("found"))]),
            ScriptedReply::Events(vec![completion("last", text_output("done"))]),
        ]]).await else { panic!("server unavailable"); };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        request.service_tier = Some(tier.to_string());
        let response = final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await;
        Arc::make_mut(&mut request.messages)
            .push(ProviderMessage::assistant_with_tools("".to_string(), response.tool_calls.unwrap()));
        Arc::make_mut(&mut request.messages)
            .push(ProviderMessage::tool_response("call_1".to_string(), "result".to_string()));
        let response = final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await;
        Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant(response.content.unwrap()));
        Arc::make_mut(&mut request.messages).push(ProviderMessage::user("next".to_string()));
        final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert_eq!(requests[0]["generate"], false);
            assert_eq!(requests[1]["previous_response_id"], "warm");
            assert_eq!(requests[1]["input"], json!([]));
            assert_eq!(requests[2]["previous_response_id"], "tool_turn");
            assert_eq!(
                requests[2]["input"],
                json!([{"type":"function_call_output", "call_id":"call_1", "output":"result"}])
            );
            assert_eq!(requests[3]["previous_response_id"], "reply");
            assert_eq!(
                requests[3]["input"],
                json!([{"role":"user", "content":[{"type":"input_text", "text":"next"}]}])
            );
            assert!(
                requests
                    .iter()
                    .all(|event| event["service_tier"] == tier && event["store"] == false)
            );
        }

        handle.await.unwrap();
    }
}

#[test]
fn websocket_history_matches_only_cache_annotations_and_starts_fresh_for_changes() {
    let payload = json!({"model":"gpt-5.6","instructions":"stable","tools":[{"name":"a"}],"input":[{"role":"user","content":[{"type":"input_text","text":"first","prompt_cache_breakpoint":true}]}]});
    let prepared = prepare_websocket_event(&payload, None, true).unwrap();
    let cache = OpenAIResponsesWebSocketContinuationCache::from_response(
        &json!({"id":"warm"}),
        &websocket_test_request(),
        &prepared,
        "gpt-5.6",
    )
    .unwrap();
    let mut next = payload.clone();
    next["input"][0]["content"][0]
        .as_object_mut()
        .unwrap()
        .remove("prompt_cache_breakpoint");
    assert!(
        prepare_websocket_event(&next, Some(&cache), false)
            .unwrap()
            .used_previous_response_id
    );
    for (field, value) in [
        ("model", json!("gpt-6-astra")),
        ("instructions", json!("changed")),
        ("tools", json!([])),
        ("input", json!([])),
        ("input", json!([{"type":"compaction","encrypted_content":"opaque"}])),
    ] {
        let mut changed = next.clone();
        changed[field] = value;
        assert!(
            !prepare_websocket_event(&changed, Some(&cache), false)
                .unwrap()
                .used_previous_response_id,
            "{field}"
        );
    }
    next["input"][0]["content"][0]["text"] = json!("edited");
    assert!(
        !prepare_websocket_event(&next, Some(&cache), false)
            .unwrap()
            .used_previous_response_id
    );
}

#[tokio::test]
async fn websocket_stream_early_drop_and_cancellation_invalidate_and_reconnect() {
    for before_output in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
            vec![
                ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                ScriptedReply::Paused {
                    events: if before_output {
                        vec![]
                    } else {
                        vec![text_delta("partial")]
                    },
                    gate: Arc::clone(&gate),
                    completion: completion("cancelled", text_output("partial")),
                },
            ],
            vec![
                ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                ScriptedReply::Events(vec![completion("fresh", text_output("fresh"))]),
            ],
        ])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        if before_output {
            let startup = provider.stream_normalized_request(websocket_test_request());
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), startup)
                    .await
                    .is_err()
            );
        } else {
            let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
            assert!(stream.next().await.unwrap().is_ok());
            drop(stream);
        }
        assert!(provider.websocket_session.lock().await.is_none());
        assert!(provider.websocket_continuation_snapshot().is_none());
        gate.notify_one();
        let response =
            final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap()).await;
        assert_eq!(response.content.as_deref(), Some("fresh"));
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 4);
            assert!(requests[2].get("previous_response_id").is_none());
            assert_eq!(requests[2]["generate"], false);
            assert_eq!(requests[3]["previous_response_id"], "warm_new");
        }

        handle.await.unwrap();
    }
}

async fn http_stream_server(expected: u64) -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let body = format!("data: {}\n\ndata: {}\n\n", text_delta("http"), completion("http_final", text_output("http")));
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(expected)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn websocket_pre_output_failures_use_http_once_and_post_output_never_replay() {
    let failures = vec![
        vec![Message::Close(None)],
        vec![], // Socket EOF when the script ends.
        vec![Message::Text("{invalid".into())],
        vec![Message::Text(json!({"type":"response.failed","response":{"error":{"message":"failed"}}}).to_string().into())],
        vec![Message::Text(json!({"type":"response.incomplete","response":{"status":"incomplete"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"invalid_request_error","message":"Conversation already has an active response in progress: resp_active"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"websocket_connection_limit_reached","message":"limit"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"previous_response_not_found","message":"missing"}}).to_string().into())],
        vec![Message::Text(json!({"type":"response.completed","response":{"id":"empty","output":[]}}).to_string().into())],
    ];
    for legacy in [false, true] {
        for post_output in [false, true] {
            for failure in &failures {
                let mut frames = Vec::new();
                if post_output {
                    frames.push(Message::Text(text_delta("partial").to_string().into()));
                }
                // An empty successful completion after a delta is valid recovery,
                // covered above; use a missing response to test malformed completion.
                if post_output
                    && failure
                        .first()
                        .is_some_and(|frame| matches!(frame,Message::Text(text) if text.contains("response.completed")))
                {
                    frames.push(Message::Text(json!({"type":"response.completed"}).to_string().into()));
                } else {
                    frames.extend(failure.clone());
                }
                let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
                    ScriptedReply::Events(vec![completion("warm", json!([]))]),
                    ScriptedReply::Frames(frames),
                ]])
                .await
                else {
                    panic!("server unavailable");
                };
                let http = http_stream_server(if post_output { 0 } else { 1 }).await;
                let mut provider = websocket_test_provider(base);
                provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
                if legacy {
                    let mut stream = provider.stream_request(websocket_test_request()).await.unwrap();
                    let mut errors = 0;
                    let mut text = String::new();
                    let mut done = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(crate::provider::LLMStreamEvent::Token { delta }) => text.push_str(&delta),
                            Ok(crate::provider::LLMStreamEvent::Completed { .. }) => done = true,
                            Err(_) => errors += 1,
                            _ => {}
                        }
                    }
                    assert_eq!(text, if post_output { "partial" } else { "http" });
                    assert_eq!(errors, usize::from(post_output));
                    assert_eq!(done, !post_output);
                } else {
                    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
                    let mut errors = 0;
                    let mut text = String::new();
                    let mut done = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(crate::provider::NormalizedStreamEvent::TextDelta { delta }) => text.push_str(&delta),
                            Ok(crate::provider::NormalizedStreamEvent::Done { .. }) => done = true,
                            Err(_) => errors += 1,
                            _ => {}
                        }
                    }
                    assert_eq!(text, if post_output { "partial" } else { "http" });
                    assert_eq!(errors, usize::from(post_output));
                    assert_eq!(done, !post_output);
                }
                assert_eq!(recorded.lock().unwrap().len(), 2, "one warmup and one generation, no WS retry");
                assert!(provider.websocket_session.lock().await.is_none());
                assert!(provider.websocket_continuation_snapshot().is_none());
                assert_eq!(http.received_requests().await.unwrap().len(), usize::from(!post_output));
                handle.await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn websocket_tier_rejection_uses_tierless_http_then_cached_reconnect() {
    for legacy in [false, true] {
        for warmup_rejected in [false, true] {
            for tier in ["flex", "ultrafast"] {
                let mut first = Vec::new();
                if !warmup_rejected {
                    first.push(ScriptedReply::Events(vec![completion("warm_old", json!([]))]));
                }
                first.push(ScriptedReply::Events(vec![json!({"type":"error", "status":400,"error":{"code":"invalid_request_error","param":"service_tier","message":"The requested service tier is not allowed for this project"}})]));
                let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
                    first,
                    vec![
                        ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                        ScriptedReply::Events(vec![completion("ws_final", text_output("cached"))]),
                    ],
                ])
                .await
                else {
                    panic!("server unavailable");
                };
                let http = http_stream_server(1).await;
                let mut provider = websocket_test_provider(base);
                provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
                let mut request = websocket_test_request();
                request.model = "gpt-6-astra".to_string();
                request.service_tier = Some(tier.to_string());
                if legacy {
                    let mut stream = provider.stream_request(request.clone()).await.unwrap();
                    let mut content = String::new();
                    while let Some(event) = stream.next().await {
                        if let crate::provider::LLMStreamEvent::Completed { response } = event.unwrap() {
                            content = response.content.unwrap();
                        }
                    }
                    assert_eq!(content, "http");
                } else {
                    assert_eq!(
                        final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap())
                            .await
                            .content
                            .as_deref(),
                        Some("http")
                    );
                }
                let response = final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
                assert_eq!(response.content.as_deref(), Some("cached"));
                let http_requests = http.received_requests().await.unwrap();
                let body: Value = serde_json::from_slice(&http_requests[0].body).unwrap();
                assert!(body.get("service_tier").is_none());
                {
                    let requests = recorded.lock().unwrap();
                    let failed_count = if warmup_rejected { 1 } else { 2 };
                    assert_eq!(requests.len(), failed_count + 2);
                    assert!(requests[..failed_count].iter().all(|event| event["service_tier"] == tier));
                    assert!(requests[failed_count..].iter().all(|event| event.get("service_tier").is_none()));
                    assert!(requests[failed_count].get("previous_response_id").is_none());
                    assert_eq!(requests[failed_count + 1]["previous_response_id"], "warm_new");
                }

                handle.await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn websocket_blank_and_missing_tier_are_omitted_and_opt_out_uses_http() {
    for tier in [None, Some("   ".to_string())] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("reply", text_output("ok"))]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.service_tier = tier;
        final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
        assert!(recorded.lock().unwrap().iter().all(|event| event.get("service_tier").is_none()));
        handle.await.unwrap();
    }
    for unsupported in [false, true] {
        let http = http_stream_server(2).await;
        let mut provider = websocket_test_provider("http://127.0.0.1:1/v1".to_string());
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        if unsupported {
            provider.backend_setup = super::super::super::backend_setup::OpenAIBackendSetup::chatgpt_subscription_rig(
                provider.base_url.to_string(),
            );
        } else {
            provider.websocket_mode = false;
        }
        assert!(!provider.websocket_mode_enabled("gpt-5.6"));
        final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap()).await;
        let mut stream = provider.stream_request(websocket_test_request()).await.unwrap();
        while let Some(event) = stream.next().await {
            event.unwrap();
        }
        assert_eq!(http.received_requests().await.unwrap().len(), 2);
    }
}

#[tokio::test]
async fn websocket_stream_and_generation_share_a_serial_lane() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Paused {
            events: vec![text_delta("first")],
            gate: Arc::clone(&gate),
            completion: completion("first_response", text_output("first")),
        },
        ScriptedReply::Events(vec![completion("second_response", text_output("second"))]),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    let mut next = websocket_test_request();
    Arc::make_mut(&mut next.messages).push(ProviderMessage::assistant("first".to_string()));
    Arc::make_mut(&mut next.messages).push(ProviderMessage::user("second".to_string()));
    let generation = provider.generate_via_responses_websocket(&next);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), generation)
            .await
            .is_err()
    );
    assert_eq!(recorded.lock().unwrap().len(), 2, "queued request must not send on the active lane");
    gate.notify_one();
    final_normalized(stream).await;
    let response = provider.generate_via_responses_websocket(&next).await.unwrap();
    assert_eq!(response.content.as_deref(), Some("second"));
    {
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2]["previous_response_id"], "first_response");
        assert_eq!(requests[2]["input"], json!([{"role":"user","content":[{"type":"input_text","text":"second"}]}]));
    }

    handle.await.unwrap();
}

#[tokio::test]
async fn websocket_stream_owns_provider_state_and_control_frames_do_not_extend_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(socket).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Text(completion("warm", json!([])).to_string().into()))
            .await
            .unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Text(text_delta("start").to_string().into()))
            .await
            .unwrap();
        let mut pongs = 0;
        for _ in 0..20 {
            if socket.send(Message::Ping(vec![1, 2].into())).await.is_err() {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(20), socket.next()).await {
                Ok(Some(Ok(Message::Pong(_)))) => pongs += 1,
                Ok(Some(Err(_))) | Ok(None) => break,
                _ => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        pongs
    });
    let mut provider = websocket_test_provider(format!("http://api.openai.com@{addr}/v1"));
    provider.websocket_streaming_ceiling = std::time::Duration::from_millis(100);
    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
    drop(provider);
    assert!(stream.next().await.unwrap().is_ok());
    let error = tokio::time::timeout(std::time::Duration::from_millis(400), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("deadline exceeded"));
    assert!(stream.next().await.is_none());
    assert!(task.await.unwrap() > 0, "ping must receive a pong while the request is live");
    let provider = OpenAIProvider::from_config(
        Some("test".to_string()),
        None,
        Some("gpt-5.6".to_string()),
        None,
        None,
        Some(vtcode_config::TimeoutsConfig { streaming_ceiling_seconds: 0, ..Default::default() }),
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert_eq!(provider.websocket_streaming_ceiling, std::time::Duration::from_secs(600));
}
#[tokio::test]
async fn websocket_tool_start_failure_respects_caller_output_boundary() {
    let start = json!({"type":"response.output_item.added", "output_index":0, "sequence_number":1,
        "item":{"type":"function_call", "id":"fc_1", "call_id":"call_1", "name":"lookup", "arguments":"", "status":"in_progress"}});
    for legacy in [false, true] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![
                start.clone(),
                json!({"type":"response.failed", "response":{"error":{"message":"after tool"}}}),
            ]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let http = http_stream_server(u64::from(legacy)).await;
        let mut provider = websocket_test_provider(base);
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        if legacy {
            let response = final_legacy(provider.stream_request(websocket_test_request()).await.unwrap()).await;
            assert_eq!(response.content.as_deref(), Some("http"));
        } else {
            let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
            assert!(
                matches!(stream.next().await.unwrap().unwrap(),crate::provider::NormalizedStreamEvent::ToolCallStart{call_id,..} if call_id=="call_1")
            );
            assert!(stream.next().await.unwrap().unwrap_err().to_string().contains("after tool"));
            assert!(stream.next().await.is_none());
        }
        assert_eq!(recorded.lock().unwrap().len(), 2);
        assert_eq!(http.received_requests().await.unwrap().len(), usize::from(legacy));
        assert!(provider.websocket_session.lock().await.is_none());
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn websocket_usage_before_done_keeps_the_stream_lease_until_delivery() {
    use crate::provider::NormalizedStreamEvent;
    for text_delta_sent in [false, true] {
        for drop_before_done in [false, true] {
            let mut events = Vec::new();
            if text_delta_sent {
                events.push(text_delta("old reply"));
            }
            events.push(completion("old", text_output("old reply")));
            let mut first_connection = vec![
                ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                ScriptedReply::Events(events),
            ];
            let script = if drop_before_done {
                vec![
                    first_connection,
                    vec![
                        ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                        ScriptedReply::Events(vec![completion("new", text_output("new reply"))]),
                    ],
                ]
            } else {
                first_connection.push(ScriptedReply::Events(vec![completion("new", text_output("new reply"))]));
                vec![first_connection]
            };
            let Some((base, recorded, handle)) = spawn_scripted_websocket_server(script).await else {
                panic!("server unavailable");
            };
            let provider = websocket_test_provider(base);
            let mut request = websocket_test_request();
            let mut stream = provider.stream_normalized_request(request.clone()).await.unwrap();
            if text_delta_sent {
                assert!(
                    matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::TextDelta{delta} if delta == "old reply")
                );
            }
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::Usage{usage} if usage.total_tokens == 26)
            );
            if !drop_before_done {
                assert!(
                    matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::Done{response} if response.content.as_deref() == Some("old reply"))
                );
            }
            drop(stream);
            assert_eq!(provider.websocket_session.lock().await.is_none(), drop_before_done);
            assert_eq!(provider.websocket_continuation_snapshot().is_none(), drop_before_done);
            Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant("old reply".to_string()));
            Arc::make_mut(&mut request.messages).push(ProviderMessage::user("next".to_string()));
            assert_eq!(
                final_normalized(provider.stream_normalized_request(request).await.unwrap())
                    .await
                    .content
                    .as_deref(),
                Some("new reply")
            );
            {
                let requests = recorded.lock().unwrap();
                assert_eq!(requests.len(), if drop_before_done { 4 } else { 3 });
                if drop_before_done {
                    assert!(requests[2].get("previous_response_id").is_none());
                    assert_eq!(requests[2]["generate"], false);
                    assert_eq!(requests[3]["previous_response_id"], "warm_new");
                    assert_eq!(requests[3]["input"], json!([]));
                } else {
                    assert_eq!(requests[2]["previous_response_id"], "old");
                    assert_eq!(
                        requests[2]["input"],
                        json!([{"role":"user","content":[{"type":"input_text","text":"next"}]}])
                    );
                }
            }
            handle.await.unwrap();
        }
    }
}

#[tokio::test]
async fn websocket_unpolled_completion_stream_drop_invalidates_state() {
    for legacy in [false, true] {
        for include_usage in [false, true] {
            let mut completed = completion("old", text_output("old reply"));
            if !include_usage {
                completed["response"].as_object_mut().unwrap().remove("usage");
            }
            let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
                vec![
                    ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                    ScriptedReply::Events(vec![completed]),
                ],
                vec![
                    ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                    ScriptedReply::Events(vec![completion("new", text_output("new reply"))]),
                ],
            ])
            .await
            else {
                panic!("server unavailable");
            };
            let provider = websocket_test_provider(base);
            if legacy {
                drop(provider.stream_request(websocket_test_request()).await.unwrap());
            } else {
                drop(provider.stream_normalized_request(websocket_test_request()).await.unwrap());
            }
            assert!(provider.websocket_session.lock().await.is_none(), "legacy={legacy} usage={include_usage}");
            assert!(provider.websocket_continuation_snapshot().is_none());
            assert_eq!(
                final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap())
                    .await
                    .content
                    .as_deref(),
                Some("new reply")
            );
            {
                let requests = recorded.lock().unwrap();
                assert_eq!(requests.len(), 4);
                assert!(requests[2].get("previous_response_id").is_none());
                assert_eq!(requests[2]["generate"], false);
                assert_eq!(requests[3]["previous_response_id"], "warm_new");
            }
            handle.await.unwrap();
        }
    }
}
async fn final_legacy(mut stream: crate::provider::LLMStream) -> crate::provider::LLMResponse {
    while let Some(event) = stream.next().await {
        if let crate::provider::LLMStreamEvent::Completed { response } = event.expect("legacy event") {
            return *response;
        }
    }
    panic!("missing legacy completion");
}

fn private_reasoning_delta() -> Value {
    json!({"type":"response.reasoning_text.delta", "item_id":"rs_private", "output_index":0,
        "content_index":0, "sequence_number":1, "delta":"private transport reasoning"})
}

#[tokio::test]
async fn websocket_legacy_gpt_reasoning_matches_http_after_final_assembly() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for empty_output in [false, true] {
        let events = vec![
            private_reasoning_delta(),
            text_delta("visible"),
            completion(
                "response",
                if empty_output {
                    json!([])
                } else {
                    text_output("visible")
                },
            ),
        ];
        let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(events.clone()),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let http = MockServer::start().await;
        let body = events.iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(1)
            .mount(&http)
            .await;
        let mut provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        let websocket_response = final_legacy(provider.stream_request(request.clone()).await.unwrap()).await;
        provider.websocket_mode = false;
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        let http_response = final_legacy(provider.stream_request(request).await.unwrap()).await;
        assert_eq!(websocket_response.content, http_response.content);
        assert_eq!(websocket_response.content.as_deref(), Some("visible"));
        assert!(http_response.reasoning.is_none());
        assert!(websocket_response.reasoning.is_none(), "legacy completion must reapply model policy after merge");
        assert!(websocket_response.reasoning_details.is_none());
        assert_eq!(websocket_response.usage.as_ref().unwrap().total_tokens, 26);
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn websocket_gpt_reasoning_replay_chains_using_the_returned_history() {
    let output = json!([
        {"type":"reasoning", "id":"rs_private", "encrypted_content":"opaque", "summary":[]},
        {"type":"message", "id":"msg_first", "role":"assistant", "content":[{"type":"output_text", "text":"visible", "annotations":[]}]}
    ]);
    for entrypoint in ["generate", "legacy", "normalized"] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("first", output.clone())]),
            ScriptedReply::Events(vec![completion("second", text_output("next reply"))]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        let first = match entrypoint {
            "generate" => provider.generate_request(request.clone()).await.unwrap(),
            "legacy" => final_legacy(provider.stream_request(request.clone()).await.unwrap()).await,
            _ => final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await,
        };
        assert!(first.reasoning_details.is_none());
        Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant_with_tools_and_reasoning(
            first.content.unwrap(),
            first.tool_calls.unwrap_or_default(),
            None,
        ));
        Arc::make_mut(&mut request.messages).push(ProviderMessage::user("new turn".to_string()));
        let second = match entrypoint {
            "generate" => provider.generate_request(request).await.unwrap(),
            "legacy" => final_legacy(provider.stream_request(request).await.unwrap()).await,
            _ => final_normalized(provider.stream_normalized_request(request).await.unwrap()).await,
        };
        assert_eq!(second.content.as_deref(), Some("next reply"));
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert_eq!(requests[2]["previous_response_id"], "first", "{entrypoint}");
            assert_eq!(
                requests[2]["input"],
                json!([{"role":"user", "content":[{"type":"input_text", "text":"new turn"}]}]),
                "{entrypoint}"
            );
        }
        handle.await.unwrap();
    }
}

#[test]
fn websocket_replay_retains_reasoning_on_models_that_support_it() {
    let response = json!({"id":"reasoned", "output":[
        {"type":"reasoning", "id":"rs_1", "encrypted_content":"opaque", "summary":[]},
        {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"answer"}]}
    ]});
    assert_eq!(
        super::completed_replay_output(&response, "o3").unwrap(),
        vec![
            json!({"type":"reasoning", "id":"rs_1", "encrypted_content":"opaque", "summary":[]}),
            json!({"role":"assistant", "content":[{"type":"output_text", "text":"answer"}]}),
        ]
    );
}

#[test]
fn fast_service_tier_rebuilt_client_payloads_follow_configuration() {
    for model in ["gpt-6-astra", "gpt-6.1-sol", "gpt-5.6-sol"] {
        for tier in [Some(vtcode_config::OpenAIServiceTier::Ultrafast), None] {
            let provider = OpenAIProvider::from_config(
                Some("fixture-key".to_string()),
                None,
                Some(model.to_string()),
                None,
                None,
                None,
                None,
                Some(OpenAIConfig { service_tier: tier, ..Default::default() }),
                None,
            );
            let request = LLMRequest {
                model: model.to_string(),
                messages: vec![crate::provider::Message::user("hello".to_string())].into(),
                ..Default::default()
            };
            let http = provider.convert_to_openai_responses_format(&request).unwrap();
            let chat = provider.convert_to_openai_format(&request).unwrap();
            for warmup in [false, true] {
                let websocket = prepare_websocket_event(&http, None, warmup).unwrap();
                let expected = (tier.is_some() && model != "gpt-5.6-sol").then_some("ultrafast");
                for payload in [&http, &chat, &websocket.event] {
                    assert_eq!(
                        payload.get("service_tier").and_then(Value::as_str),
                        expected,
                        "{model} {tier:?} warmup={warmup}"
                    );
                }
            }
        }
    }
}

#[test]
fn websocket_prompt_cache_ttl_matches_http_streaming_and_non_streaming() {
    let mut provider = OpenAIProvider::new("fixture-key".into());
    provider.prompt_cache_settings.prompt_cache_ttl = Some(vtcode_config::core::prompt_cache::PromptCacheTtl::H12);
    for stream in [false, true] {
        let request = LLMRequest {
            model: vtcode_config::constants::models::openai::GPT_6_LUNA.into(),
            messages: vec![ProviderMessage::user("hello".into())].into(),
            stream,
            ..Default::default()
        };
        let payload = provider.convert_to_openai_responses_format(&request).unwrap();
        let prepared = prepare_websocket_event(&payload, None, false).unwrap();
        assert_eq!(prepared.event.get("prompt_cache_options"), Some(&json!({"ttl": "12h"})));
        assert_eq!(prepared.event.get("prompt_cache_options"), payload.get("prompt_cache_options"));
    }
}
