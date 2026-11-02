use super::*;

#[tokio::test]
async fn generate_sends_inline_compaction_and_parses_compaction_response() {
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("anthropic-beta", "compact-2026-01-12"))
        .and(body_partial_json(json!({
            "context_management": {
                "edits": [{"type": "compact_20260112"}]
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "compaction", "content": "opaque summary"}],
            "stop_reason": "compaction"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::CLAUDE_SONNET_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let response = LLMProvider::generate(
        &provider,
        LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("compact this".to_string())].into(),
            context_management: Some(json!({
                "edits": [{
                    "type": "compact_20260112",
                    "trigger": {"type": "input_tokens", "value": 50_000},
                    "pause_after_compaction": true
                }]
            })),
            ..Default::default()
        },
    )
    .await
    .expect("inline compaction request should succeed");

    assert!(matches!(response.finish_reason, crate::provider::FinishReason::Pause));
    assert_eq!(response.compaction.as_deref(), Some("opaque summary"));
}
#[tokio::test]
async fn stream_preserves_anthropic_compaction_block_and_iteration_usage() {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"compaction\",\"content\":null,\"signature\":\"signed-summary\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"compaction_delta\",\"content\":null,\"encrypted_content\":\"opaque-extension\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"compaction_delta\",\"content\":\"opaque \"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"compaction_delta\",\"content\":\"summary\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"compaction\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":null,\"output_tokens\":0,\"iterations\":[{\"type\":\"compaction\",\"input_tokens\":50,\"output_tokens\":5},{\"type\":\"message\",\"model\":null,\"input_tokens\":10,\"output_tokens\":0},{\"type\":\"advisor_message\",\"model\":\"claude-opus-5\",\"input_tokens\":7,\"output_tokens\":2}]}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(json!({"stream": true})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::CLAUDE_SONNET_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let mut stream = LLMProvider::stream(
        &provider,
        LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("continue after compaction".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("stream request should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            LLMStreamEvent::Completed { response } => completed = Some(*response),
            LLMStreamEvent::Token { .. }
            | LLMStreamEvent::Reasoning { .. }
            | LLMStreamEvent::ReasoningSignature { .. }
            | LLMStreamEvent::ReasoningStage { .. } => {}
        }
    }

    let response = completed.expect("completed stream response");
    assert_eq!(response.compaction.as_deref(), Some("opaque summary"));
    let details = response.reasoning_details.expect("compaction detail");
    assert_eq!(details.len(), 1);
    let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized compaction detail");
    assert_eq!(detail["content"], "opaque summary");
    assert_eq!(detail["signature"], "signed-summary");
    assert_eq!(detail["encrypted_content"], "opaque-extension");

    let usage = response.usage.expect("usage");
    let totals = usage.billable_totals();
    assert_eq!(totals.prompt_tokens, 67);
    assert_eq!(totals.completion_tokens, 7);
}
#[tokio::test]
async fn stream_carries_refusal_stop_details_from_message_delta() {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-5\",\"stop_reason\":null,\"stop_sequence\":null,\"stop_details\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_sequence\":null,\"stop_details\":{\"type\":\"refusal\",\"category\":\"cyber\",\"explanation\":\"declined\",\"fallback_credit_token\":\"credit-1\",\"fallback_has_prefill_claim\":true}},\"usage\":{\"output_tokens\":0}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(json!({"stream": true})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::CLAUDE_SONNET_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let mut stream = LLMProvider::stream(
        &provider,
        LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("stream request should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        if let LLMStreamEvent::Completed { response } = event.expect("stream event") {
            completed = Some(*response);
        }
    }

    let response = completed.expect("completed stream response");
    assert!(matches!(response.finish_reason, crate::provider::FinishReason::Refusal));
    let details = response.reasoning_details.expect("stop_details detail");
    assert_eq!(details.len(), 1);
    let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized stop_details");
    assert_eq!(detail["type"], "stop_details");
    assert_eq!(detail["category"], "cyber");
    assert_eq!(detail["explanation"], "declined");
    assert_eq!(detail["fallback_credit_token"], "credit-1");
    assert_eq!(detail["fallback_has_prefill_claim"], true);
}
#[tokio::test]
async fn stream_records_interleaved_block_order_for_replay() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-opus-5-5\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-1\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Reading \"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"the parser.\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Checking entry.\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-2\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/parser.rs\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":12}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        model.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let mut stream = LLMProvider::stream(
        &provider,
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("fix the parser".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("stream request should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        if let LLMStreamEvent::Completed { response } = event.expect("stream event") {
            completed = Some(*response);
        }
    }
    let response = completed.expect("completed stream response");
    let tool_calls = response.tool_calls.clone().expect("tool calls");
    let details = response
        .reasoning_details
        .clone()
        .map(|details| details.into_iter().map(serde_json::Value::String).collect());
    let assistant = Message::assistant_with_tools_and_reasoning(
        response.content.clone().expect("text content"),
        tool_calls,
        details,
    );

    let replay = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::user("fix the parser".to_string()),
            assistant,
            Message::tool_response("toolu_1".to_string(), "fn parse() {}".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let payload = provider.convert_to_anthropic_format(&replay).expect("payload conversion");
    assert_eq!(
        payload["messages"][1]["content"],
        json!([
            { "type": "thinking", "thinking": "", "signature": "sig-1" },
            { "type": "text", "text": "Reading the parser." },
            { "type": "thinking", "thinking": "Checking entry.", "signature": "sig-2" },
            { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "src/parser.rs" } }
        ])
    );
}
#[tokio::test]
async fn stream_mid_output_fallback_drops_declined_thinking_and_tool_use() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-fable-5-1\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Refused model reasoning.\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-1\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking. \"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_declined\",\"name\":\"read_file\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"src/\"}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"fallback\",\"from\":{\"model\":\"claude-fable-5-1\"},\"to\":{\"model\":\"claude-opus-4-8\"}}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":4,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":4,\"delta\":{\"type\":\"text_delta\",\"text\":\"Here is the answer.\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":4}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":12}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;

    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        model.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    let mut stream = LLMProvider::stream(
        &provider,
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("fix the parser".to_string())].into(),
            ..Default::default()
        },
    )
    .await
    .expect("stream request should succeed");

    let mut completed = None;
    while let Some(event) = stream.next().await {
        if let LLMStreamEvent::Completed { response } = event.expect("stream event") {
            completed = Some(*response);
        }
    }
    let response = completed.expect("completed stream response");
    assert!(response.tool_calls.is_none(), "declined tool_use must not run");
    assert_eq!(response.content.as_deref(), Some("Checking. Here is the answer."));
    let raw_details = response.reasoning_details.clone().expect("details");
    let details: Vec<serde_json::Value> = raw_details
        .iter()
        .map(|detail| serde_json::from_str(detail).expect("detail json"))
        .collect();
    assert!(details.iter().all(|detail| detail["type"] != "thinking"));
    assert!(details.iter().any(|detail| {
        detail["type"] == "fallback"
            && detail["from"]["model"] == "claude-fable-5-1"
            && detail["to"]["model"] == "claude-opus-4-8"
    }));

    let assistant = Message::assistant(response.content.clone().expect("text content"))
        .with_reasoning_details(Some(raw_details.into_iter().map(serde_json::Value::String).collect()));
    let replay = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::user("fix the parser".to_string()),
            assistant,
            Message::user("thanks".to_string()),
        ]
        .into(),
        ..Default::default()
    };
    let payload = provider.convert_to_anthropic_format(&replay).expect("payload conversion");
    assert_eq!(
        payload["messages"][1]["content"],
        json!([
            { "type": "text", "text": "Checking. " },
            { "type": "text", "text": "Here is the answer." }
        ])
    );
}
