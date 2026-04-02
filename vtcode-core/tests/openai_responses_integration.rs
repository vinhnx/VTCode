use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::llm::provider::{AssistantPhase, LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::openai::OpenAIProvider;

fn mock_openai_base_url(server: &ServerGuard) -> String {
    // Keep mock traffic local while preserving the provider's native-OpenAI URL checks.
    format!("{}/api.openai.com", server.url())
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "unknown panic".to_string()
}

async fn start_mockito_server_or_skip() -> Option<ServerGuard> {
    match tokio::spawn(async { Server::new_async().await }).await {
        Ok(server) => Some(server),
        Err(err) if err.is_panic() => {
            let message = panic_message(err.into_panic());
            if message.contains("Operation not permitted")
                || message.contains("PermissionDenied")
                || message.contains("the server is not running")
            {
                return None;
            }
            panic!("mockito server should start: {message}");
        }
        Err(err) => panic!("mockito task should complete: {err}"),
    }
}

#[tokio::test]
async fn mock_responses_api_receives_prompt_cache_retention() {
    // Start mock server
    let expect_body = json!({
        "prompt_cache_retention": "24h",
        "output_types": ["message", "tool_call"]
    });

    let Some(mut server) = start_mockito_server_or_skip().await else {
        return;
    };
    let mock = server
        .mock("POST", "/api.openai.com/responses")
        .match_body(Matcher::PartialJson(expect_body))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create_async()
        .await;
    // Create config with retention
    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("24h".to_string());

    // Create provider referencing mock server base URL
    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        None,
        Some("gpt-5".to_string()),
        Some(base_url.to_string()),
        Some(pc),
        None,
        None,
        None,
        None,
    );

    // Build a simple request that will be sent via Responses API
    let request = LLMRequest {
        messages: vec![Message::user("Hello".to_string())],
        model: "gpt-5".to_string(),
        ..Default::default()
    };

    // Execute
    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    // Verify mock received the expected body at least 1 call
    mock.assert_async().await;
    assert!(response.content.is_some());
}

#[tokio::test]
async fn mock_responses_api_sampling_parameters_structure() {
    let Some(mut server) = start_mockito_server_or_skip().await else {
        return;
    };
    let mock = server
        .mock("POST", "/api.openai.com/responses")
        .match_body(Matcher::Regex(
            r#"(?s)"model":"gpt-5.2".*"max_output_tokens":100.*"sampling_parameters":\{"temperature":0.5"#.to_string(),
        ))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create_async()
        .await;

    // Create provider referencing mock server base URL
    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        None,
        Some("gpt-5.2".to_string()),
        Some(base_url.to_string()),
        None,
        None,
        None,
        None,
        None,
    );

    // Build a request with sampling parameters
    let request = LLMRequest {
        messages: vec![Message::user("Hello".to_string())],
        model: "gpt-5.2".to_string(),
        max_tokens: Some(100),
        temperature: Some(0.5),
        // top_p: Some(0.9), // Keeping strict check simple
        ..Default::default()
    };

    // Execute
    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    // Verify mock received the expected body
    mock.assert_async().await;
    assert!(response.content.is_some());
}

#[tokio::test]
async fn mock_responses_api_top_level_tool_calls() {
    use serde_json::json;
    // Test parsing of top-level tool calls (if that's the format)
    // Based on our code update, we handle both nested and top-level.
    // Let's test top-level injection which we added support for.

    let Some(mut server) = start_mockito_server_or_skip().await else {
        return;
    };

    // Simulate a response with top-level function_call
    let response_body = json!({
        "output": [
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "I will call the tool."}]
            },
            {
                "type": "function_call",
                "id": "call_123",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"location\":\"San Francisco\"}"
                }
            }
        ]
    });

    let mock = server
        .mock("POST", "/api.openai.com/responses")
        .with_status(200)
        .with_body(response_body.to_string())
        .create_async()
        .await;

    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        None,
        Some("gpt-5".to_string()),
        Some(base_url.to_string()),
        None,
        None,
        None,
        None,
        None,
    );

    let request = LLMRequest {
        messages: vec![Message::user("Check weather".to_string())],
        model: "gpt-5".to_string(),
        tools: Some(std::sync::Arc::new(vec![])), // Enable tools to trigger responses path logic if checked
        ..Default::default()
    };

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    mock.assert_async().await;

    // Check if tool call was parsed
    assert!(response.tool_calls.is_some());
    let calls = response.tool_calls.unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_123");
    assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
}

#[tokio::test]
async fn mock_responses_api_minimal_reasoning_effort() {
    use vtcode_core::config::types::ReasoningEffortLevel;
    // Test that minimal reasoning effort is correctly serialized
    let expect_body = json!({
        "model": "gpt-5",
        "reasoning": {
            "effort": "minimal"
        }
    });

    let Some(mut server) = start_mockito_server_or_skip().await else {
        return;
    };
    let mock = server
        .mock("POST", "/api.openai.com/responses")
        .match_body(Matcher::PartialJson(expect_body))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create_async()
        .await;

    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        None,
        Some("gpt-5".to_string()),
        Some(base_url.to_string()),
        None,
        None,
        None,
        None,
        None,
    );

    let request = LLMRequest {
        messages: vec![Message::user("Hello".to_string())],
        model: "gpt-5".to_string(),
        reasoning_effort: Some(ReasoningEffortLevel::Minimal),
        ..Default::default()
    };

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    mock.assert_async().await;
    assert!(response.content.is_some());
}

#[tokio::test]
async fn mock_responses_api_preserves_assistant_phase_history() {
    let Some(mut server) = start_mockito_server_or_skip().await else {
        return;
    };
    let mock = server
        .mock("POST", "/api.openai.com/responses")
        .match_body(Matcher::Regex(
            r#"(?s)"phase":"commentary".*"phase":"final_answer""#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(
            concat!(
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_history\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}]}}\n\n",
                "data: [DONE]\n\n",
            ),
        )
        .create_async()
        .await;

    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        None,
        Some("gpt-5.4".to_string()),
        Some(base_url.to_string()),
        None,
        None,
        None,
        None,
        None,
    );

    let request = LLMRequest {
        messages: vec![
            Message::user("Start".to_string()),
            Message::assistant("Checking prerequisites".to_string())
                .with_phase(Some(AssistantPhase::Commentary)),
            Message::assistant("Ready to answer".to_string())
                .with_phase(Some(AssistantPhase::FinalAnswer)),
            Message::user("Continue".to_string()),
        ],
        model: "gpt-5.4".to_string(),
        ..Default::default()
    };

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    mock.assert_async().await;
    assert!(response.content.is_some());
}
