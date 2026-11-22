use super::*;

#[tokio::test]
async fn responses_http_completion_controls_final_text_and_success() {
    let final_message = json!([{
        "type": "message",
        "id": "msg_1",
        "status": "completed",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "Final answer", "annotations": []}]
    }]);
    for (output, end_marker, expected) in [
        (Some(final_message), true, Some("Final answer")),
        (Some(json!([])), true, Some("Provisional text")),
        (None, true, None),
        (None, false, None),
    ] {
        let mut body = format!(
            "data: {}\n\n",
            json!({
                "type": "response.output_text.delta",
                "delta": "Provisional text",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 1
            })
        );
        if let Some(output) = output {
            body.push_str(&format!(
                "data: {}\n\n",
                json!({"type": "response.completed", "response": {"id": "resp_terminal", "status": "completed", "output": output}})
            ));
        }
        if end_marker {
            body.push_str("data: [DONE]\n\n");
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(2)
            .mount(&server)
            .await;
        let model = models::openai::GPT_5;
        let mut provider = OpenAIProvider::new_with_client(
            "test-key".to_string(),
            None,
            model.to_string(),
            reqwest::Client::builder().no_proxy().build().unwrap(),
            urls::OPENAI_API_BASE.to_string(),
            TimeoutsConfig::default(),
        );
        provider.responses_url = Arc::from(format!("{}/responses", server.uri()));
        let request = provider::LLMRequest {
            model: model.to_string(),
            messages: vec![provider::Message::user("hello".to_string())].into(),
            ..Default::default()
        };

        let mut legacy = provider.stream_request(request.clone()).await.unwrap();
        assert!(matches!(
            legacy.next().await.unwrap().unwrap(),
            provider::LLMStreamEvent::Token { delta } if delta == "Provisional text"
        ));
        let legacy_completion = legacy.next().await.unwrap().map(|event| match event {
            provider::LLMStreamEvent::Completed { response } => response,
            event => panic!("unexpected legacy event: {event:?}"),
        });
        assert!(legacy.next().await.is_none());

        let mut normalized = provider.stream_normalized_request(request).await.unwrap();
        assert!(matches!(
            normalized.next().await.unwrap().unwrap(),
            NormalizedStreamEvent::TextDelta { delta } if delta == "Provisional text"
        ));
        let normalized_completion = normalized.next().await.unwrap().map(|event| match event {
            NormalizedStreamEvent::Done { response } => response,
            event => panic!("unexpected normalized event: {event:?}"),
        });
        assert!(normalized.next().await.is_none());

        for completion in [legacy_completion, normalized_completion] {
            if let Some(expected) = expected {
                let response = completion.unwrap();
                assert_eq!(response.content.as_deref(), Some(expected));
                assert_eq!(response.request_id.as_deref(), Some("resp_terminal"));
            } else {
                assert!(completion.unwrap_err().to_string().to_lowercase().contains("without a complet"));
            }
        }
    }
}

#[tokio::test]
async fn responses_http_rejects_invalid_utf8_and_oversized_unterminated_event() {
    let server = MockServer::start().await;
    let mut oversized = b"data: ".to_vec();
    oversized.resize(crate::providers::shared::MAX_SSE_TAIL_BYTES + 1, b'a');
    for (model, body) in [("gpt-5", b"data: \xff\n\n".to_vec()), ("gpt-5.6", oversized)] {
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(wiremock::matchers::body_partial_json(json!({"model": model})))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .expect(1)
            .mount(&server)
            .await;
    }

    let provider = OpenAIProvider::new_with_client(
        "test-key".to_string(),
        None,
        "gpt-5".to_string(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        native_openai_mock_base_url(&server),
        TimeoutsConfig::default(),
    );
    let request = |model: &str| provider::LLMRequest {
        model: model.to_string(),
        messages: vec![provider::Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let mut legacy = provider.stream_request(request("gpt-5")).await.unwrap();
    let invalid_utf8 = legacy.next().await.unwrap().unwrap_err();
    assert!(invalid_utf8.to_string().contains("utf-8"), "{invalid_utf8}");

    let mut normalized = provider.stream_normalized_request(request("gpt-5.6")).await.unwrap();
    let oversized = normalized.next().await.unwrap().unwrap_err();
    assert!(oversized.to_string().contains("bytes without an event boundary"), "{oversized}");
}

#[tokio::test]
async fn responses_http_proxy_completion_policy_uses_the_actual_host() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Via proxy\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":1}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/api.openai.com/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let model = models::openai::GPT_5;
    let mut provider = OpenAIProvider::new_with_client(
        "test-key".to_string(),
        None,
        model.to_string(),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        "https://gateway.example/api.openai.com/v1".to_string(),
        TimeoutsConfig::default(),
    );
    provider.responses_url = Arc::from(format!("{}/api.openai.com/responses", server.uri()));
    let request = provider::LLMRequest {
        model: model.to_string(),
        messages: vec![provider::Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let mut stream = provider.stream_normalized_request(request).await.unwrap();
    assert!(matches!(stream.next().await.unwrap().unwrap(),
        NormalizedStreamEvent::TextDelta { delta } if delta == "Via proxy"));
    assert!(matches!(stream.next().await.unwrap().unwrap(),
        NormalizedStreamEvent::Done { response } if response.content.as_deref() == Some("Via proxy")));
}
