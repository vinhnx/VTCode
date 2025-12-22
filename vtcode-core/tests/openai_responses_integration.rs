use mockito::{Matcher, Server};
use serde_json::json;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::openai::OpenAIProvider;

#[tokio::test]
async fn mock_responses_api_receives_prompt_cache_retention() {
    // Start mock server
    let expect_body = json!({ "prompt_cache_retention": "24h" });

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
