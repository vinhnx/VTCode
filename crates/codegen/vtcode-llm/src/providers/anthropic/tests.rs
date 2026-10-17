//! Tests for the Anthropic provider module
//!
//! This module contains unit tests for the modular Anthropic provider implementation.
//! Tests are organized by submodule functionality.

#[cfg(test)]
mod capabilities_tests {
    use crate::providers::anthropic::capabilities::*;
    use vtcode_config::constants::models;

    #[test]
    fn test_supports_structured_output() {
        assert!(supports_structured_output(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
        assert!(supports_structured_output("claude-opus-4-5-20251101", models::anthropic::DEFAULT_MODEL));
        assert!(supports_structured_output("claude-sonnet-4-5-20250929", models::anthropic::DEFAULT_MODEL));
        assert!(supports_structured_output(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
        assert!(!supports_structured_output("claude-3-7-sonnet-test", models::anthropic::DEFAULT_MODEL));
    }

    #[test]
    fn structured_output_models_lists_exactly_the_accepted_models() {
        let listed = structured_output_models();
        for model in models::anthropic::SUPPORTED_MODELS {
            assert!(listed.contains(model), "{model} accepts structured output but is not listed");
        }
        for model in &listed {
            assert!(supports_structured_output(model, ""), "{model} is listed but rejected");
        }
        for rejected in ["claude-sonnet-4-6", "claude-opus-4-8", "claude-haiku-4-5"] {
            assert!(!supports_structured_output(rejected, ""), "{rejected}");
            assert!(!listed.contains(&rejected), "{rejected}");
        }
    }

    #[test]
    fn test_supports_vision() {
        assert!(supports_vision(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
        assert!(supports_vision("claude-3-opus", models::anthropic::DEFAULT_MODEL));
        assert!(supports_vision("claude-4-sonnet", models::anthropic::DEFAULT_MODEL));
    }

    #[test]
    fn test_supports_effort() {
        assert!(supports_effort(models::CLAUDE_SONNET_5, models::anthropic::DEFAULT_MODEL));
    }

    #[test]
    fn test_effective_context_size() {
        assert_eq!(effective_context_size(models::CLAUDE_SONNET_5), 1_000_000);
        assert_eq!(effective_context_size("claude-sonnet-4-5-latest"), 200_000);
        assert_eq!(effective_context_size("claude-haiku-4-5-latest"), 200_000);
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
    use crate::provider::PromptCacheProfile;
    use crate::providers::anthropic::prompt_cache::*;
    use vtcode_config::core::AnthropicPromptCacheSettings;

    #[test]
    fn test_cache_ttl_for_seconds() {
        assert_eq!(get_cache_ttl_for_seconds(300), "5m");
        assert_eq!(get_cache_ttl_for_seconds(3600), "1h");
        assert_eq!(get_cache_ttl_for_seconds(7200), "1h");
    }

    #[test]
    fn test_requires_extended_ttl_beta() {
        let settings = AnthropicPromptCacheSettings {
            tools_ttl_seconds: 3600,
            messages_ttl_seconds: 300,
            ..Default::default()
        };
        assert!(requires_extended_ttl_beta(&settings, None));

        let settings = AnthropicPromptCacheSettings {
            tools_ttl_seconds: 300,
            messages_ttl_seconds: 300,
            ..Default::default()
        };
        assert!(!requires_extended_ttl_beta(&settings, None));

        // A budget-continuation profile promotes the messages breakpoint to the
        // profile TTL (1h by default), so the beta must be requested even with
        // 5m tools/messages settings.
        assert_eq!(messages_cache_ttl_for_profile(&settings, Some(PromptCacheProfile::BudgetContinuation)), "1h");
        assert!(requires_extended_ttl_beta(&settings, Some(PromptCacheProfile::BudgetContinuation)));

        // The profile TTL itself is honored when explicitly shortened.
        let short_profile = AnthropicPromptCacheSettings {
            tools_ttl_seconds: 300,
            messages_ttl_seconds: 300,
            extended_ttl_seconds: Some(300),
            ..Default::default()
        };
        assert_eq!(messages_cache_ttl_for_profile(&short_profile, Some(PromptCacheProfile::BudgetContinuation)), "5m");
        assert!(!requires_extended_ttl_beta(&short_profile, Some(PromptCacheProfile::BudgetContinuation)));
    }
}

#[cfg(test)]
mod validation_tests {
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
}

#[cfg(test)]
mod response_parser_tests {
    use crate::provider::FinishReason;
    use crate::providers::anthropic::response_parser::*;
    use serde_json::json;

    #[test]
    fn test_parse_finish_reason() {
        assert!(matches!(parse_finish_reason("end_turn"), FinishReason::Stop));
        assert!(matches!(parse_finish_reason("max_tokens"), FinishReason::Length));
        assert!(matches!(parse_finish_reason("tool_use"), FinishReason::ToolCalls));
        assert!(matches!(parse_finish_reason("compaction"), FinishReason::Pause));
        assert!(matches!(parse_finish_reason("pause_turn"), FinishReason::Pause));
        assert!(matches!(parse_finish_reason("refusal"), FinishReason::Refusal));
        assert!(matches!(parse_finish_reason("model_context_window_exceeded"), FinishReason::Length));
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

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        assert_eq!(response.content.as_deref(), Some("Hello, world!"));
        assert!(matches!(response.finish_reason, FinishReason::Stop));
    }

    #[test]
    fn test_parse_response_carries_refusal_stop_details() {
        let response_json = json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": {
                "type": "refusal",
                "category": "cyber",
                "explanation": "declined",
                "fallback_credit_token": "credit-1",
                "fallback_has_prefill_claim": false
            }
        });

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        assert!(matches!(response.finish_reason, FinishReason::Refusal));
        let details = response.reasoning_details.expect("stop_details detail");
        assert_eq!(details.len(), 1);
        let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized stop_details");
        assert_eq!(detail["type"], "stop_details");
        assert_eq!(detail["category"], "cyber");
        assert_eq!(detail["explanation"], "declined");
        assert_eq!(detail["fallback_credit_token"], "credit-1");
        assert_eq!(detail["fallback_has_prefill_claim"], false);
    }

    #[test]
    fn test_parse_response_ignores_null_stop_details() {
        let response_json = json!({
            "content": [{"type": "text", "text": "done"}],
            "stop_reason": "end_turn",
            "stop_details": null
        });

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        assert!(response.reasoning_details.is_none(), "details: {:?}", response.reasoning_details);
    }

    #[test]
    fn test_parse_response_with_compaction() {
        let response_json = json!({
            "content": [
                {
                    "type": "compaction",
                    "content": "opaque summary",
                    "signature": "signed-summary",
                    "encrypted_content": "opaque-extension"
                }
            ],
            "stop_reason": "compaction"
        });

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        assert!(matches!(response.finish_reason, FinishReason::Pause));
        assert_eq!(response.compaction.as_deref(), Some("opaque summary"));
        let details = response.reasoning_details.expect("compaction detail");
        assert_eq!(details.len(), 1);
        let detail: serde_json::Value = serde_json::from_str(&details[0]).expect("serialized compaction detail");
        assert_eq!(detail["signature"], "signed-summary");
        assert_eq!(detail["encrypted_content"], "opaque-extension");
    }

    #[test]
    fn test_parse_response_keeps_compaction_block_when_content_is_null() {
        let response = parse_response(
            json!({
                "content": [{"type": "compaction", "content": null, "signature": "signed-summary"}],
                "stop_reason": "compaction"
            }),
            "claude-sonnet-5".to_string(),
        )
        .expect("parse response");

        assert!(response.compaction.is_none());
        let details = response.reasoning_details.expect("opaque compaction detail");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&details[0]).expect("detail")["content"], json!(null));
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

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        let reasoning = response.reasoning.as_deref().expect("expected reasoning content");
        assert!(reasoning.contains("Let me think"));
        assert_eq!(
            response.reasoning_details,
            Some(vec![
                json!({
                    "type": "thinking",
                    "thinking": "Let me think...",
                    "signature": "sig123"
                })
                .to_string()
            ])
        );
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

        let response = parse_response(response_json, "claude-sonnet-5".to_string()).expect("parse response");
        let tool_calls = response.tool_calls.as_ref().expect("expected tool calls");
        assert_eq!(tool_calls.len(), 1);
        let function = tool_calls[0].function.as_ref().expect("expected function call");
        assert_eq!(function.name, "get_weather");
    }
}

#[cfg(test)]
mod request_builder_tests {
    use crate::provider::{LLMRequest, Message, PromptCacheProfile, ToolDefinition};
    use crate::providers::anthropic::request_builder::{
        RequestBuilderContext, convert_to_anthropic_format, tool_result_blocks,
    };
    use serde_json::{Value, json};
    use std::sync::Arc;
    use vtcode_config::constants::models;
    use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

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
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["cache_control"]["type"], "ephemeral");
        assert_eq!(payload["cache_control"]["ttl"], "5m");
    }

