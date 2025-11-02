//! Comprehensive tests for LLM providers refactor

use serde_json::json;
use vtcode_core::config::constants::models;
use vtcode_core::config::models::Provider;
use vtcode_core::llm::{
    factory::{LLMFactory, create_provider_for_model, infer_provider},
    provider::{LLMProvider, LLMRequest, Message, MessageRole, ToolDefinition},
    providers::{
        AnthropicProvider, GeminiProvider, LmStudioProvider, MoonshotProvider, OllamaProvider,
        OpenAIProvider, OpenRouterProvider, XAIProvider,
    },
};

#[test]
fn test_provider_factory_creation() {
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
    assert!(providers.contains(&"ollama".to_string()));
    assert!(providers.contains(&"lmstudio".to_string()));
    assert!(providers.contains(&"minimax".to_string()));
    assert_eq!(providers.len(), 11);
}

#[test]
fn test_provider_auto_detection() {
    let factory = LLMFactory::new();

    // Test OpenAI models
    assert_eq!(
        factory.provider_from_model("gpt-5"),
        Some("openai".to_string())
    );
    assert_eq!(
        factory.provider_from_model("gpt-5-mini"),
        Some("openai".to_string())
    );

    // Test Anthropic models
    assert_eq!(
        factory.provider_from_model(models::CLAUDE_SONNET_4_5),
        Some("anthropic".to_string())
    );
    assert_eq!(
        factory.provider_from_model("claude-sonnet-4-20250514"),
        Some("anthropic".to_string())
    );
    assert_eq!(
        factory.provider_from_model("claude-opus-4-1-20250805"),
        Some("anthropic".to_string())
    );

    // Test Gemini models
    assert_eq!(
        factory.provider_from_model("gemini-2.5-flash"),
        Some("gemini".to_string())
    );
    assert_eq!(
        factory.provider_from_model("gemini-2.5-flash-lite"),
        Some("gemini".to_string())
    );
    assert_eq!(
        factory.provider_from_model("gemini-2.5-pro"),
        Some("gemini".to_string())
    );

    // Test OpenRouter models
    assert_eq!(
        factory.provider_from_model(models::OPENROUTER_X_AI_GROK_CODE_FAST_1),
        Some("openrouter".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::OPENROUTER_QWEN3_CODER),
        Some("openrouter".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5),
        Some("openrouter".to_string())
    );

    // Test LM Studio models
    assert_eq!(
        factory.provider_from_model(models::lmstudio::META_LLAMA_31_8B_INSTRUCT),
        Some("lmstudio".to_string())
    );

    // Test xAI models
    assert_eq!(
        factory.provider_from_model(models::xai::GROK_4),
        Some("xai".to_string())
    );
    assert_eq!(
        factory.provider_from_model(models::xai::GROK_4_CODE_LATEST),
        Some("xai".to_string())
    );

    // Test Moonshot models
    assert_eq!(
        factory.provider_from_model(models::MOONSHOT_KIMI_K2_TURBO_PREVIEW),
        Some("moonshot".to_string())
    );

    // Test unknown model
    assert_eq!(factory.provider_from_model("unknown-model"), None);
}

#[test]
fn infer_provider_respects_override_and_model() {
    let provider = infer_provider(Some("openai"), "gemini-2.5-flash");
    assert_eq!(provider, Some(Provider::OpenAI));

    let provider = infer_provider(None, models::CLAUDE_SONNET_4_5);
    assert_eq!(provider, Some(Provider::Anthropic));

    let provider = infer_provider(None, "unknown-model");
    assert_eq!(provider, None);
}

#[test]
fn test_provider_creation() {
    // Test creating providers directly
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

    let ollama = create_provider_for_model(models::ollama::DEFAULT_MODEL, String::new(), None);
    assert!(ollama.is_ok());

    // Test invalid model
    let invalid = create_provider_for_model("invalid-model", "test_key".to_string(), None);
    assert!(invalid.is_err());
}

