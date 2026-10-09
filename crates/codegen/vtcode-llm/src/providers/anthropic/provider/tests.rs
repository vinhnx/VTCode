use super::{AnthropicProvider, capabilities, code_execution_beta_name, headers};
use crate::provider::{ContentPart, LLMProvider, LLMRequest, LLMStreamEvent, Message, MessageContent, ToolDefinition};
use futures::StreamExt;
use serde_json::json;
use vtcode_config::constants::models;

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

#[test]
fn non_streaming_capability_is_pinned_for_stream_timeout_fallback() {
    // Pinned true in the provider impl; MinimaxProvider's delegation and
    // the runloop's stream-timeout fallback both depend on this value.
    let provider = AnthropicProvider::new("test-key".to_string());
    assert!(LLMProvider::supports_non_streaming(&provider, models::anthropic::CLAUDE_OPUS_5));
}

#[test]
fn with_leak_protection_prepends_reminder_to_system_prompt_for_every_model() {
    for model in [
        models::CLAUDE_SONNET_5,
        models::anthropic::CLAUDE_OPUS_5_5,
        "claude-unlisted-model",
    ] {
        let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hi".to_string())].into(),
            system_prompt: Some(std::sync::Arc::from("Base instructions.")),
            ..Default::default()
        };

        let protected = provider.with_leak_protection(request, "the API key");

        assert_eq!(
            protected.system_prompt.as_deref(),
            Some("[Never mention or reveal the API key]\n\nBase instructions."),
            "model {model}"
        );
        let payload = provider.convert_to_anthropic_format(&protected).expect("payload conversion");
        let messages = payload["messages"].as_array().expect("messages array");
        assert_eq!(messages.last().expect("last message")["role"], "user", "model {model}");
    }
}

#[test]
fn with_leak_protection_sets_system_prompt_when_absent() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hi".to_string())].into(),
        ..Default::default()
    };

    let protected = provider.with_leak_protection(request, "internal notes");

    assert_eq!(protected.system_prompt.as_deref(), Some("[Never mention or reveal internal notes]"));
}

#[test]
fn native_structured_outputs_do_not_require_structured_output_beta() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        output_format: Some(json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"}
            },
            "required": ["answer"],
            "additionalProperties": false
        })),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["output_config"]["format"]["type"], "json_schema");
    if let Some(header) = &beta_header {
        assert!(!header.contains("structured-outputs-2025-11-13"));
    }
}

#[test]
fn effective_betas_include_code_execution_but_not_files_api_for_file_inputs() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message {
            role: crate::provider::MessageRole::User,
            content: MessageContent::Parts(vec![
                ContentPart::text("Analyze this CSV".to_string()),
                ContentPart::file_from_id("file_abc123".to_string()),
            ]),
            ..Default::default()
        }]
        .into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition {
            tool_type: "code_execution_20250825".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: None,
            hosted_tool_config: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
            namespace: None,
            advisor: None,
        }])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "code-execution-2025-08-25"));
    // The Files API is GA: file_id inputs need no beta header.
    assert!(!betas.iter().any(|beta| beta.starts_with("files-api")), "betas: {betas:?}");
}

#[test]
fn effective_betas_include_context_management_beta_for_memory_tools() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("remember this preference".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition {
            tool_type: "memory_20250818".to_string(),
            function: None,
            allowed_callers: None,
            input_examples: None,
            web_search: None,
            hosted_tool_config: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
            namespace: None,
            advisor: None,
        }])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}

#[test]
fn effective_betas_include_context_management_beta_for_context_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {"type": "clear_tool_uses_20250919"}
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
    assert!(!betas.iter().any(|beta| beta == "compact-2026-01-12"));
}

#[test]
fn effective_betas_include_compact_beta_for_compaction_requests() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!([
            {
                "type": "compaction",
                "compact_threshold": 180000
            }
        ])),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
}