    #[test]
    fn test_convert_to_anthropic_format_preserves_opus_5_mid_conversation_system_message() {
        let request = LLMRequest {
            model: models::CLAUDE_OPUS_5.to_string(),
            messages: vec![
                Message::user("Review this code.".to_string()),
                Message::system("From now on, every suggestion must include explicit type annotations.".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][1]["role"], "system");
        assert_eq!(
            payload["messages"][1]["content"][0]["text"],
            "From now on, every suggestion must include explicit type annotations."
        );
    }

    #[test]
    fn test_convert_to_anthropic_format_preserves_opus_48_mid_conversation_system_message() {
        let request = LLMRequest {
            model: models::CLAUDE_OPUS_5.to_string(),
            messages: vec![
                Message::user("Review this code.".to_string()),
                Message::system("From now on, every suggestion must include explicit type annotations.".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["messages"][0]["role"], "user");
        assert_eq!(payload["messages"][1]["role"], "system");
        assert_eq!(
            payload["messages"][1]["content"][0]["text"],
            "From now on, every suggestion must include explicit type annotations."
        );
    }

    #[test]
    fn test_convert_to_anthropic_format_drops_mid_conversation_system_for_pre_opus_48() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![
                Message::user("Review this code.".to_string()),
                Message::system("Use strict typing.".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["messages"].as_array().unwrap().len(), 1);
        assert_eq!(payload["messages"][0]["role"], "user");
    }

    #[test]
    fn test_convert_to_anthropic_format_uses_native_structured_outputs() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            output_format: Some(json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"],
                "additionalProperties": false
            })),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["output_config"]["format"]["type"], "json_schema");
        assert_eq!(payload["output_config"]["format"]["schema"]["required"], json!(["answer"]));
        assert!(payload.get("tools").is_none());
    }

    #[test]
    fn test_convert_to_anthropic_format_reuses_last_explicit_ttl_for_automatic_cache() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("system prompt")),
            messages: vec![Message::user("hello".to_string())].into(),
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["cache_control"]["ttl"], "1h");
        assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
    }

    #[test]
    fn test_convert_to_anthropic_format_skips_automatic_cache_when_slots_exhausted() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system")),
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
            ]
            .into(),
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload.get("cache_control").is_none());
        assert!(payload["tools"][0]["cache_control"].is_object());
        assert!(payload["system"][0]["cache_control"].is_object());
        assert!(payload["messages"][0]["content"][0]["cache_control"].is_object());
        assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
    }

    #[test]
    fn test_rolling_anchors_only_last_two_qualifying_messages() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system")),
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
                Message::user("cccccccc".to_string()),
            ]
            .into(),
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        // Tools and system consume two breakpoints; the remaining two land on
        // the last two qualifying user messages (rolling anchors). The oldest
        // qualifying message must NOT be anchored.
        assert!(payload["tools"][0]["cache_control"].is_object());
        assert!(payload["system"][0]["cache_control"].is_object());
        assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
        assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
        assert!(payload["messages"][2]["content"][0]["cache_control"].is_object());
    }

    #[test]
    fn test_message_anchoring_skipped_when_tools_and_system_exhaust_breakpoints() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system")),
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
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings {
            max_breakpoints: 2,
            min_message_length_for_cache: 1,
            ..AnthropicPromptCacheSettings::default()
        };
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        // Tools and system exhaust the budget; no message gets an anchor.
        assert!(payload["tools"][0]["cache_control"].is_object());
        assert!(payload["system"][0]["cache_control"].is_object());
        assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
        assert!(payload["messages"][1]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_message_anchoring_uses_remaining_breakpoint_on_newest_message() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system")),
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
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings {
            max_breakpoints: 3,
            min_message_length_for_cache: 1,
            ..AnthropicPromptCacheSettings::default()
        };
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        // Only one breakpoint is left after tools+system; it must go to the
        // newest qualifying message (the primary rolling anchor).
        assert!(payload["messages"][0]["content"][0].get("cache_control").is_none());
        assert!(payload["messages"][1]["content"][0]["cache_control"].is_object());
    }

    #[test]
    fn test_convert_to_anthropic_format_splits_runtime_context_without_caching_tail() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from(
                "stable system instructions\n[Runtime Context]\n- turns: 7\n- tool_calls: 3",
            )),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload["system"].is_array());
        assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
        assert!(payload["system"][0]["text"].as_str().unwrap_or("").contains("stable system"));
        assert!(
            payload["system"][1]["text"]
                .as_str()
                .unwrap_or("")
                .contains("[Runtime Context]")
        );
        assert!(payload["system"][1].get("cache_control").is_none());
        assert!(payload.get("cache_control").is_none());
    }

    #[test]
    fn test_convert_to_anthropic_format_splits_editor_only_runtime_context_without_caching_tail() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from(
                "stable system instructions\n[Runtime Context]\n## Active Editor Context\n- Active file: src/main.rs",
            )),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload["system"].is_array());
        assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
        assert!(
            payload["system"][1]["text"]
                .as_str()
                .unwrap_or("")
                .contains("## Active Editor Context")
        );
        assert!(payload["system"][1].get("cache_control").is_none());
        assert!(payload.get("cache_control").is_none());
    }

    #[test]
    fn test_convert_to_anthropic_format_keeps_planning_notices_out_of_cached_prefix() {
        // Prompt-caching discipline: planning/full-auto transitions must live
        // in the uncached suffix so toggling them never invalidates the stable
        // prefix cache.
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from(
                "stable system instructions\n# PLANNING WORKFLOW (READ-ONLY)\nread-only\n[Harness Limits]\n- max_tool_calls_per_turn: 5",
            )),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload["system"].is_array());
        assert_eq!(payload["system"][0]["cache_control"]["ttl"], "1h");
        assert!(payload["system"][0]["text"].as_str().unwrap_or("").contains("stable system"));
        assert!(!payload["system"][0]["text"].as_str().unwrap_or("").contains("PLANNING"));
        let tail = payload["system"][1]["text"].as_str().unwrap_or("");
        assert!(tail.contains("# PLANNING WORKFLOW (READ-ONLY)"));
        assert!(tail.contains("[Harness Limits]"));
        assert!(payload["system"][1].get("cache_control").is_none());
    }

    #[test]
    fn test_convert_to_anthropic_format_uses_extended_message_ttl_for_budget_continuations() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system instructions")),
            messages: vec![Message::user("resume ".repeat(60))].into(),
            prompt_cache_profile: Some(PromptCacheProfile::BudgetContinuation),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["cache_control"]["ttl"], "1h");
        assert_eq!(payload["messages"][0]["content"][0]["cache_control"]["ttl"], "1h");
    }

    #[test]
    fn test_convert_to_anthropic_format_hoists_history_system_directives_into_system_prompt() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            system_prompt: Some(Arc::from("stable system instructions")),
            messages: vec![
                Message::user("explore architecture".to_string()),
                Message::system(
                    "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration.".to_string(),
                ),
            ].into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert!(payload["system"].is_array());
        assert!(
            payload["system"][1]["text"]
                .as_str()
                .unwrap_or("")
                .contains("[History Directives]")
        );
        assert!(
            payload["system"][1]["text"]
                .as_str()
                .unwrap_or("")
                .contains("Previous turn already completed tool execution")
        );
        assert_eq!(payload["messages"].as_array().map_or(0, |msgs| msgs.len()), 1);
        assert_eq!(payload["messages"][0]["role"], "user");
        assert!(payload["system"][1].get("cache_control").is_none());
    }

    fn mid_conversation_payload(messages: Vec<Message>) -> Value {
        let request = LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            system_prompt: Some(Arc::from("stable system instructions")),
            messages: messages.into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: true,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };
        convert_to_anthropic_format(&request, &ctx).expect("payload conversion")
    }

    #[test]
    fn test_mid_conversation_system_message_is_not_duplicated_into_system_prompt() {
        let directive = "Reuse the latest tool outputs instead of rerunning the same exploration.";
        let payload = mid_conversation_payload(vec![
            Message::user("explore architecture".to_string()),
            Message::system(directive.to_string()),
        ]);

        assert_eq!(payload["messages"][1]["role"], "system");
        assert_eq!(payload["messages"][1]["content"][0]["text"], directive);
        let system_text = payload["system"].to_string();
        assert!(!system_text.contains(directive), "directive must render only in messages[]: {system_text}");
        assert!(!system_text.contains("[History Directives]"));
    }

    #[test]
    fn test_mid_conversation_system_messages_keep_system_prompt_stable_across_turns() {
        let first_turn = mid_conversation_payload(vec![Message::user("explore architecture".to_string())]);
        let second_turn = mid_conversation_payload(vec![
            Message::user("explore architecture".to_string()),
            Message::system("Previous turn already completed tool execution.".to_string()),
            Message::assistant("Done exploring.".to_string()),
            Message::user("now summarize".to_string()),
            Message::system("Keep the summary under ten lines.".to_string()),
        ]);

        assert_eq!(first_turn["system"], second_turn["system"]);
        assert_eq!(first_turn["messages"][0]["content"][0]["text"], second_turn["messages"][0]["content"][0]["text"]);
        let roles: Vec<&str> = second_turn["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .filter_map(|message| message["role"].as_str())
            .collect();
        assert_eq!(roles, ["user", "system", "assistant", "user", "system"]);
    }

    #[test]
    fn test_leading_history_system_message_is_folded_once_on_mid_conversation_route() {
        let summary = "Previous conversation summary: the parser was refactored.";
        let payload = mid_conversation_payload(vec![
            Message::system(summary.to_string()),
            Message::user("continue".to_string()),
        ]);

        // A system message cannot be messages[0], so the leading run is folded
        // into the system prompt and dropped from messages[].
        let messages = payload["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        let system_text = payload["system"].to_string();
        assert_eq!(system_text.matches(summary).count(), 1, "summary folded exactly once: {system_text}");
    }

    fn long_context_payload(model: &str) -> Value {
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![
                Message::user("short opener".to_string()),
                Message::assistant("Acknowledged.".to_string()),
                Message::user(format!("large pasted document: {}", "x".repeat(512))),
            ]
            .into(),
            coding_agent_settings: Some(Box::new(crate::provider::CodingAgentSettings {
                long_context_optimization: true,
            })),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };
        convert_to_anthropic_format(&request, &ctx).expect("payload conversion")
    }

    #[test]
    fn test_long_context_hoisting_is_skipped_for_preserved_thinking_models() {
        for model in [models::anthropic::CLAUDE_OPUS_5_5, models::anthropic::CLAUDE_FABLE_5_1] {
            let payload = long_context_payload(model);
            let messages = payload["messages"].as_array().expect("messages");
            assert_eq!(messages.len(), 3, "{model}: {payload}");
            assert_eq!(messages[0]["content"][0]["text"], "short opener", "{model}: history must stay in order");
            assert_eq!(messages[1]["role"], "assistant");
            assert!(
                messages[2]["content"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("large pasted document")),
                "{model}: {payload}"
            );
        }
    }

    #[test]
    fn test_long_context_hoisting_still_applies_without_preserved_thinking() {
        let payload = long_context_payload(models::anthropic::CLAUDE_OPUS_5);
        let first_text = payload["messages"][0]["content"][0]["text"].as_str().unwrap_or_default();
        assert!(first_text.starts_with("large pasted document"), "largest user message hoisted: {payload}");
    }

    #[test]
    fn test_convert_to_anthropic_format_includes_native_web_search_tool() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("find latest rust release notes".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "web_search_20260209".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: Some(json!({
                    "allowed_callers": ["direct"]
                })),
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
                namespace: None,
                advisor: None,
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["type"], "web_search_20260209");
        assert_eq!(payload["tools"][0]["name"], "web_search");
        assert_eq!(payload["tools"][0]["allowed_callers"], json!(["direct"]));
        assert!(payload["tools"][0]["input_schema"].is_null());
    }

    #[test]
    fn test_convert_to_anthropic_format_rejects_mixed_web_search_domain_filters() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("search docs".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "web_search_20250305".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: Some(json!({
                    "allowed_domains": ["docs.rs"],
                    "blocked_domains": ["example.com"]
                })),
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
                namespace: None,
                advisor: None,
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
            server_side_fallbacks_available: false,
        };

        convert_to_anthropic_format(&request, &ctx).unwrap_err();
    }

    #[test]
    fn test_convert_to_anthropic_format_includes_native_code_execution_tool() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("analyze this csv".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "code_execution_20250825".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: None,
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
                namespace: None,
                advisor: None,
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["type"], "code_execution_20250825");
        assert_eq!(payload["tools"][0]["name"], "code_execution");
    }

    fn adaptive_effort_payload(
        model: &str,
        reasoning_effort: Option<vtcode_config::types::ReasoningEffortLevel>,
        configured_effort: Option<vtcode_config::types::ReasoningEffortLevel>,
    ) -> Value {
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("solve this carefully".to_string())].into(),
            reasoning_effort,
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig {
            effort: configured_effort,
            ..AnthropicConfig::default()
        };
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");
        assert_eq!(payload["thinking"]["type"], "adaptive", "model {model}");
        payload
    }

    #[test]
    fn test_convert_to_anthropic_format_uses_model_default_effort_when_unset() {
        // Opus 5.5 is tuned for `medium`; the others default to `high`.
        for (model, expected) in [
            (models::anthropic::CLAUDE_OPUS_5_5, "medium"),
            (models::anthropic::CLAUDE_OPUS_5, "high"),
            (models::CLAUDE_SONNET_5, "high"),
            (models::anthropic::CLAUDE_FABLE_5_1, "high"),
        ] {
            let payload = adaptive_effort_payload(model, None, None);
            assert_eq!(payload["output_config"]["effort"], expected, "model {model}");
        }
    }

    #[test]
    fn test_convert_to_anthropic_format_honors_explicit_reasoning_effort_over_model_default() {
        use vtcode_config::types::ReasoningEffortLevel;

        let payload =
            adaptive_effort_payload(models::anthropic::CLAUDE_OPUS_5_5, Some(ReasoningEffortLevel::High), None);
        assert_eq!(payload["output_config"]["effort"], "high");

        // `agent.reasoning_effort` / `/effort` wins over `provider.anthropic.effort`.
        let payload = adaptive_effort_payload(
            models::anthropic::CLAUDE_OPUS_5_5,
            Some(ReasoningEffortLevel::High),
            Some(ReasoningEffortLevel::Low),
        );
        assert_eq!(payload["output_config"]["effort"], "high");
    }

    #[test]
    fn test_convert_to_anthropic_format_honors_explicit_configured_effort() {
        use vtcode_config::types::ReasoningEffortLevel;

        let payload =
            adaptive_effort_payload(models::anthropic::CLAUDE_OPUS_5_5, None, Some(ReasoningEffortLevel::XHigh));
        assert_eq!(payload["output_config"]["effort"], "xhigh");

        let payload = adaptive_effort_payload(models::CLAUDE_SONNET_5, None, Some(ReasoningEffortLevel::XHigh));
        assert_eq!(payload["output_config"]["effort"], "xhigh");
    }

    fn convert_ending_on_assistant(model: &str) -> Vec<Value> {
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![
                Message::user("start the task".to_string()),
                Message::assistant("partial answer".to_string()),
            ]
            .into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");
        payload["messages"].as_array().expect("messages array").clone()
    }

    fn assert_trailing_assistant_is_followed_by_user_sentinel(model: &str) {
        let messages = convert_ending_on_assistant(model);

        assert_eq!(messages.len(), 3, "model {model}: {messages:?}");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "user", "model {model} must never receive a trailing assistant turn");
        assert_eq!(messages[2]["content"][0]["text"], "[Continue]");
    }

    #[test]
    fn test_convert_to_anthropic_format_never_ends_on_assistant_for_sonnet_5() {
        assert_trailing_assistant_is_followed_by_user_sentinel(models::CLAUDE_SONNET_5);
    }

    #[test]
    fn test_convert_to_anthropic_format_never_ends_on_assistant_for_opus_5_5() {
        assert_trailing_assistant_is_followed_by_user_sentinel(models::anthropic::CLAUDE_OPUS_5_5);
    }

    #[test]
    fn test_convert_to_anthropic_format_never_ends_on_assistant_for_unknown_model() {
        assert_trailing_assistant_is_followed_by_user_sentinel("claude-unlisted-model");
    }

    #[test]
    fn test_convert_to_anthropic_format_never_ends_on_assistant_for_claude_4_6_and_later() {
        for model in ["claude-opus-4-8", "claude-opus-4-6", "claude-sonnet-4-6"] {
            assert_trailing_assistant_is_followed_by_user_sentinel(model);
        }
    }

    #[test]
    fn test_convert_to_anthropic_format_keeps_trailing_assistant_for_prefill_backends() {
        for model in [
            models::minimax::MINIMAX_M3,
            "claude-haiku-4-5",
            "claude-sonnet-4-5-20250929",
        ] {
            let messages = convert_ending_on_assistant(model);

            assert_eq!(messages.len(), 2, "model {model}: {messages:?}");
            assert_eq!(messages[1]["role"], "assistant", "model {model} continues from the trailing turn");
            assert_eq!(messages[1]["content"][0]["text"], "partial answer");
        }
    }

    #[test]
    fn test_convert_to_anthropic_format_includes_native_memory_tool() {
        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("remember my preferred test runner".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition {
                tool_type: "memory_20250818".to_string(),
                function: None,
                allowed_callers: None,
                input_examples: None,
                web_search: None,
                hosted_tool_config: None,
                shell: None,
                grammar: None,
                strict: None,
                defer_loading: None,
                namespace: None,
                advisor: None,
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
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["type"], "memory_20250818");
        assert_eq!(payload["tools"][0]["name"], "memory");
    }

    #[test]
    fn test_convert_to_anthropic_format_preserves_function_allowed_callers() {
        let mut tool = ToolDefinition::function(
            "get_weather".to_string(),
            "Get weather for a city".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        );
        tool.allowed_callers = Some(vec!["code_execution_20250825".to_string()]);

        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("find warmest city".to_string())].into(),
            tools: Some(Arc::new(vec![tool])),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["allowed_callers"], json!(["code_execution_20250825"]));
    }

    #[test]
    fn test_convert_to_anthropic_format_preserves_tool_examples_and_strict() {
        let tool = ToolDefinition::function(
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
        .with_input_examples(vec![json!({
            "input": "Weather in Paris",
            "tool_use": {
                "city": "Paris"
            }
        })]);

        let request = LLMRequest {
            model: models::CLAUDE_SONNET_5.to_string(),
            messages: vec![Message::user("find warmest city".to_string())].into(),
            tools: Some(Arc::new(vec![tool])),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };

        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");

        assert_eq!(payload["tools"][0]["strict"], json!(true));
        assert_eq!(
            payload["tools"][0]["input_examples"],
            json!([{
                "input": "Weather in Paris",
                "tool_use": {
                    "city": "Paris"
                }
            }])
        );
    }
}

