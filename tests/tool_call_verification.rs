//! Comprehensive tool call verification for all LLM providers

use serde_json::json;
use vtcode_core::config::constants::models;
use vtcode_core::llm::{
    provider::{
        LLMProvider, LLMRequest, Message, MessageRole, ToolCall, ToolChoice, ToolDefinition,
    },
    providers::{
        AnthropicProvider, GeminiProvider, OllamaProvider, OpenAIProvider, OpenRouterProvider,
    },
};

#[test]
fn test_openai_tool_call_format() {
    let provider = OpenAIProvider::new("test_key".to_string());

    // Test tool definition
    let tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a location".to_string(),
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    );

    // Test assistant message with tool call
    let assistant_msg = Message {
        role: MessageRole::Assistant,
        content: "I'll get the weather for you.".to_string(),
        tool_calls: Some(vec![ToolCall::function(
            "call_123".to_string(),
            "get_weather".to_string(),
            json!({"location": "New York"}).to_string(),
        )]),
        tool_call_id: None,
        reasoning: None,
    };

    // Test tool response message
    let tool_msg = Message {
        role: MessageRole::Tool,
        content: "Sunny, 72°F".to_string(),
        tool_calls: None,
        tool_call_id: Some("call_123".to_string()),
        reasoning: None,
    };

    let request = LLMRequest {
        messages: vec![
            Message::user("What's the weather in New York?".to_string()),
            assistant_msg,
            tool_msg,
        ],
        system_prompt: Some("You are a helpful assistant.".to_string()),
        tools: Some(vec![tool]),
        model: models::GPT_5.to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.7),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    // Only validate shape via provider API; internal conversion details are private
    assert!(provider.validate_request(&request).is_ok());
}

#[test]
fn test_anthropic_tool_call_format() {
    let provider = AnthropicProvider::new("test_key".to_string());

    // Test tool definition
    let tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a location".to_string(),
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    );

    // Test assistant message with tool call
    let assistant_msg = Message {
        role: MessageRole::Assistant,
        content: "I'll get the weather for you.".to_string(),
        tool_calls: Some(vec![ToolCall::function(
            "toolu_123".to_string(),
            "get_weather".to_string(),
            json!({"location": "New York"}).to_string(),
        )]),
        tool_call_id: None,
        reasoning: None,
    };

    // Test tool response message
    let tool_msg = Message {
        role: MessageRole::Tool,
        content: "Sunny, 72°F".to_string(),
        tool_calls: None,
        tool_call_id: Some("toolu_123".to_string()),
        reasoning: None,
    };

    let request = LLMRequest {
        messages: vec![
            Message::user("What's the weather in New York?".to_string()),
            assistant_msg,
            tool_msg,
        ],
        system_prompt: Some("You are a helpful assistant.".to_string()),
        tools: Some(vec![tool]),
        model: models::CLAUDE_SONNET_4_5.to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.7),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    // Only validate shape via provider API; internal conversion details are private
    assert!(provider.validate_request(&request).is_ok());
}

#[test]
fn test_gemini_tool_call_format() {
    let provider = GeminiProvider::new("test_key".to_string());

    // Test tool definition
    let tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a location".to_string(),
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    );

    // Test assistant message with tool call
    let assistant_msg = Message {
        role: MessageRole::Assistant,
        content: "I'll get the weather for you.".to_string(),
        tool_calls: Some(vec![ToolCall::function(
            "func_123".to_string(),
            "get_weather".to_string(),
            json!({"location": "New York"}).to_string(),
        )]),
        tool_call_id: None,
        reasoning: None,
    };

    // Test tool response message
    let tool_msg = Message {
        role: MessageRole::Tool,
        content: "Sunny, 72°F".to_string(),
        tool_calls: None,
        tool_call_id: Some("func_123".to_string()),
        reasoning: None,
    };

    let request = LLMRequest {
        messages: vec![
            Message::user("What's the weather in New York?".to_string()),
            assistant_msg,
            tool_msg,
        ],
        system_prompt: Some("You are a helpful assistant.".to_string()),
        tools: Some(vec![tool]),
        model: "gemini-2.5-flash-preview-05-20".to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.7),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    assert!(provider.validate_request(&request).is_ok());
}

