use crate::provider::{LLMRequest, Message, ParallelToolConfig, ToolChoice, ToolDefinition};
use crate::providers::anthropic::validation::*;
use serde_json::json;
use std::sync::Arc;
use vtcode_config::constants::models;
use vtcode_config::core::AnthropicConfig;

#[test]
fn test_validate_empty_messages() {
    let request = LLMRequest {
        messages: vec![].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        ..Default::default()
    };
    let config = AnthropicConfig::default();
    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn unsupported_structured_output_error_names_the_supported_models() {
    let request = LLMRequest {
        messages: vec![Message::user("hi".to_string())].into(),
        model: "claude-haiku-4-5".to_string(),
        output_format: Some(json!({ "type": "object", "properties": {}, "additionalProperties": false })),
        ..Default::default()
    };
    let config = AnthropicConfig::default();
    let err = validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic")
        .expect_err("haiku 4.5 has no structured output support");
    let message = err.to_string();
    assert!(message.contains("'claude-haiku-4-5'"), "{message}");
    for model in crate::providers::anthropic::capabilities::structured_output_models() {
        assert!(message.contains(model), "missing {model}: {message}");
    }
    assert!(!message.contains("4.6") && !message.contains("Haiku 4.5 models"), "{message}");
}

#[test]
fn test_validate_anthropic_schema_valid() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"}
        },
        "required": ["name", "age"],
        "additionalProperties": false
    });
    validate_anthropic_schema(&schema, "Anthropic").unwrap();
}

#[test]
fn test_validate_anthropic_schema_invalid_numeric_constraints() {
    let schema = json!({
        "type": "object",
        "properties": {
            "age": {
                "type": "integer",
                "minimum": 0,
                "maximum": 100
            }
        },
        "additionalProperties": false
    });
    assert!(validate_anthropic_schema(&schema, "Anthropic").is_err());
}

#[test]
fn test_validate_anthropic_schema_invalid_string_constraints() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 50
            }
        },
        "additionalProperties": false
    });
    assert!(validate_anthropic_schema(&schema, "Anthropic").is_err());
}

#[test]
fn test_validate_effort_rejects_models_without_thinking_profile() {
    let request = LLMRequest {
        messages: vec![Message::user("hi".to_string())].into(),
        model: "claude-3-5-sonnet-20241022".to_string(),
        effort: Some("medium".to_string()),
        ..Default::default()
    };
    let config = AnthropicConfig::default();
    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn test_validate_effort_max_supported_for_adaptive_models() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("hi".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        effort: Some("max".to_string()),
        ..Default::default()
    };
    validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").unwrap();
}

#[test]
fn test_validate_effort_xhigh_accepted_for_sonnet_5() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("hi".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        effort: Some("xhigh".to_string()),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_ok());
}

#[test]
fn test_validate_programmatic_tool_calling_rejects_disable_parallel_tool_use() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("find warmest city".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        tools: Some(Arc::new(vec![
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
        parallel_tool_config: Some(Box::new(ParallelToolConfig::sequential_only())),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn test_validate_programmatic_tool_calling_rejects_strict_tools() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("find warmest city".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        tools: Some(Arc::new(vec![
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
            .with_strict(true)
            .with_allowed_callers(vec!["code_execution_20250825".to_string()]),
        ])),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn test_validate_programmatic_tool_calling_rejects_any_tool_choice() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("find warmest city".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        tools: Some(Arc::new(vec![
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
        tool_choice: Some(ToolChoice::any()),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn test_validate_programmatic_tool_calling_rejects_specific_tool_choice() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("find warmest city".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        tools: Some(Arc::new(vec![
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
        tool_choice: Some(ToolChoice::function("get_weather".to_string())),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}

#[test]
fn test_validate_anthropic_tool_name_rejects_invalid_names() {
    let config = AnthropicConfig::default();
    let request = LLMRequest {
        messages: vec![Message::user("hi".to_string())].into(),
        model: models::CLAUDE_SONNET_5.to_string(),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "bad tool name".to_string(),
            "Bad name".to_string(),
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )])),
        ..Default::default()
    };

    assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config, "Anthropic").is_err());
}
