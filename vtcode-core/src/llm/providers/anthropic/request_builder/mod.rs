//! Request building for Anthropic Claude API.

mod messages;
mod system;
mod thinking;
mod tools;

use crate::config::constants::models;
use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::{LLMError, LLMRequest};
use crate::llm::providers::anthropic_types::{
    AnthropicOutputConfig, AnthropicRequest, CacheControl,
};
use serde_json::Value;

use super::capabilities::supports_effort;
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

    let max_breakpoints = if ctx.prompt_cache_enabled {
        ctx.prompt_cache_settings.max_breakpoints as usize
    } else {
        0
    };
    let mut breakpoints_remaining = max_breakpoints;

    let tools_breakpoints_before = breakpoints_remaining;
    let tools = build_tools(request, &tools_cache_control, &mut breakpoints_remaining);
    let tools_breakpoints_used = tools_breakpoints_before.saturating_sub(breakpoints_remaining);

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

    if let Some(settings) = &request.coding_agent_settings
        && settings.long_context_optimization
        && messages_to_process.len() > 1
    {
        hoist_largest_user_message(&mut messages_to_process);
    }

    let messages_breakpoints_before = breakpoints_remaining;
    let messages = build_messages(
        request,
        &messages_to_process,
        &messages_cache_control,
        ctx.prompt_cache_settings,
        &mut breakpoints_remaining,
    )?;
    let messages_breakpoints_used =
        messages_breakpoints_before.saturating_sub(breakpoints_remaining);
    let explicit_breakpoints_used = max_breakpoints.saturating_sub(breakpoints_remaining);

    let (thinking_val, reasoning_val) =
        build_thinking_config(request, ctx.anthropic_config, ctx.model);

    let tools = append_structured_output_tool(request, tools, ctx.model);

    let final_tool_choice = build_tool_choice(request, &thinking_val);

    let resolved_model = if request.model.trim().is_empty() {
        ctx.model
    } else {
        request.model.as_str()
    };
    let adaptive_effort =
        if resolved_model == models::anthropic::CLAUDE_OPUS_4_6 && request.effort.is_none() {
            request
                .reasoning_effort
                .map(|effort| effort_from_reasoning_for_adaptive(effort).to_string())
        } else {
            None
        };
    let effort_value = if supports_effort(resolved_model, ctx.model) {
        request
            .effort
            .as_ref()
            .map(|effort| effort.to_ascii_lowercase())
            .or_else(|| {
                adaptive_effort
                    .as_ref()
                    .map(|effort| effort.to_ascii_lowercase())
            })
            .or_else(|| {
                let eff = ctx.anthropic_config.effort.as_str();
                if eff.eq_ignore_ascii_case("high") {
                    None
                } else {
                    Some(eff.to_ascii_lowercase())
                }
            })
    } else {
        None
    };
    let output_config = effort_value.map(|effort| AnthropicOutputConfig { effort });

    let effective_temperature = if thinking_val.is_some() {
        None
    } else {
        request.temperature
    };

    let top_level_cache_control =
        if ctx.prompt_cache_enabled && explicit_breakpoints_used < max_breakpoints {
            let ttl = if messages_breakpoints_used > 0 {
                messages_ttl
            } else if breakpoints_used > 0 || tools_breakpoints_used > 0 {
                tools_ttl
            } else {
                messages_ttl
            };

            Some(CacheControl {
                control_type: "ephemeral".to_string(),
                ttl: Some(ttl.to_string()),
            })
        } else {
            None
        };

    let anthropic_request = AnthropicRequest {
        model: request.model.clone(),
        max_tokens: request
            .max_tokens
            .unwrap_or(if thinking_val.is_some() { 16000 } else { 4096 }),
        cache_control: top_level_cache_control,
        messages,
        system: system_value,
        temperature: effective_temperature,
        tools,
        tool_choice: final_tool_choice,
        thinking: thinking_val,
        reasoning: reasoning_val,
        output_config,
        context_management: request.context_management.clone(),
        stream: request.stream,
    };

    serde_json::to_value(anthropic_request).map_err(|e| LLMError::Provider {
        message: format!("Serialization error: {}", e),
        metadata: None,
    })
}

fn effort_from_reasoning_for_adaptive(effort: ReasoningEffortLevel) -> &'static str {
    match effort {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => {
            "low"
        }
        ReasoningEffortLevel::Medium => "medium",
        ReasoningEffortLevel::High => "high",
        ReasoningEffortLevel::XHigh => "max",
    }
}
