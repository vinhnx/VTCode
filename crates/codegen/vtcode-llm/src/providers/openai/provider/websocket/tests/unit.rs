use super::*;

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
