use super::*;
use crate::provider::AnthropicRequestOverrides;
use vtcode_config::{AnthropicSpeed, core::AnthropicConfig};

#[derive(Clone, Default)]
struct ServedSpeeds(std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>);

impl tracing::Subscriber for ServedSpeeds {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().ends_with("anthropic::speed") {
            return;
        }
        #[derive(Default)]
        struct SpeedVisitor(Option<String>);
        impl tracing::field::Visit for SpeedVisitor {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "served_speed" {
                    self.0 = Some(value.to_owned());
                }
            }
        }
        let mut visitor = SpeedVisitor::default();
        event.record(&mut visitor);
        self.0.lock().unwrap().push(visitor.0);
    }
}

fn native_provider(model: &str, speed: Option<AnthropicSpeed>) -> AnthropicProvider {
    AnthropicProvider::from_config(
        Some("fixture-key".to_owned()),
        Some(model.to_owned()),
        Some("https://api.anthropic.com/v1".to_owned()),
        None,
        None,
        Some(AnthropicConfig { speed, ..Default::default() }),
        None,
    )
}

fn request(model: &str, speed: Option<AnthropicSpeed>) -> LLMRequest {
    LLMRequest {
        model: model.to_owned(),
        messages: vec![Message::user("hello".to_owned())].into(),
        anthropic_request_overrides: Some(AnthropicRequestOverrides { speed, ..Default::default() }),
        ..Default::default()
    }
}

#[test]
fn anthropic_speed_request_precedence_preview_headers_and_dated_models() {
    for (model, beta) in [
        ("claude-opus-5-5", false),
        ("claude-opus-5-5-20261016", false),
        ("claude-opus-5", true),
        ("claude-opus-4-8-20260416", true),
    ] {
        let provider = native_provider(model, Some(AnthropicSpeed::Fast));
        for (explicit, expected) in [
            (None, "fast"),
            (Some(AnthropicSpeed::Standard), "standard"),
            (Some(AnthropicSpeed::Fast), "fast"),
        ] {
            for stream in [false, true] {
                let mut request = request(model, explicit);
                request.stream = stream;
                request.betas = Some(vec!["test-beta".to_owned(), "test-beta".to_owned()]);
                let body = provider.convert_to_anthropic_format(&request).unwrap();
                assert_eq!(body["speed"], expected);
                assert_eq!(body["stream"], stream);
                let header = provider
                    .beta_header_for_request(&request, &body, false, request.betas.as_deref())
                    .unwrap();
                assert_eq!(
                    header
                        .split(',')
                        .map(str::trim)
                        .filter(|name| *name == "fast-mode-2026-02-01")
                        .count(),
                    usize::from(beta)
                );
                assert_eq!(header.split(',').map(str::trim).filter(|name| *name == "test-beta").count(), 1);
                request.betas.as_mut().unwrap().push("fast-mode-2026-02-01".to_owned());
                let header = provider
                    .beta_header_for_request(&request, &body, false, request.betas.as_deref())
                    .unwrap();
                assert_eq!(
                    header
                        .split(',')
                        .map(str::trim)
                        .filter(|name| *name == "fast-mode-2026-02-01")
                        .count(),
                    1
                );
            }
        }
        let provider = native_provider(model, None);
        let request = request(model, None);
        let body = provider.convert_to_anthropic_format(&request).unwrap();
        assert!(body.get("speed").is_none());
        assert!(
            !provider
                .beta_header_for_request(&request, &body, false, None)
                .unwrap_or_default()
                .contains("fast-mode")
        );
    }
}

#[tokio::test]
async fn anthropic_speed_dormant_preferences_and_explicit_rejection_before_dispatch() {
    for (model, endpoint, custom) in [
        ("claude-sonnet-5-5", "https://api.anthropic.com/v1", false),
        ("anthropic/claude-opus-5", "https://api.anthropic.com/v1", false),
        ("claude-opus-5", "https://api.anthropic.com/v1/compat", false),
        ("claude-opus-5", "https://gateway.example/v1", false),
        ("claude-opus-5", "https://api.anthropic.com/v1", true),
    ] {
        let mut provider = native_provider(model, Some(AnthropicSpeed::Fast));
        provider.base_url = endpoint.to_owned();
        if custom {
            provider = provider.with_custom_auth(None);
        }
        let body = provider.convert_to_anthropic_format(&request(model, None)).unwrap();
        assert!(body.get("speed").is_none(), "{model} {endpoint} custom={custom}");
        assert_eq!(provider.anthropic_config.speed, Some(AnthropicSpeed::Fast));
        for stream in [false, true] {
            let request = request(model, Some(AnthropicSpeed::Fast));
            let error = if stream {
                LLMProvider::stream(&provider, request).await.err().unwrap()
            } else {
                LLMProvider::generate(&provider, request).await.unwrap_err()
            };
            assert!(matches!(error, crate::provider::LLMError::InvalidRequest { .. }));
        }
    }
}

