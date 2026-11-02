use crate::provider::{LLMRequest, Message, PromptCacheProfile, ToolDefinition};
use crate::providers::anthropic::request_builder::{
    RequestBuilderContext, convert_to_anthropic_format, tool_result_blocks,
};
use serde_json::{Value, json};
use std::sync::Arc;
use vtcode_config::constants::models;
use vtcode_config::core::{AnthropicConfig, AnthropicPromptCacheSettings};

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

fn long_context_payload(model: &str) -> Value {
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::user("short opener".to_string()),
            Message::assistant("Acknowledged.".to_string()),
            Message::user(format!("large pasted document: {}", "x".repeat(512))),
        ]
        .into(),
        coding_agent_settings: Some(Box::new(crate::provider::CodingAgentSettings { long_context_optimization: true })),
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

mod cache_anchors;
mod context_tools;
mod effort_assistant;
mod format;
mod runtime_hoisting;