#[cfg(test)]
mod block_order_round_trip_tests {
    use crate::provider::{LLMResponse, Message};
    use crate::providers::anthropic::request_builder::{RequestBuilderContext, convert_to_anthropic_format};
    use crate::providers::anthropic::response_parser::parse_response;
    use serde_json::{Value, json};
    use vtcode_config::constants::models;
    use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

    /// Mirror of the runloop: an assistant turn stored from a response.
    fn assistant_message(response: LLMResponse) -> Message {
        let details = response
            .reasoning_details
            .map(|details| details.into_iter().map(Value::String).collect());
        let content = response.content.unwrap_or_default();
        match response.tool_calls {
            Some(tool_calls) => Message::assistant_with_tools_and_reasoning(content, tool_calls, details),
            None => Message::assistant(content).with_reasoning_details(details),
        }
    }

    fn replayed_assistant_content(history: Vec<Message>) -> Vec<Value> {
        let request = crate::provider::LLMRequest {
            model: models::anthropic::CLAUDE_OPUS_5_5.to_string(),
            messages: history.into(),
            ..Default::default()
        };
        let cache_settings = AnthropicPromptCacheSettings::default();
        let anthropic_config = AnthropicConfig::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &cache_settings,
            anthropic_config: &anthropic_config,
            model: models::anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: false,
        };
        let payload = convert_to_anthropic_format(&request, &ctx).expect("payload conversion");
        payload["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message["role"] == "assistant")
            .expect("assistant message")["content"]
            .as_array()
            .expect("assistant content")
            .clone()
    }