#[test]
fn test_unified_client_creation() {
    // Test creating unified clients for different providers
    let gemini_client = create_provider_for_model(
        "gemini-2.5-flash-preview-05-20",
        "test_key".to_string(),
        None,
    );
    assert!(gemini_client.is_ok());
    if let Ok(client) = gemini_client {
        assert_eq!(client.name(), "gemini");
    }

    let openai_client = create_provider_for_model("gpt-5", "test_key".to_string(), None);
    assert!(openai_client.is_ok());
    if let Ok(client) = openai_client {
        assert_eq!(client.name(), "openai");
    }

    let anthropic_client =
        create_provider_for_model(models::CLAUDE_SONNET_4_5, "test_key".to_string(), None);
    assert!(anthropic_client.is_ok());
    if let Ok(client) = anthropic_client {
        assert_eq!(client.name(), "anthropic");
    }

    let openrouter_client = create_provider_for_model(
        models::OPENROUTER_X_AI_GROK_CODE_FAST_1,
        "test_key".to_string(),
        None,
    );
    assert!(openrouter_client.is_ok());
    if let Ok(client) = openrouter_client {
        assert_eq!(client.name(), "openrouter");
    }

    let xai_client = create_provider_for_model(models::xai::GROK_4, "test_key".to_string(), None);
    assert!(xai_client.is_ok());
    if let Ok(client) = xai_client {
        assert_eq!(client.name(), "xai");
    }

    let moonshot_client = create_provider_for_model(
        models::MOONSHOT_KIMI_K2_TURBO_PREVIEW,
        "test_key".to_string(),
        None,
    );
    assert!(moonshot_client.is_ok());
    if let Ok(client) = moonshot_client {
        assert_eq!(client.name(), "moonshot");
    }

    let ollama_client =
        create_provider_for_model(models::ollama::DEFAULT_MODEL, String::new(), None);
    assert!(ollama_client.is_ok());
    if let Ok(client) = ollama_client {
        assert_eq!(client.name(), "ollama");
    }

    let lmstudio_client =
        create_provider_for_model(models::lmstudio::DEFAULT_MODEL, String::new(), None);
    assert!(lmstudio_client.is_ok());
    if let Ok(client) = lmstudio_client {
        assert_eq!(client.name(), "lmstudio");
    }
}

#[test]
fn test_message_creation() {
    // Test message creation helpers
    let user_msg = Message::user("Hello, world!".to_string());
    assert_eq!(user_msg.content.as_text(), "Hello, world!");
    assert!(matches!(user_msg.role, MessageRole::User));
    assert!(user_msg.tool_calls.is_none());

    let assistant_msg = Message::assistant("Hi there!".to_string());
    assert_eq!(assistant_msg.content.as_text(), "Hi there!");
    assert!(matches!(assistant_msg.role, MessageRole::Assistant));

    let system_msg = Message::system("You are a helpful assistant".to_string());
    assert_eq!(system_msg.content.as_text(), "You are a helpful assistant");
    assert!(matches!(system_msg.role, MessageRole::System));
}

#[test]
#[ignore]
fn test_provider_supported_models() {
    // Test that providers report correct supported models
    let gemini = GeminiProvider::new("test_key".to_string());
    let gemini_models = gemini.supported_models();
    assert!(gemini_models.contains(&"gemini-2.5-flash".to_string()));
    assert!(gemini_models.contains(&"gemini-2.5-flash-lite".to_string()));
    assert!(gemini_models.contains(&"gemini-2.5-pro".to_string()));
    assert!(gemini_models.contains(&"gemini-2.5-flash-lite-preview-06-17".to_string()));
    assert!(gemini_models.contains(&"gemini-2.5-flash-preview-05-20".to_string()));
    assert!(gemini_models.len() >= 5);

    let openai = OpenAIProvider::new("test_key".to_string());
    let openai_models = openai.supported_models();
    assert!(openai_models.contains(&"gpt-5".to_string()));
    assert!(openai_models.contains(&"gpt-5-mini".to_string()));
    assert!(openai_models.len() >= 2);

    let anthropic = AnthropicProvider::new("test_key".to_string());
    let anthropic_models = anthropic.supported_models();
    assert!(anthropic_models.contains(&models::CLAUDE_SONNET_4_5.to_string()));
    assert!(anthropic_models.contains(&models::CLAUDE_HAIKU_4_5.to_string()));
    assert!(anthropic_models.contains(&models::CLAUDE_SONNET_4_20250514.to_string()));
    assert!(anthropic_models.contains(&"claude-opus-4-1-20250805".to_string()));
    assert!(anthropic_models.len() >= 3);

    let openrouter = OpenRouterProvider::new("test_key".to_string());
    let openrouter_models = openrouter.supported_models();
    assert!(openrouter_models.contains(&models::OPENROUTER_X_AI_GROK_CODE_FAST_1.to_string()));
    assert!(openrouter_models.contains(&models::OPENROUTER_QWEN3_CODER.to_string()));
    assert!(
        openrouter_models.contains(&models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5.to_string())
    );
    assert!(openrouter_models.len() >= 2);

    let xai = XAIProvider::new("test_key".to_string());
    let xai_models = xai.supported_models();
    assert!(xai_models.contains(&models::xai::GROK_4.to_string()));
    assert!(xai_models.contains(&models::xai::GROK_4_CODE.to_string()));
    assert!(xai_models.len() >= 2);

    let moonshot = MoonshotProvider::new("test_key".to_string());
    let moonshot_models = moonshot.supported_models();
    assert!(moonshot_models.contains(&models::MOONSHOT_KIMI_K2_TURBO_PREVIEW.to_string()));
    assert!(moonshot_models.contains(&models::MOONSHOT_KIMI_K2_0905_PREVIEW.to_string()));
    assert!(moonshot_models.contains(&models::MOONSHOT_KIMI_LATEST_128K.to_string()));
    assert!(moonshot_models.len() >= 3);
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

    let ollama = OllamaProvider::new(String::new());
    assert_eq!(ollama.name(), "ollama");

    let lmstudio = LmStudioProvider::new(String::new());
    assert_eq!(lmstudio.name(), "lmstudio");
}