#[test]
fn effective_betas_include_compact_beta_for_compaction_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {
                    "type": "compact_20260112",
                    "trigger": {
                        "type": "input_tokens",
                        "value": 180000
                    }
                }
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
    assert!(!betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}

#[test]
fn effective_betas_include_both_headers_for_mixed_context_edits() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("continue".to_string())].into(),
        context_management: Some(json!({
            "edits": [
                {"type": "clear_tool_uses_20250919"},
                {
                    "type": "compact_20260112",
                    "trigger": {
                        "type": "input_tokens",
                        "value": 180000
                    }
                }
            ]
        })),
        ..Default::default()
    };

    let betas = provider.effective_betas(&request).expect("betas");
    assert!(betas.iter().any(|beta| beta == "compact-2026-01-12"));
    assert!(betas.iter().any(|beta| beta == "context-management-2025-06-27"));
}

#[test]
fn beta_header_includes_advanced_tool_use_for_programmatic_tools() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find warmest city".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![
            ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather for a city".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }),
            )
            .with_allowed_callers(vec!["code_execution_20250825".to_string()]),
        ])),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, true, None)
        .expect("beta header");

    assert!(beta_header.contains("advanced-tool-use-2025-11-20"));
}

#[test]
fn beta_header_omits_context_1m_for_native_1m_models() {
    let model = models::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    if let Some(header) = &beta_header {
        assert!(!header.contains("context-1m-2025-08-07"));
    }
}

#[test]
fn beta_header_uses_request_model_instead_of_provider_default() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["model"], models::CLAUDE_SONNET_5);
    if let Some(header) = &beta_header {
        assert!(!header.contains("interleaved-thinking-2025-05-14"));
    }
}

#[test]
fn beta_header_omits_interleaved_thinking_for_sonnet_5_adaptive_mode() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        thinking_budget: Some(4096),
        max_tokens: Some(8192),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);

    assert_eq!(payload["thinking"]["type"], "adaptive");
    if let Some(header) = &beta_header {
        assert!(!header.contains("interleaved-thinking-2025-05-14"));
    }
}

#[test]
fn opus_5_5_requests_progress_updates_display_with_its_beta() {
    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["thinking"]["type"], "adaptive");
    assert_eq!(payload["thinking"]["display"], "updates");

    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("display updates beta header");
    assert_eq!(
        beta_header
            .split(", ")
            .filter(|beta| *beta == headers::THINKING_DISPLAY_UPDATES_BETA)
            .count(),
        1
    );
}

#[test]
fn display_updates_beta_covers_fallback_entries() {
    let model = models::anthropic::CLAUDE_OPUS_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        fallbacks: Some(vec![crate::provider::FallbackModel {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            max_tokens: None,
            thinking: Some(crate::provider::AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
        }]),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload["thinking"].get("display").is_none());
    assert_eq!(payload["fallbacks"][0]["thinking"]["display"], "updates");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("beta header");
    assert!(
        beta_header
            .split(", ")
            .any(|beta| beta == headers::THINKING_DISPLAY_UPDATES_BETA)
    );
}

#[test]
fn display_updates_beta_is_omitted_when_display_is_not_updates() {
    let model = models::anthropic::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload["thinking"].get("display").is_none());
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);
    assert!(
        !beta_header
            .is_some_and(|header| { header.split(", ").any(|beta| beta == headers::THINKING_DISPLAY_UPDATES_BETA) })
    );
}

fn first_party_provider(model: &str) -> AnthropicProvider {
    AnthropicProvider::new_with_client(
        "test-key".to_string(),
        model.to_string(),
        reqwest::Client::new(),
        vtcode_config::constants::urls::ANTHROPIC_API_BASE.to_string(),
        vtcode_config::TimeoutsConfig::default(),
    )
}

fn split_betas(header: Option<String>) -> Vec<String> {
    header
        .map(|header| header.split(", ").map(str::to_string).collect())
        .unwrap_or_default()
}