    fn interleaved_content() -> Value {
        json!([
            { "type": "thinking", "thinking": "", "signature": "sig-1" },
            { "type": "text", "text": "Reading the parser first." },
            { "type": "thinking", "thinking": "Found the entry point.", "signature": "sig-2" },
            { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "src/parser.rs" } }
        ])
    }

    fn history_with(assistant: Message) -> Vec<Message> {
        vec![
            Message::user("fix the parser".to_string()),
            assistant,
            Message::tool_response("toolu_1".to_string(), "fn parse() {}".to_string()),
        ]
    }

    #[test]
    fn interleaved_response_replays_blocks_in_received_order() {
        let response = parse_response(
            json!({ "content": interleaved_content(), "stop_reason": "tool_use" }),
            models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        )
        .expect("parse");

        let content = replayed_assistant_content(history_with(assistant_message(response)));

        assert_eq!(content, interleaved_content().as_array().expect("array").clone());
    }

    #[test]
    fn fixed_order_response_stores_no_block_order_record() {
        let response = parse_response(
            json!({
                "content": [
                    { "type": "thinking", "thinking": "", "signature": "sig-1" },
                    { "type": "text", "text": "Reading." },
                    { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {} }
                ],
                "stop_reason": "tool_use"
            }),
            models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        )
        .expect("parse");

        assert!(
            !response
                .reasoning_details
                .as_ref()
                .expect("details")
                .iter()
                .any(|detail| detail.contains("anthropic_block_order"))
        );
    }

    #[test]
    fn mid_output_fallback_drops_declined_thinking_and_tool_use_before_boundary() {
        let response = parse_response(
            json!({
                "content": [
                    { "type": "thinking", "thinking": "Refused model reasoning.", "signature": "sig-1" },
                    { "type": "text", "text": "Checking. " },
                    { "type": "tool_use", "id": "toolu_declined", "name": "read_file", "input": {} },
                    { "type": "fallback", "from": { "model": "claude-fable-5-1" }, "to": { "model": "claude-opus-4-8" } },
                    { "type": "text", "text": "Here is the answer." }
                ],
                "stop_reason": "end_turn"
            }),
            models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        )
        .expect("parse");

        assert!(response.tool_calls.is_none());
        assert!(response.reasoning.is_none());
        assert_eq!(response.content.as_deref(), Some("Checking. Here is the answer."));
        let details: Vec<Value> = response
            .reasoning_details
            .clone()
            .expect("details")
            .iter()
            .map(|detail| serde_json::from_str(detail).expect("detail json"))
            .collect();
        assert!(details.iter().all(|detail| detail["type"] != "thinking"));
        assert!(details.iter().any(|detail| {
            detail["type"] == "fallback"
                && detail["from"]["model"] == "claude-fable-5-1"
                && detail["to"]["model"] == "claude-opus-4-8"
        }));

        let content = replayed_assistant_content(vec![
            Message::user("fix the parser".to_string()),
            assistant_message(response),
            Message::user("thanks".to_string()),
        ]);
        assert_eq!(
            content,
            vec![
                json!({ "type": "text", "text": "Checking. " }),
                json!({ "type": "text", "text": "Here is the answer." }),
            ]
        );
    }

    #[test]
    fn edited_assistant_text_falls_back_to_default_order() {
        let response = parse_response(
            json!({ "content": interleaved_content(), "stop_reason": "tool_use" }),
            models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        )
        .expect("parse");
        let mut assistant = assistant_message(response);
        assistant.content = crate::provider::MessageContent::Text("rewritten by the runtime".to_string());

        let content = replayed_assistant_content(history_with(assistant));
        let types: Vec<&str> = content.iter().filter_map(|block| block["type"].as_str()).collect();

        assert_eq!(types, ["thinking", "thinking", "text", "tool_use"]);
        assert_eq!(content[2]["text"], "rewritten by the runtime");
    }
}
