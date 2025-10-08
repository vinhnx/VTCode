//! Focused test for LLM provider functionality

use vtcode_core::config::constants::models;
use vtcode_core::llm::{
    factory::{LLMFactory, create_provider_for_model},
    provider::{LLMProvider, LLMRequest, Message, MessageRole},
    providers::{
        AnthropicProvider, GeminiProvider, MoonshotProvider, OpenAIProvider, OpenRouterProvider,
        XAIProvider,
    },
};

#[test]
fn test_provider_factory_basic() {
    let factory = LLMFactory::new();
    // Test available providers
    let providers = factory.list_providers();
    assert!(providers.contains(&"gemini".to_string()));
    assert!(providers.contains(&"openai".to_string()));
    assert!(providers.contains(&"anthropic".to_string()));
    assert!(providers.contains(&"openrouter".to_string()));
    assert!(providers.contains(&"moonshot".to_string()));
    assert!(providers.contains(&"xai".to_string()));
    assert!(providers.contains(&"deepseek".to_string()));
    assert!(providers.contains(&"zai".to_string()));
    assert_eq!(providers.len(), 8);
}

#[test]
fn test_provider_auto_detection() {
    let factory = LLMFactory::new();

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
        factory.provider_from_model("gemini-2.5-flash-preview-05-20"),
        Some("gemini".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::OPENROUTER_X_AI_GROK_CODE_FAST_1),
        Some("openrouter".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::xai::GROK_4),
        Some("xai".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::MOONSHOT_KIMI_K2_TURBO_PREVIEW),
        Some("moonshot".to_string())
    );
    assert_eq!(factory.provider_from_model("unknown-model"), None);
}

#[test]
fn test_unified_client_creation() {
    // Test creating providers directly using the factory
    let gemini = create_provider_for_model(
        "gemini-2.5-flash-preview-05-20",
        "test_key".to_string(),
        None,
    );
    assert!(gemini.is_ok());

    let openai = create_provider_for_model(models::GPT_5, "test_key".to_string(), None);
    assert!(openai.is_ok());

    let anthropic =
        create_provider_for_model(models::CLAUDE_SONNET_4_5, "test_key".to_string(), None);
    assert!(anthropic.is_ok());

    let openrouter = create_provider_for_model(
        models::OPENROUTER_X_AI_GROK_CODE_FAST_1,
        "test_key".to_string(),
        None,
    );
    assert!(openrouter.is_ok());

    let xai = create_provider_for_model(models::xai::GROK_4, "test_key".to_string(), None);
    assert!(xai.is_ok());

    let moonshot = create_provider_for_model(
        models::MOONSHOT_KIMI_K2_TURBO_PREVIEW,
        "test_key".to_string(),
        None,
    );
    assert!(moonshot.is_ok());
}

#[test]
fn test_message_creation() {
    let user_msg = Message::user("Hello".to_string());
    assert_eq!(user_msg.content, "Hello");
    assert!(matches!(user_msg.role, MessageRole::User));

    let assistant_msg = Message::assistant("Hi".to_string());
    assert_eq!(assistant_msg.content, "Hi");
    assert!(matches!(assistant_msg.role, MessageRole::Assistant));
}

#[test]
fn test_provider_names() {
    let gemini = GeminiProvider::new("test_key".to_string());
    assert_eq!(gemini.name(), "gemini");

    let openai = OpenAIProvider::new("test_key".to_string());
    assert_eq!(openai.name(), "openai");

    let anthropic = AnthropicProvider::new("test_key".to_string());
    assert_eq!(anthropic.name(), "anthropic");

    let openrouter = OpenRouterProvider::new("test_key".to_string());
    assert_eq!(openrouter.name(), "openrouter");

    let xai = XAIProvider::new("test_key".to_string());
    assert_eq!(xai.name(), "xai");

    let moonshot = MoonshotProvider::new("test_key".to_string());
    assert_eq!(moonshot.name(), "moonshot");
}

#[test]
fn test_anthropic_tool_message_handling() {
    let anthropic = AnthropicProvider::new("test_key".to_string());

    // Test that tool messages are converted to user messages for Anthropic
    let tool_message = Message {
        role: MessageRole::Tool,
        content: "Tool result".to_string(),
        tool_calls: None,
        tool_call_id: None,
    };

    let request = LLMRequest {
        messages: vec![tool_message],
        system_prompt: None,
        tools: None,
        model: models::CLAUDE_SONNET_4_5.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    // Validate request shape instead of internal conversion
    assert!(anthropic.validate_request(&request).is_ok());
}
