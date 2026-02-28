use mockito::{Matcher, Server};
use serde_json::json;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::openai::OpenAIProvider;

#[tokio::test]
async fn mock_responses_api_receives_prompt_cache_retention() {
    // Start mock server
    let expect_body = json!({
        "prompt_cache_retention": "24h",
        "output_types": ["message", "tool_call"]
    });

    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/responses")
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
    let base_url = server.url();
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
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
    use serde_json::json;
    // Start mock server
    // We expect sampling_parameters to be nested
    let expect_body = json!({
        "sampling_parameters": {
            "max_output_tokens": 100,
            "temperature": 0.5
        },
        "model": "gpt-5.1"
    });

    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/responses")
        .match_body(Matcher::PartialJson(expect_body))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create_async()
        .await;

    // Create provider referencing mock server base URL
    let base_url = server.url();
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
        Some("gpt-5.1".to_string()),
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
        model: "gpt-5.1".to_string(),
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

    let mut server = Server::new_async().await;

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
        .mock("POST", "/responses")
        .with_status(200)
        .with_body(response_body.to_string())
        .create_async()
        .await;

    let base_url = server.url();
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
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

    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/responses")
        .match_body(Matcher::PartialJson(expect_body))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create_async()
        .await;

    let base_url = server.url();
    let provider = OpenAIProvider::from_config(
        Some("testkey".to_string()),
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