#[tokio::test]
async fn anthropic_speed_http_errors_preserve_fast_payload_and_retry_metadata() {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for status in [429, 529] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(json!({"speed": "fast"})))
            .respond_with(ResponseTemplate::new(status).insert_header("retry-after", "7").set_body_json(
                json!({"type":"error", "error":{"type":"overloaded_error","message":"capacity unavailable"}}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        let model = "claude-opus-5-5";
        let mut provider = native_provider(model, Some(AnthropicSpeed::Fast));
        let request = request(model, None);
        let body = provider.convert_to_anthropic_format(&request).unwrap();
        // Redirect transport only after native request preparation. No live API calls.
        provider.base_url = format!("{}/v1", server.uri());
        provider.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
        for _ in 0..2 {
            let error = provider
                .generate_with_body(&request, &body, model.to_owned())
                .await
                .unwrap_err();
            assert_eq!(error.metadata().unwrap().retry_after.as_deref(), Some("7"));
            assert_eq!(error.metadata().unwrap().status, Some(status));
            assert!(vtcode_commons::ErrorCategory::from(&error).is_retryable());
        }
        assert_eq!(provider.anthropic_config.speed, Some(AnthropicSpeed::Fast));
    }
}

#[tokio::test]
async fn anthropic_speed_refusal_retry_omits_dormant_speed_and_rejects_explicit_fast() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"content": [], "stop_reason":"end_turn"})))
        .expect(1)
        .mount(&server)
        .await;
    let mut provider = native_provider("claude-opus-5-5", Some(AnthropicSpeed::Fast));
    let plan = crate::providers::anthropic::fallback_retry::RefusalRetryPlan {
        model: "claude-sonnet-5".to_owned(),
        credit_token: "credit".to_owned(),
    };
    let request = request("claude-opus-5-5", None);
    let body = provider.convert_to_anthropic_format(&request).unwrap();
    provider.base_url = format!("{}/v1", server.uri());
    provider.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert!(provider.send_refusal_retry(&request, &body, &plan).await.is_some());
    let sent = server.received_requests().await.unwrap();
    let sent: serde_json::Value = sent[0].body_json().unwrap();
    assert_eq!(sent["model"], "claude-sonnet-5");
    assert!(sent.get("speed").is_none());
    let mut explicit = request;
    explicit.anthropic_request_overrides.as_mut().unwrap().speed = Some(AnthropicSpeed::Fast);
    assert!(provider.send_refusal_retry(&explicit, &body, &plan).await.is_none());
}

#[tokio::test]
async fn anthropic_speed_logs_only_server_evidence_at_completion_for_both_transports() {
    use tracing::instrument::WithSubscriber;
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for served in [Some("fast"), Some("standard"), None] {
        for streaming in [false, true] {
            let server = MockServer::start().await;
            let mut usage = json!({"input_tokens": 11, "output_tokens": 3});
            if let Some(speed) = served {
                usage["speed"] = json!(speed);
            }
            let response = if streaming {
                let start = json!({"type":"message_start", "message":{"id":"msg_speed","role":"assistant","content":[],"model":"claude-opus-5-5","usage":usage}});
                let end =
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}});
                ResponseTemplate::new(200).set_body_raw(
                    format!("data: {start}\n\ndata: {end}\n\ndata: {{\"type\":\"message_stop\"}}\n\n"),
                    "text/event-stream",
                )
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"content":[],"stop_reason":"end_turn","usage":usage}))
            };
            Mock::given(method("POST"))
                .and(body_partial_json(json!({"speed":"fast", "stream":streaming})))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
            let mut provider = native_provider("claude-opus-5-5", Some(AnthropicSpeed::Fast));
            let mut request = request("claude-opus-5-5", None);
            request.stream = streaming;
            let body = provider.convert_to_anthropic_format(&request).unwrap();
            provider.base_url = format!("{}/v1", server.uri());
            provider.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
            let observed = ServedSpeeds::default();
            async {
                if streaming {
                    let http = provider.send_request(&request, &body).await.unwrap();
                    let stream = crate::providers::anthropic::stream_decoder::create_stream(
                        http.response,
                        request.model.clone(),
                        None,
                        None,
                    );
                    assert!(observed.0.lock().unwrap().is_empty());
                    let events: Vec<_> = stream.collect().await;
                    assert!(events.iter().all(Result::is_ok));
                    assert!(matches!(events.last(), Some(Ok(LLMStreamEvent::Completed { .. }))));
                } else {
                    provider
                        .generate_with_body(&request, &body, request.model.clone())
                        .await
                        .unwrap();
                }
            }
            .with_subscriber(observed.clone())
            .await;
            assert_eq!(*observed.0.lock().unwrap(), vec![served.map(str::to_owned)]);
        }
    }
}
