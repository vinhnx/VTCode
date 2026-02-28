//! Tests for the Anthropic provider module
//!
//! This module contains unit tests for the modular Anthropic provider implementation.
//! Tests are organized by submodule functionality.

#[cfg(test)]
mod capabilities_tests {
    use crate::config::constants::models;
    use crate::llm::providers::anthropic::capabilities::*;

    #[test]
    fn test_supports_structured_output() {
        assert!(supports_structured_output(
            models::CLAUDE_SONNET_4_5,
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_structured_output(
            models::CLAUDE_OPUS_4_1_20250805,
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_structured_output(
            "claude-3-7-sonnet-test",
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_structured_output(
            "claude-3-5-sonnet-test",
            models::anthropic::DEFAULT_MODEL
        ));
    }

    #[test]
    fn test_supports_vision() {
        assert!(supports_vision(
            models::CLAUDE_SONNET_4_5,
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_vision(
            "claude-3-opus",
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_vision(
            "claude-4-sonnet",
            models::anthropic::DEFAULT_MODEL
        ));
    }

    #[test]
    fn test_supports_effort() {
        assert!(supports_effort(
            models::CLAUDE_OPUS_4_6,
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(supports_effort(
            models::CLAUDE_OPUS_4_5,
            models::anthropic::DEFAULT_MODEL
        ));
        assert!(!supports_effort(
            models::CLAUDE_SONNET_4_5,
            models::anthropic::DEFAULT_MODEL
        ));
    }

    #[test]
    fn test_effective_context_size() {
        assert_eq!(
            effective_context_size("claude-sonnet-4-5-latest"),
            1_000_000
        );
        assert_eq!(effective_context_size("claude-haiku-4-5-latest"), 1_000_000);
        assert_eq!(effective_context_size("claude-3-opus"), 200_000);
    }

    #[test]
    fn test_supported_models() {
        let models = supported_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.contains("claude")));
    }
}

#[cfg(test)]
mod prompt_cache_tests {
    use crate::config::core::AnthropicPromptCacheSettings;
    use crate::llm::providers::anthropic::prompt_cache::*;

    #[test]
    fn test_cache_ttl_for_seconds() {
        assert_eq!(get_cache_ttl_for_seconds(300), "5m");
        assert_eq!(get_cache_ttl_for_seconds(3600), "1h");
        assert_eq!(get_cache_ttl_for_seconds(7200), "1h");
    }

    #[test]
    fn test_requires_extended_ttl_beta() {
        let mut settings = AnthropicPromptCacheSettings::default();
        settings.tools_ttl_seconds = 3600;
        settings.messages_ttl_seconds = 300;
        assert!(requires_extended_ttl_beta(&settings));

        settings.tools_ttl_seconds = 300;
        settings.messages_ttl_seconds = 300;
        assert!(!requires_extended_ttl_beta(&settings));
    }
}

#[cfg(test)]
mod validation_tests {
    use crate::config::constants::models;
    use crate::config::core::AnthropicConfig;
    use crate::llm::provider::LLMRequest;
    use crate::llm::providers::anthropic::validation::*;
    use serde_json::json;

    #[test]
    fn test_validate_empty_messages() {
        let request = LLMRequest {
            messages: vec![],
            model: models::CLAUDE_SONNET_4_5.to_string(),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config).is_err());
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
        assert!(validate_anthropic_schema(&schema).is_ok());
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
        assert!(validate_anthropic_schema(&schema).is_err());
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
        assert!(validate_anthropic_schema(&schema).is_err());
    }

    #[test]
    fn test_validate_effort_rejects_unsupported_models() {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user("hi".to_string())],
            model: models::CLAUDE_SONNET_4_5.to_string(),
            effort: Some("medium".to_string()),
            ..Default::default()
        };
        let config = AnthropicConfig::default();
        assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config).is_err());
    }

    #[test]
    fn test_validate_effort_max_only_for_opus_4_6() {
        let config = AnthropicConfig::default();
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user("hi".to_string())],
            model: models::CLAUDE_OPUS_4_5.to_string(),
            effort: Some("max".to_string()),
            ..Default::default()
        };
        assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config).is_err());

        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user("hi".to_string())],
            model: models::CLAUDE_OPUS_4_6.to_string(),
            effort: Some("max".to_string()),
            ..Default::default()
        };
        assert!(validate_request(&request, models::anthropic::DEFAULT_MODEL, &config).is_ok());
    }
}

#[cfg(test)]
mod response_parser_tests {
    use crate::llm::provider::FinishReason;
    use crate::llm::providers::anthropic::response_parser::*;
    use serde_json::json;