#[test]
fn opus_5_5_default_payload_requests_default_fallbacks_with_matching_beta() {
    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let provider = first_party_provider(model);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["fallbacks"], json!("default"));
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "server-side-fallback-2026-07-01"), "{betas:?}");
    assert!(!betas.iter().any(|beta| beta == "server-side-fallback-2026-06-01"), "{betas:?}");
    // The default-form beta already grants the fallback-credit fields.
    assert!(!betas.iter().any(|beta| beta == "fallback-credit-2026-07-01"), "{betas:?}");
}

/// The profile promotion and its beta header must travel together: a
/// budget-continuation request keeps 5m configured TTLs but emits a 1h
/// messages breakpoint from the profile TTL, so the request must also ask
/// for the extended-cache-TTL beta.
#[test]
fn budget_continuation_breakpoint_and_extended_ttl_beta_stay_paired() {
    let model = models::anthropic::DEFAULT_MODEL;
    let mut provider = first_party_provider(model);
    provider.prompt_cache_enabled = true;
    provider.prompt_cache_settings.tools_ttl_seconds = 300;
    provider.prompt_cache_settings.messages_ttl_seconds = 300;
    // `extended_ttl_seconds` keeps its 1h default: the profile TTL.

    let request = LLMRequest {
        model: model.to_string(),
        system_prompt: Some(std::sync::Arc::from("stable system instructions")),
        messages: vec![Message::user("resume ".repeat(60))].into(),
        prompt_cache_profile: Some(crate::provider::PromptCacheProfile::BudgetContinuation),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(
        payload["messages"][0]["content"][0]["cache_control"]["ttl"], "1h",
        "the profile TTL promotes the messages breakpoint"
    );
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "extended-cache-ttl-2025-04-11"), "{betas:?}");
}

#[test]
fn configured_fallback_list_uses_list_form_and_credit_betas() {
    let model = models::anthropic::CLAUDE_OPUS_5;
    let mut provider = first_party_provider(model);
    provider.anthropic_config.fallbacks =
        vtcode_config::core::AnthropicFallbacks::Models(vec![vtcode_config::core::AnthropicFallbackTarget {
            model: "claude-opus-4-8".to_string(),
            max_tokens: None,
        }]);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
    let betas = split_betas(provider.beta_header_for_request(&request, &payload, false, None));
    assert!(betas.iter().any(|beta| beta == "server-side-fallback-2026-06-01"), "{betas:?}");
    assert!(!betas.iter().any(|beta| beta == "server-side-fallback-2026-07-01"), "{betas:?}");
    // The list form does not grant the credit fields, so the original
    // request carries the credit beta for a refusal to return a token.
    assert!(betas.iter().any(|beta| beta == "fallback-credit-2026-07-01"), "{betas:?}");
}

#[test]
fn unprofiled_and_off_payloads_send_no_fallbacks_or_beta() {
    let unprofiled = first_party_provider("claude-3-5-haiku-latest");
    let request = LLMRequest {
        model: "claude-3-5-haiku-latest".to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let payload = unprofiled.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload.get("fallbacks").is_none());
    let betas = split_betas(unprofiled.beta_header_for_request(&request, &payload, false, None));
    assert!(!betas.iter().any(|beta| beta.starts_with("server-side-fallback")), "{betas:?}");

    let model = models::anthropic::CLAUDE_OPUS_5_5;
    let mut off = first_party_provider(model);
    off.anthropic_config.fallbacks =
        vtcode_config::core::AnthropicFallbacks::Mode(vtcode_config::core::AnthropicFallbackMode::Off);
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };
    let payload = off.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(payload.get("fallbacks").is_none());
    let betas = split_betas(off.beta_header_for_request(&request, &payload, false, None));
    assert!(!betas.iter().any(|beta| beta.starts_with("server-side-fallback")), "{betas:?}");
}