#[test]
fn test_all_providers_tool_validation() {
    let gemini = GeminiProvider::new("test_key".to_string());
    let openai = OpenAIProvider::new("test_key".to_string());
    let anthropic = AnthropicProvider::new("test_key".to_string());
    let openrouter = OpenRouterProvider::new("test_key".to_string());
    let ollama = OllamaProvider::from_config(None, None, None, None);

    // Test valid requests with tools
    let tool = ToolDefinition::function(
        "test_tool".to_string(),
        "A test tool".to_string(),
        json!({"type": "object"}),
    );

    let gemini_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: Some(vec![tool.clone()]),
        model: "gemini-2.5-flash-preview-05-20".to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.7),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    let openai_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: Some(vec![tool.clone()]),
        model: models::GPT_5.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: Some(ToolChoice::auto()),
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    let anthropic_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: Some(vec![tool.clone()]),
        model: models::CLAUDE_SONNET_4_20250514.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    let openrouter_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: Some(vec![tool.clone()]),
        model: models::OPENROUTER_X_AI_GROK_CODE_FAST_1.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    assert!(gemini.validate_request(&gemini_request).is_ok());
    assert!(openai.validate_request(&openai_request).is_ok());
    assert!(anthropic.validate_request(&anthropic_request).is_ok());
    assert!(openrouter.validate_request(&openrouter_request).is_ok());

    let ollama_request = LLMRequest {
        messages: vec![Message::user("test".to_string())],
        system_prompt: None,
        tools: Some(vec![tool]),
        model: models::ollama::DEFAULT_MODEL.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    assert!(
        ollama.validate_request(&ollama_request).is_ok(),
        "Ollama should accept tool-bearing requests"
    );
}

#[test]
fn test_openrouter_tool_call_format() {
    let provider = OpenRouterProvider::new("test_key".to_string());

    let tool = ToolDefinition::function(
        "get_weather".to_string(),
        "Get weather for a location".to_string(),
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            },
            "required": ["location"]
        }),
    );

    let assistant_msg = Message {
        role: MessageRole::Assistant,
        content: "I'll get the weather for you.".to_string(),
        tool_calls: Some(vec![ToolCall::function(
            "call_456".to_string(),
            "get_weather".to_string(),
            json!({"location": "Paris"}).to_string(),
        )]),
        tool_call_id: None,
        reasoning: None,
    };

    let tool_msg = Message {
        role: MessageRole::Tool,
        content: "Cloudy, 68°F".to_string(),
        tool_calls: None,
        tool_call_id: Some("call_456".to_string()),
        reasoning: None,
    };

    let request = LLMRequest {
        messages: vec![
            Message::user("What's the weather in Paris?".to_string()),
            assistant_msg,
            tool_msg,
        ],
        system_prompt: Some("You are a helpful assistant.".to_string()),
        tools: Some(vec![tool]),
        model: models::OPENROUTER_X_AI_GROK_CODE_FAST_1.to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.7),
        stream: false,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

    assert!(provider.validate_request(&request).is_ok());
}

#[test]
fn test_provider_tool_support_matrix() {
    let gemini = GeminiProvider::new("test_key".to_string());
    let openai = OpenAIProvider::new("test_key".to_string());
    let anthropic = AnthropicProvider::new("test_key".to_string());
    let openrouter = OpenRouterProvider::new("test_key".to_string());
    let ollama = OllamaProvider::from_config(None, None, None, None);

    for &model in models::google::SUPPORTED_MODELS {
        assert!(
            gemini.supports_tools(model),
            "Gemini should advertise tool calling for {}",
            model
        );
    }

    for &model in models::openai::SUPPORTED_MODELS {
        let supports = openai.supports_tools(model);
        if models::openai::TOOL_UNAVAILABLE_MODELS.contains(&model) {
            assert!(
                !supports,
                "OpenAI should disable tool calling for {}",
                model
            );
        } else {
            assert!(
                supports,
                "OpenAI should advertise tool calling for {}",
                model
            );
        }
    }

    for &model in models::anthropic::SUPPORTED_MODELS {
        assert!(
            anthropic.supports_tools(model),
            "Anthropic should advertise tool calling for {}",
            model
        );
    }

    for &model in models::openrouter::SUPPORTED_MODELS {
        let supports = openrouter.supports_tools(model);
        if models::openrouter::TOOL_UNAVAILABLE_MODELS.contains(&model) {
            assert!(
                !supports,
                "OpenRouter should disable tool calling for {}",
                model
            );
        } else {
            assert!(
                supports,
                "OpenRouter should advertise tool calling for {}",
                model
            );
        }
    }

    for &model in models::ollama::SUPPORTED_MODELS {
        assert!(
            ollama.supports_tools(model),
            "Ollama should advertise tool calling for {}",
            model
        );
    }

    for &model in models::openai::TOOL_UNAVAILABLE_MODELS {
        assert!(
            !openai.supports_tools(model),
            "OpenAI should disable tool calling for {}",
            model
        );
    }

    for &model in models::openrouter::TOOL_UNAVAILABLE_MODELS {
        assert!(
            !openrouter.supports_tools(model),
            "OpenRouter should disable tool calling for {}",
            model
        );
    }
}
