use super::*;

#[tokio::test]
async fn generate_retries_refusal_once_on_recommended_model_with_credit_token() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| body_has(req, "model", Some("claude-opus-5-5")))
        .respond_with(ResponseTemplate::new(200).set_body_json(refusal_with_recommendation()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            body_has(req, "model", Some("claude-opus-4-8"))
                && body_has(req, "fallback_credit_token", Some("credit-1"))
                && body_has(req, "fallbacks", None)
                && req
                    .headers
                    .get("anthropic-beta")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.split(", ").any(|beta| beta == "fallback-credit-2026-07-01"))
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "retried answer"}],
            "stop_reason": "end_turn"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = credit_retry_provider(&server);
    let response = LLMProvider::generate(
        &provider,
        LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("retried request should succeed");

    assert_eq!(response.content.as_deref(), Some("retried answer"));
    assert_eq!(response.model, "claude-opus-4-8");
    assert!(matches!(response.finish_reason, crate::provider::FinishReason::Stop));
}
#[tokio::test]
async fn generate_resends_without_credit_token_when_token_is_rejected() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| body_has(req, "model", Some("claude-opus-5-5")))
        .respond_with(ResponseTemplate::new(200).set_body_json(refusal_with_recommendation()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            body_has(req, "model", Some("claude-opus-4-8")) && body_has(req, "fallback_credit_token", Some("credit-1"))
        })
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "fallback_credit_token has expired"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            body_has(req, "model", Some("claude-opus-4-8")) && body_has(req, "fallback_credit_token", None)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "uncredited answer"}],
            "stop_reason": "end_turn"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = credit_retry_provider(&server);
    let response = LLMProvider::generate(
        &provider,
        LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("uncredited retry should succeed");

    assert_eq!(response.content.as_deref(), Some("uncredited answer"));
}
#[tokio::test]
async fn generate_retry_without_credit_token_when_thinking_must_be_rewritten() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            body_has(req, "model", Some("claude-opus-5-5")) && body["thinking"]["display"] == "updates"
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(refusal_with_recommendation()))
        .expect(1)
        .mount(&server)
        .await;
    // Opus 4.8 rejects `display: "updates"`, so the retry's thinking
    // differs from the refused request and the token could never match:
    // one request, sent without the token.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            body_has(req, "model", Some("claude-opus-4-8"))
                && body_has(req, "fallback_credit_token", None)
                && body["thinking"].get("display").is_none()
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "uncredited answer"}],
            "stop_reason": "end_turn"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let response = LLMProvider::generate(
        &provider,
        LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("uncredited retry should succeed");

    assert_eq!(response.content.as_deref(), Some("uncredited answer"));
    assert_eq!(response.model, "claude-opus-4-8");
}
#[tokio::test]
async fn generate_keeps_original_refusal_when_retry_fails() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| body_has(req, "model", Some("claude-opus-5-5")))
        .respond_with(ResponseTemplate::new(200).set_body_json(refusal_with_recommendation()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| body_has(req, "model", Some("claude-opus-4-8")))
        .respond_with(ResponseTemplate::new(529).set_body_json(json!({
            "type": "error",
            "error": {"type": "overloaded_error", "message": "Overloaded"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let response = LLMProvider::generate(
        &provider,
        LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("original refusal is returned");

    assert!(matches!(response.finish_reason, crate::provider::FinishReason::Refusal));
    assert_eq!(response.model, models::anthropic::CLAUDE_OPUS_5_5);
}
#[tokio::test]
async fn stream_replaces_retryable_refusal_with_retry_stream() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let refused = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-5-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_sequence\":null,\"stop_details\":{\"type\":\"refusal\",\"category\":\"cyber\",\"fallback_credit_token\":\"credit-1\",\"recommended_model\":\"claude-opus-4-8\"}},\"usage\":{\"output_tokens\":0}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let retried = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-4-8\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"retried\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| body_has(req, "model", Some("claude-opus-5-5")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(refused, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(|req: &wiremock::Request| {
            body_has(req, "model", Some("claude-opus-4-8")) && body_has(req, "fallback_credit_token", Some("credit-1"))
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(retried, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let provider = credit_retry_provider(&server);
    let mut stream = LLMProvider::stream(
        &provider,
        LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("stream request should succeed");

    let mut completed = Vec::new();
    while let Some(event) = stream.next().await {
        if let LLMStreamEvent::Completed { response } = event.expect("stream event") {
            completed.push(*response);
        }
    }

    assert_eq!(completed.len(), 1, "the refused attempt is replaced, not surfaced");
    assert_eq!(completed[0].content.as_deref(), Some("retried"));
    assert!(matches!(completed[0].finish_reason, crate::provider::FinishReason::Stop));
}