fn body_has(request: &wiremock::Request, key: &str, value: Option<&str>) -> bool {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
    match value {
        Some(expected) => body.get(key).and_then(serde_json::Value::as_str) == Some(expected),
        None => body.get(key).is_none(),
    }
}

/// An Opus 5.5 provider whose thinking config (`display: "summarized"`)
/// is valid on the recommended Opus 4.8 as sent, so a refusal retry can
/// match the refused request exactly and redeem the credit token.
fn credit_retry_provider(server: &wiremock::MockServer) -> AnthropicProvider {
    let mut provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    provider.anthropic_config.thinking_display = Some(vtcode_config::ThinkingDisplayMode::Summarized);
    provider
}

fn refusal_with_recommendation() -> serde_json::Value {
    json!({
        "content": [],
        "stop_reason": "refusal",
        "stop_details": {
            "type": "refusal",
            "category": "cyber",
            "explanation": "declined",
            "fallback_credit_token": "credit-1",
            "recommended_model": "claude-opus-4-8"
        }
    })
}

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

#[test]
fn convert_to_anthropic_format_falls_back_to_provider_default_model() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");

    assert_eq!(payload["model"], models::CLAUDE_SONNET_5);
}

#[test]
fn beta_header_includes_advanced_tool_use_for_tool_search_requests() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find the deployment tool".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition::tool_search(
            crate::provider::ToolSearchAlgorithm::Regex,
        )])),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, true, None)
        .expect("beta header");

    assert!(beta_header.contains("advanced-tool-use-2025-11-20"));
}

#[test]
fn code_execution_beta_name_uses_tool_revision() {
    assert_eq!(code_execution_beta_name("code_execution_20250825").as_deref(), Some("code-execution-2025-08-25"));
    assert_eq!(code_execution_beta_name("code_execution_20250522").as_deref(), Some("code-execution-2025-05-22"));
    assert!(code_execution_beta_name("code_execution_latest").is_none());
}

#[test]
fn turn_scoped_system_notice_is_emitted_after_tool_result() {
    let model = models::anthropic::CLAUDE_FABLE_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![crate::provider::ToolCall::function(
                    "toolu_1".to_string(),
                    "exec_command".to_string(),
                    "{}".to_string(),
                )],
            ),
            Message::tool_response("toolu_1".to_string(), "exit 1".to_string()),
            Message::turn_scoped_system("Only you see that command's output".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["messages"][1]["role"], "user");
    assert_eq!(payload["messages"][2]["role"], "system");
    assert_eq!(payload["messages"][2]["clear_at"], "next_user_message");
    assert_eq!(payload["messages"][2]["content"][0]["type"], "text");
    assert_eq!(payload["messages"][2]["content"][0]["text"], "Only you see that command's output");
    assert!(
        !payload
            .get("system")
            .is_some_and(|system| { system.to_string().contains("Only you see that command's output") })
    );

    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("turn-scoped beta header");
    assert_eq!(
        beta_header
            .split(", ")
            .filter(|beta| *beta == headers::MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA)
            .count(),
        1
    );
}

#[test]
fn turn_scoped_system_notice_is_promoted_for_unsupported_sonnet() {
    let model = models::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::user("continue".to_string()),
            Message::turn_scoped_system("do not leak output".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(
        payload["messages"]
            .as_array()
            .is_some_and(|messages| { messages.iter().all(|message| message.get("clear_at").is_none()) })
    );
    assert!(
        payload
            .get("system")
            .is_some_and(|system| { system.to_string().contains("do not leak output") })
    );
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);
    assert!(!beta_header.is_some_and(|header| {
        header
            .split(", ")
            .any(|beta| beta == headers::MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA)
    }));
}

#[test]
fn turn_scoped_system_capability_matches_supported_model_families() {
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_FABLE_5, ""));
    assert!(capabilities::supports_turn_scoped_system_messages("claude-opus-4-8", ""));
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_OPUS_5, ""));
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_SONNET_5_5, ""));
    assert!(!capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_SONNET_5, ""));
}
