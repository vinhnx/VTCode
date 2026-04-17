//! Request building for Anthropic Claude API.

mod messages;
mod system;
mod thinking;
mod tools;

use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::{LLMError, LLMRequest, PromptCacheProfile};
use crate::llm::providers::anthropic_types::{
    AnthropicOutputConfig, AnthropicOutputFormat, AnthropicRequest, AnthropicTaskBudget,
    CacheControl,
};
use serde_json::Value;

use super::capabilities::{
    default_effort_for_model, effort_allowed_for_model, resolve_model_name,
    supports_adaptive_thinking, supports_effort, supports_task_budget,
};
use super::prompt_cache::{get_messages_cache_ttl, get_tools_cache_ttl};
use messages::{build_messages, hoist_largest_user_message};
use system::{SystemPromptBuildResult, build_system_prompt};
use thinking::build_thinking_config;
use tools::{build_tool_choice, build_tools};

#[cfg(test)]
pub use messages::tool_result_blocks;

pub struct RequestBuilderContext<'a> {
    pub prompt_cache_enabled: bool,
    pub prompt_cache_settings: &'a AnthropicPromptCacheSettings,
    pub anthropic_config: &'a AnthropicConfig,
    pub model: &'a str,
}

fn resolve_messages_ttl(request: &LLMRequest, ctx: &RequestBuilderContext<'_>) -> &'static str {
    if !ctx.prompt_cache_enabled {
        return "5m";
    }

    match request.prompt_cache_profile {
        Some(PromptCacheProfile::BudgetContinuation) => "1h",
        None => get_messages_cache_ttl(ctx.prompt_cache_settings),
    }
}

pub fn convert_to_anthropic_format(
    request: &LLMRequest,
    ctx: &RequestBuilderContext,
) -> Result<Value, LLMError> {
    let resolved_model = resolve_model_name(&request.model, ctx.model);
    let tools_ttl = if ctx.prompt_cache_enabled {
        get_tools_cache_ttl(ctx.prompt_cache_settings)
    } else {
        "5m"
    };

    let messages_ttl = resolve_messages_ttl(request, ctx);

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
    let tools = build_tools(request, &tools_cache_control, &mut breakpoints_remaining)?;
    let tools_breakpoints_used = tools_breakpoints_before.saturating_sub(breakpoints_remaining);

    let SystemPromptBuildResult {
        system_value,
        breakpoints_used,
        has_uncached_runtime_context,
    } = build_system_prompt(request, &system_cache_control, breakpoints_remaining);
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

    let final_tool_choice = build_tool_choice(request, &thinking_val);

    let adaptive_effort =
        if supports_adaptive_thinking(resolved_model, ctx.model) && request.effort.is_none() {
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
                thinking_val.as_ref().and_then(|_| {
                    let configured_effort = ctx.anthropic_config.effort.to_ascii_lowercase();
                    if effort_allowed_for_model(resolved_model, ctx.model, &configured_effort) {
                        Some(configured_effort)
                    } else {
                        default_effort_for_model(resolved_model, ctx.model).map(str::to_string)
                    }
                })
            })
    } else {
        None
    };
    let task_budget = if supports_task_budget(resolved_model, ctx.model) {
        ctx.anthropic_config
            .task_budget_tokens
            .map(|total| AnthropicTaskBudget {
                budget_type: "tokens".to_string(),
                total,
            })
    } else {
        None
    };
    let output_format =
        request
            .output_format
            .as_ref()
            .map(|schema| AnthropicOutputFormat::JsonSchema {
                schema: schema.clone(),
            });
    let output_config =
        if effort_value.is_some() || task_budget.is_some() || output_format.is_some() {
            Some(AnthropicOutputConfig {
                effort: effort_value,
                task_budget,
                format: output_format,
            })
        } else {
            None
        };

    let effective_temperature =
        if thinking_val.is_some() || supports_adaptive_thinking(resolved_model, ctx.model) {
            None
        } else {
            request.temperature
        };

    let top_level_cache_control = if ctx.prompt_cache_enabled
        && explicit_breakpoints_used < max_breakpoints
        && !has_uncached_runtime_context
    {
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
        model: resolved_model.to_string(),
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
        ReasoningEffortLevel::XHigh => "xhigh",
        ReasoningEffortLevel::Max => "max",
    }
}