#[test]
#[ignore]
fn test_request_validation() {
    let gemini = GeminiProvider::new("test_key".to_string());
    let openai = OpenAIProvider::new("test_key".to_string());
    let anthropic = AnthropicProvider::new("test_key".to_string());
    let openrouter = OpenRouterProvider::new("test_key".to_string());
    let xai = XAIProvider::new("test_key".to_string());

    // Test valid requests
    let valid_gemini_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: "gemini-2.5-flash-lite-preview-06-17".to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(gemini.validate_request(&valid_gemini_request).is_ok());

    let valid_openai_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: "gpt-5".to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(openai.validate_request(&valid_openai_request).is_ok());

    let valid_anthropic_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
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
    assert!(anthropic.validate_request(&valid_anthropic_request).is_ok());

    let legacy_anthropic_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(
        anthropic
            .validate_request(&legacy_anthropic_request)
            .is_ok()
    );

    let valid_openrouter_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: models::OPENROUTER_X_AI_GROK_CODE_FAST_1.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(
        openrouter
            .validate_request(&valid_openrouter_request)
            .is_ok()
    );

    let valid_xai_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: models::xai::GROK_4.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(xai.validate_request(&valid_xai_request).is_ok());

    // Test invalid requests (wrong model for provider)
    let invalid_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: None,
        model: "invalid-model".to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };
    assert!(gemini.validate_request(&invalid_request).is_err());
    assert!(openai.validate_request(&invalid_request).is_err());
    assert!(anthropic.validate_request(&invalid_request).is_err());
    assert!(xai.validate_request(&invalid_request).is_err());
}

#[test]
fn test_anthropic_tool_message_handling() {
    let anthropic = AnthropicProvider::new("test_key".to_string());

    // Test tool message conversion
    let tool_message =
        Message::tool_response("tool_123".to_string(), "Tool result content".to_string());

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

    // Use the public validator as a proxy for ensuring request shape is acceptable
    // (internal conversion is implementation detail and not tested directly here)
    assert!(anthropic.validate_request(&request).is_ok());
}

#[test]
fn test_backward_compatibility() {
    use vtcode_core::llm::make_client;
    use vtcode_core::models::ModelId;

    // Test that the old make_client function still works
    use std::str::FromStr;
    let model = ModelId::from_str("gemini-2.5-flash-preview-05-20").unwrap();
    let client = make_client("test_key".to_string(), model);

    // Should be able to get model ID
    let model_id = client.model_id();
    assert!(!model_id.is_empty());
}

#[test]
fn test_tool_definition_creation() {
    let tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a location".to_string(),
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string", "description": "The location to get weather for"}
            },
            "required": ["location"]
        }),
    );

    assert_eq!(tool.function_name(), "get_weather");
    assert_eq!(tool.function.description, "Get weather for a location");
    assert!(tool.function.parameters.is_object());
}
