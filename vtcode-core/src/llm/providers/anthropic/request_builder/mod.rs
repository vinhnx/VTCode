//! Request building for Anthropic Claude API.

mod messages;
mod system;
mod thinking;
mod tools;

use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};
use crate::llm::provider::{LLMError, LLMRequest};
use crate::llm::providers::anthropic_types::{
    AnthropicOutputConfig, AnthropicRequest, CacheControl,
};
use serde_json::Value;

use super::prompt_cache::{get_messages_cache_ttl, get_tools_cache_ttl};
use messages::{build_messages, hoist_largest_user_message};
use system::build_system_prompt;
use thinking::build_thinking_config;
use tools::{append_structured_output_tool, build_tool_choice, build_tools};

#[cfg(test)]
pub use messages::tool_result_blocks;

pub struct RequestBuilderContext<'a> {
    pub prompt_cache_enabled: bool,
    pub prompt_cache_settings: &'a AnthropicPromptCacheSettings,
    pub anthropic_config: &'a AnthropicConfig,
    pub model: &'a str,
}

pub fn convert_to_anthropic_format(
    request: &LLMRequest,
    ctx: &RequestBuilderContext,
) -> Result<Value, LLMError> {
    let tools_ttl = if ctx.prompt_cache_enabled {
        get_tools_cache_ttl(ctx.prompt_cache_settings)
    } else {
        "5m"
    };

    let messages_ttl = if ctx.prompt_cache_enabled {
        get_messages_cache_ttl(ctx.prompt_cache_settings)
    } else {
        "5m"
    };

    let tools_cache_control =
        if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_tool_definitions {
            Some(CacheControl {
                control_type: "ephemeral".to_string(),
                ttl: Some(tools_ttl.to_string()),
            })
        } else {
            None
        };

    let system_cache_control =
        if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_system_messages {
            Some(CacheControl {
                control_type: "ephemeral".to_string(),
                ttl: Some(tools_ttl.to_string()),
            })
        } else {
            None
        };

    let mut breakpoints_remaining = if ctx.prompt_cache_enabled {
        ctx.prompt_cache_settings.max_breakpoints as usize
    } else {
        0
    };

    let tools = build_tools(request, &tools_cache_control, &mut breakpoints_remaining);

    let (system_value, breakpoints_used) =
        build_system_prompt(request, &system_cache_control, breakpoints_remaining);
    breakpoints_remaining = breakpoints_remaining.saturating_sub(breakpoints_used);

    let messages_cache_control =
        if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_user_messages {
            Some(CacheControl {
                control_type: "ephemeral".to_string(),
                ttl: Some(messages_ttl.to_string()),
            })
        } else {
            None
        };

    let mut messages_to_process = request.messages.clone();

    if let Some(settings) = &request.coding_agent_settings {
        if settings.long_context_optimization && messages_to_process.len() > 1 {
            hoist_largest_user_message(&mut messages_to_process);
        }
    }

    let messages = build_messages(
        request,
        &messages_to_process,
        &messages_cache_control,
        ctx.prompt_cache_settings,
        &mut breakpoints_remaining,
    )?;

    let (thinking_val, reasoning_val) =
        build_thinking_config(request, ctx.anthropic_config, ctx.model);

    let tools = append_structured_output_tool(request, tools, ctx.model);

    let final_tool_choice = build_tool_choice(request, &thinking_val);

    let effort_value = request.effort.as_ref().or({
        let eff = &ctx.anthropic_config.effort;
        if eff == "high" { None } else { Some(eff) }
    });
    let output_config = effort_value.map(|effort| AnthropicOutputConfig::Effort {
        effort: effort.clone(),
    });

    let effective_temperature = if thinking_val.is_some() {
        None
    } else {
        request.temperature
    };

    let anthropic_request = AnthropicRequest {
        model: request.model.clone(),
        max_tokens: request
            .max_tokens
            .unwrap_or(if thinking_val.is_some() { 16000 } else { 4096 }),
        messages,
        system: system_value,
        temperature: effective_temperature,
        tools,
        tool_choice: final_tool_choice,
        thinking: thinking_val,
        reasoning: reasoning_val,
        output_config,
        stream: request.stream,
    };

    serde_json::to_value(anthropic_request).map_err(|e| LLMError::Provider {
        message: format!("Serialization error: {}", e),
        metadata: None,
    })
}
