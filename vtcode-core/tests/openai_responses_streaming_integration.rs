use mockito::{Matcher, Server};
use serde_json::json;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::openai::OpenAIProvider;

#[tokio::test]
async fn mock_responses_api_streaming_includes_prompt_cache_retention() {
    let expect_body = json!({ "prompt_cache_retention": "48h", "stream": true });

    let mut server = Server::new_async().await;

    let mock = server
        .mock("POST", "/responses")
        .match_body(Matcher::PartialJson(expect_body))
        .with_status(200)
        .with_body(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#,
        )
        .create();

    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("48h".to_string());

    let base_url = server.url();
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        Some("gpt-5".to_string()),
        Some(base_url.to_string()),
        Some(pc),
        None,
        None,
        None,
        None,
    );

    let request = LLMRequest {
        messages: vec![Message::user("Hello".to_string())],
        model: "gpt-5".to_string(),
        stream: true,
        ..Default::default()
    };

    let response = LLMProvider::generate(&provider, request)
        .await
        .expect("provider should return");

    mock.assert();
    assert!(response.content.is_some());
}
