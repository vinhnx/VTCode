use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message};
use vtcode_core::llm::providers::openai::OpenAIProvider;

fn mock_openai_base_url(server: &ServerGuard) -> String {
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
async fn mock_responses_api_streaming_includes_prompt_cache_retention() {
    let expect_body = json!({ "prompt_cache_retention": "24h", "stream": true });

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
        .create();

    let mut pc = PromptCachingConfig::default();
    pc.providers.openai.prompt_cache_retention = Some("24h".to_string());

    let base_url = mock_openai_base_url(&server);
    let provider = OpenAIProvider::from_config(
        Some("key".to_string()),
        None,
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
