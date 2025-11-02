//! Integration tests for universal LLM provider system

use vtcode_core::config::constants::models;
use vtcode_core::llm::{
    factory::{LLMFactory, create_provider_for_model},
    provider::{LLMProvider, Message},
    providers::{AnthropicProvider, GeminiProvider, OpenAIProvider},
};

#[test]
fn test_provider_factory() {
    let factory = LLMFactory::new();

    // Test available providers
    let providers = factory.list_providers();
    assert!(providers.contains(&"gemini".to_string()));
    assert!(providers.contains(&"openai".to_string()));
    assert!(providers.contains(&"anthropic".to_string()));

    // Test provider detection from model names
    assert_eq!(
        factory.provider_from_model("gpt-5"),
        Some("openai".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::CLAUDE_SONNET_4_5),
        Some("anthropic".to_string())
    );
    assert_eq!(
        factory.provider_from_model("claude-sonnet-4-20250514"),
        Some("anthropic".to_string())
    );
    assert_eq!(
        factory.provider_from_model("gemini-2.5-flash"),
        Some("gemini".to_string())
    );
}

#[test]
fn test_provider_creation() {
    // Test creating providers
    let gemini = create_provider_for_model("gemini-2.5-flash", "test_key".to_string(), None);
    assert!(gemini.is_ok());

    let openai = create_provider_for_model("gpt-5", "test_key".to_string(), None);
    assert!(openai.is_ok());

    let anthropic =
        create_provider_for_model(models::CLAUDE_SONNET_4_5, "test_key".to_string(), None);
    assert!(anthropic.is_ok());
}

#[test]
fn test_unified_client_creation() {
    // Test creating providers for different models
    let gemini_client = create_provider_for_model(
        "gemini-2.5-flash-lite-preview-06-17",
        "test_key".to_string(),
        None,
    );
    assert!(gemini_client.is_ok());

    let openai_client = create_provider_for_model("gpt-5", "test_key".to_string(), None);
    assert!(openai_client.is_ok());

    let anthropic_client =
        create_provider_for_model(models::CLAUDE_SONNET_4_5, "test_key".to_string(), None);
    assert!(anthropic_client.is_ok());
}

#[test]
fn test_message_creation() {
    // Test message creation helpers
    let user_msg = Message::user("Hello, world!".to_string());
    assert_eq!(user_msg.content.as_text(), "Hello, world!");

    let assistant_msg = Message::assistant("Hi there!".to_string());
    assert_eq!(assistant_msg.content.as_text(), "Hi there!");

    let system_msg = Message::system("You are a helpful assistant".to_string());
    assert_eq!(system_msg.content.as_text(), "You are a helpful assistant");
}

#[test]
#[ignore]
fn test_provider_supported_models() {
    // Test that providers report correct supported models
    let gemini = GeminiProvider::new("test_key".to_string());
    let gemini_models = gemini.supported_models();
    assert_eq!(
        gemini_models,
        vec![
            "gemini-2.5-flash-preview-05-20".to_string(),
            "gemini-2.5-pro".to_string(),
        ]
    );

    let openai = OpenAIProvider::new("test_key".to_string());
    let openai_models = openai.supported_models();
    assert!(openai_models.contains(&"gpt-5".to_string()));
    assert!(openai_models.contains(&"gpt-5-mini".to_string()));

    let anthropic = AnthropicProvider::new("test_key".to_string());
    let anthropic_models = anthropic.supported_models();
    assert!(anthropic_models.contains(&models::CLAUDE_SONNET_4_5.to_string()));
    assert!(anthropic_models.contains(&"claude-sonnet-4-20250514".to_string()));
}

#[test]
fn test_backward_compatibility() {
    use vtcode_core::llm::make_client;
    use vtcode_core::models::ModelId;

    // Test that the old make_client function still works
    use std::str::FromStr;
    let model = ModelId::from_str("gemini-2.5-flash").unwrap();
    let client = make_client("test_key".to_string(), model);

    // Should be able to get model ID
    let model_id = client.model_id();
    assert!(!model_id.is_empty());
}