    #[test]
    fn test_parse_finish_reason() {
        assert!(matches!(
            parse_finish_reason("end_turn"),
            FinishReason::Stop
        ));
        assert!(matches!(
            parse_finish_reason("max_tokens"),
            FinishReason::Length
        ));
        assert!(matches!(
            parse_finish_reason("tool_use"),
            FinishReason::ToolCalls
        ));
        assert!(matches!(
            parse_finish_reason("refusal"),
            FinishReason::Refusal
        ));
    }

    #[test]
    fn test_parse_response_basic() {
        let response_json = json!({
            "content": [
                {"type": "text", "text": "Hello, world!"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let response =
            parse_response(response_json, "claude-haiku-4-5".to_string()).expect("parse response");
        assert_eq!(response.content.as_deref(), Some("Hello, world!"));
        assert!(matches!(response.finish_reason, FinishReason::Stop));
    }

    #[test]
    fn test_parse_response_with_thinking() {
        let response_json = json!({
            "content": [
                {"type": "thinking", "thinking": "Let me think...", "signature": "sig123"},
                {"type": "text", "text": "The answer is 42."}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        });

        let response =
            parse_response(response_json, "claude-haiku-4-5".to_string()).expect("parse response");
        let reasoning = response
            .reasoning
            .as_deref()
            .expect("expected reasoning content");
        assert!(reasoning.contains("Let me think"));
        assert_eq!(response.content.as_deref(), Some("The answer is 42."));
    }

    #[test]
    fn test_parse_response_with_tool_use() {
        let response_json = json!({
            "content": [
                {
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "get_weather",
                    "input": {"location": "NYC"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let response =
            parse_response(response_json, "claude-haiku-4-5".to_string()).expect("parse response");
        let tool_calls = response.tool_calls.as_ref().expect("expected tool calls");
        assert_eq!(tool_calls.len(), 1);
        let function = tool_calls[0]
            .function
            .as_ref()
            .expect("expected function call");
        assert_eq!(function.name, "get_weather");
    }
}

#[cfg(test)]
mod request_builder_tests {
    use crate::config::constants::models;
    use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};
    use crate::llm::provider::{LLMRequest, Message, ToolDefinition};
    use crate::llm::providers::anthropic::request_builder::{
        RequestBuilderContext, convert_to_anthropic_format, tool_result_blocks,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn test_tool_result_blocks_empty() {
        let blocks = tool_result_blocks("");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
    }

    #[test]
    fn test_tool_result_blocks_plain_text() {
        let blocks = tool_result_blocks("Hello world");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Hello world");
    }

    #[test]
    fn test_tool_result_blocks_json_string() {
        let blocks = tool_result_blocks("\"Hello\"");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Hello");
    }

    #[test]
    fn test_tool_result_blocks_json_object() {
        let blocks = tool_result_blocks("{\"key\": \"value\"}");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "{\"key\":\"value\"}");
    }

    #[test]
    fn test_convert_to_anthropic_format_adds_top_level_cache_control() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_4_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["cache_control"]["type"], "ephemeral");
        assert_eq!(payload["cache_control"]["ttl"], "5m");
    }

    #[test]
    fn test_convert_to_anthropic_format_reuses_last_explicit_ttl_for_automatic_cache() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_4_5.to_string(),
            system_prompt: Some(Arc::new("system prompt".to_string())),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings {
            cache_tool_definitions: false,
            cache_user_messages: false,
            tools_ttl_seconds: 3600,
            messages_ttl_seconds: 300,
            ..AnthropicPromptCacheSettings::default()
        };
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["cache_control"]["ttl"], "1h");
        assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    }

    #[test]
    fn test_convert_to_anthropic_format_skips_automatic_cache_when_slots_exhausted() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_4_5.to_string(),
            system_prompt: Some(Arc::new("stable system".to_string())),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "do_work".to_string(),
                "Do work".to_string(),
                json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            )])),
            messages: vec![
                Message::user("aaaaaaaa".to_string()),
                Message::user("bbbbbbbb".to_string()),
            ],
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings {
            max_breakpoints: 4,
            min_message_length_for_cache: 1,
            ..AnthropicPromptCacheSettings::default()
        };
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload.get("cache_control").is_none());
        assert!(payload["tools"][0]["cache_control"].is_object());
        assert!(payload["system"][0]["cache_control"].is_object());
        assert!(payload["messages"][0]["content"][0]["cache_control"].is_object());
        assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
    }

    #[test]
    fn test_convert_to_anthropic_format_includes_native_web_search_tool() {
        let request = LLMRequest {
            model: models::CLAUDE_OPUS_4_6.to_string(),
            messages: vec![Message::user("find latest rust release notes".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "web_search_20260209".to_string(),
                function: None,
                web_search: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
            }])),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["type"], "web_search_20260209");
        assert_eq!(payload["tools"][0]["name"], "web_search");
        assert!(payload["tools"][0]["input_schema"].is_null());
    }
}
