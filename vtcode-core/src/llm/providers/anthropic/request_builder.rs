//! Request building for Anthropic Claude API
//!
//! Converts internal LLMRequest format to Anthropic API JSON format,
//! handling:
//! - Message conversion with role mapping
//! - Tool definitions with cache control
//! - System prompt injection and caching
//! - Extended thinking configuration
//! - Structured output tool generation
//! - Prefill and character reinforcement

use crate::config::constants::env_vars;
use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMRequest, MessageRole};
use crate::llm::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicFunctionTool, AnthropicMessage, AnthropicOutputConfig,
    AnthropicRequest, AnthropicTool, AnthropicToolSearchTool, CacheControl, ThinkingConfig,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use serde_json::{Value, json};
use std::env;

use super::capabilities::{supports_reasoning_effort, supports_structured_output};
use super::prompt_cache::{get_messages_cache_ttl, get_tools_cache_ttl};

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

    let mut tools = tools;
    if let Some(schema) = &request.output_format {
        if supports_structured_output(&request.model, ctx.model) {
            let structured_tool = AnthropicTool::Function(AnthropicFunctionTool {
                name: "structured_output".to_string(),
                description: "Forces Claude to respond in a specific JSON format according to the provided schema".to_string(),
                input_schema: schema.clone(),
                cache_control: None,
                defer_loading: None,
            });

            if let Some(ref mut tools_vec) = tools {
                tools_vec.push(structured_tool);
            } else {
                tools = Some(vec![structured_tool]);
            }
        }
    }

    let final_tool_choice = build_tool_choice(request, &thinking_val);

    let effort_value = request.effort.as_ref().or({
        let eff = &ctx.anthropic_config.effort;
        if eff == "high" {
            None
        } else {
            Some(eff)
        }
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
        max_tokens: request.max_tokens.unwrap_or(if thinking_val.is_some() {
            16000
        } else {
            4096
        }),
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

fn build_tools(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: &mut usize,
) -> Option<Vec<AnthropicTool>> {
    let request_tools = request.tools.as_ref()?;
    if request_tools.is_empty() {
        return None;
    }

    let mut built_tools: Vec<AnthropicTool> = request_tools
        .iter()
        .filter_map(|tool| {
            if tool.is_tool_search() {
                let func = tool.function.as_ref()?;
                return Some(AnthropicTool::ToolSearch(AnthropicToolSearchTool {
                    tool_type: tool.tool_type.clone(),
                    name: func.name.clone(),
                }));
            }

            let func = tool.function.as_ref()?;
            Some(AnthropicTool::Function(AnthropicFunctionTool {
                name: func.name.clone(),
                description: func.description.clone(),
                input_schema: func.parameters.clone(),
                cache_control: None,
                defer_loading: tool.defer_loading,
            }))
        })
        .collect();

    if *breakpoints_remaining > 0 {
        if let Some(cc) = cache_control.as_ref() {
            if let Some(last_tool) = built_tools.last_mut() {
                if let AnthropicTool::Function(func_tool) = last_tool {
                    func_tool.cache_control = Some(cc.clone());
                }
                *breakpoints_remaining -= 1;
            }
        }
    }

    if built_tools.is_empty() {
        None
    } else {
        Some(built_tools)
    }
}

fn build_system_prompt(
    request: &LLMRequest,
    cache_control: &Option<CacheControl>,
    breakpoints_remaining: usize,
) -> (Option<Value>, usize) {
    let mut final_system_prompt = request.system_prompt.clone().unwrap_or_default();

    if let Some(settings) = &request.coding_agent_settings {
        if let Some(role) = &settings.role_specialization {
            if final_system_prompt.is_empty() {
                final_system_prompt = format!("You are {}.", role);
            } else {
                final_system_prompt = format!("You are {}.\n{}", role, final_system_prompt);
            }
        }
        if settings.force_xml_tags {
            final_system_prompt
                .push_str("\nPlease use XML tags to structure your response for consistency.");
        }
        if settings.allow_uncertainty {
            final_system_prompt.push_str("\nIf you are unsure or the information is missing, explicitly state 'I don't know' or 'I am unsure'.");
        }
        if settings.strict_grounding {
            final_system_prompt.push_str("\nOnly use information strictly from the provided documents. Do not rely on external knowledge.");
        }
        if settings.force_quote_grounding {
            final_system_prompt.push_str("\nFind quotes from the provided documents that are relevant to the user request. Place these in <quotes> tags first, and then use them to justify your response.");
        }
        if settings.enforce_structured_thought {
            final_system_prompt.push_str("\nBefore providing your final answer, think through the problem in <thinking> tags. Then, provide your final response in <answer> tags.");
        }
    }

    if final_system_prompt.is_empty() {
        return (None, 0);
    }

    let should_cache = cache_control.is_some() && breakpoints_remaining > 0;

    if should_cache {
        if let Some(cc) = cache_control.as_ref() {
            let block = json!({
                "type": "text",
                "text": final_system_prompt.trim(),
                "cache_control": cc
            });
            return (Some(Value::Array(vec![block])), 1);
        }
    }

    (Some(Value::String(final_system_prompt.trim().to_string())), 0)
}

fn hoist_largest_user_message(messages: &mut Vec<crate::llm::provider::Message>) {
    let mut max_len = 0;
    let mut max_idx = None;

    for (i, msg) in messages.iter().enumerate() {
        if msg.role == MessageRole::User {
            let len = msg.content.as_text().len();
            if len > max_len {
                max_len = len;
                max_idx = Some(i);
            }
        }
    }

    if let Some(idx) = max_idx {
        if idx > 0 {
            let msg = messages.remove(idx);
            messages.insert(0, msg);
        }
    }
}

fn build_messages(
    request: &LLMRequest,
    messages_to_process: &[crate::llm::provider::Message],
    messages_cache_control: &Option<CacheControl>,
    prompt_cache_settings: &AnthropicPromptCacheSettings,
    breakpoints_remaining: &mut usize,
) -> Result<Vec<AnthropicMessage>, LLMError> {
    let mut messages = Vec::with_capacity(messages_to_process.len());

    for msg in messages_to_process {
        if msg.role == MessageRole::System {
            continue;
        }

        let mut blocks = Vec::new();
        let content_text = msg.content.as_text();

        match msg.role {
            MessageRole::Assistant => {
                blocks.extend(build_reasoning_blocks(msg));

                if !msg.content.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: content_text.to_string(),
                        citations: None,
                        cache_control: None,
                    });
                }

                blocks.extend(build_tool_use_blocks(msg));

                if blocks.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: String::new(),
                        citations: None,
                        cache_control: None,
                    });
                }
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: blocks,
                });
            }
            MessageRole::Tool => {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    let tool_content_blocks = tool_result_blocks(&content_text);
                    let content_val = if tool_content_blocks.len() == 1
                        && tool_content_blocks[0]["type"] == "text"
                    {
                        json!(tool_content_blocks[0]["text"])
                    } else {
                        json!(tool_content_blocks)
                    };

                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::ToolResult {
                            tool_use_id: tool_call_id.clone(),
                            content: content_val,
                            is_error: None,
                            cache_control: None,
                        }],
                    });
                } else if !msg.content.is_empty() {
                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::Text {
                            text: content_text.to_string(),
                            citations: None,
                            cache_control: None,
                        }],
                    });
                }
            }
            _ => {
                if msg.content.is_empty() {
                    continue;
                }

                let mut cache_ctrl = None;
                let should_cache = msg.role == MessageRole::User
                    && prompt_cache_settings.cache_user_messages
                    && *breakpoints_remaining > 0
                    && content_text.len() >= prompt_cache_settings.min_message_length_for_cache;

                if should_cache {
                    if let Some(cc) = messages_cache_control.as_ref() {
                        cache_ctrl = Some(cc.clone());
                        *breakpoints_remaining -= 1;
                    }
                }

                messages.push(AnthropicMessage {
                    role: msg.role.as_anthropic_str().to_string(),
                    content: vec![AnthropicContentBlock::Text {
                        text: content_text.to_string(),
                        citations: None,
                        cache_control: cache_ctrl,
                    }],
                });
            }
        }
    }

    add_prefill_message(request, &mut messages);

    if messages.is_empty() {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "No convertible messages for Anthropic request",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    Ok(messages)
}

fn build_reasoning_blocks(msg: &crate::llm::provider::Message) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::new();

    if let Some(details) = &msg.reasoning_details {
        for detail in details {
            if detail.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                let thinking = detail
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let signature = detail
                    .get("signature")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if !thinking.is_empty() && !signature.is_empty() {
                    blocks.push(AnthropicContentBlock::Thinking {
                        thinking,
                        signature,
                        cache_control: None,
                    });
                }
            } else if detail.get("type").and_then(|t| t.as_str()) == Some("redacted_thinking") {
                let data = detail
                    .get("data")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                blocks.push(AnthropicContentBlock::RedactedThinking {
                    data,
                    cache_control: None,
                });
            }
        }
    }

    blocks
}

fn build_tool_use_blocks(msg: &crate::llm::provider::Message) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::new();

    if let Some(tool_calls) = &msg.tool_calls {
        for call in tool_calls {
            if let Some(ref func) = call.function {
                let args: Value =
                    serde_json::from_str(&func.arguments).unwrap_or_else(|_| json!({}));
                blocks.push(AnthropicContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: func.name.clone(),
                    input: args,
                    cache_control: None,
                });
            }
        }
    }

    blocks
}

fn add_prefill_message(request: &LLMRequest, messages: &mut Vec<AnthropicMessage>) {
    let mut prefill_text = String::new();

    if let Some(settings) = &request.coding_agent_settings {
        if settings.prefill_thought {
            prefill_text.push_str("<thought>");
        }
    }

    if let Some(request_prefill) = &request.prefill {
        if !prefill_text.is_empty() && !request_prefill.is_empty() {
            prefill_text.push(' ');
        }
        prefill_text.push_str(request_prefill);
    }

    if !prefill_text.is_empty() {
        let mut text = prefill_text;
        if request.character_reinforcement {
            if let Some(name) = &request.character_name {
                let tag = format!("[{}]", name);
                if !text.contains(&tag) {
                    text = format!("{} {}", tag, text).trim().to_string();
                }
            }
        }
        if !text.is_empty() {
            messages.push(AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text,
                    citations: None,
                    cache_control: None,
                }],
            });
        }
    } else if request.character_reinforcement {
        if let Some(name) = &request.character_name {
            messages.push(AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text: format!("[{}]", name),
                    citations: None,
                    cache_control: None,
                }],
            });
        }
    }
}

pub fn tool_result_blocks(content: &str) -> Vec<Value> {
    if content.trim().is_empty() {
        return vec![json!({"type": "text", "text": ""})];
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(content) {
        match parsed {
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            Value::Array(items) => {
                let mut blocks = Vec::new();
                for item in items {
                    if let Some(text) = item.as_str() {
                        blocks.push(json!({"type": "text", "text": text}));
                    } else {
                        blocks.push(json!({"type": "json", "json": item}));
                    }
                }
                if blocks.is_empty() {
                    vec![json!({"type": "json", "json": Value::Array(vec![])})]
                } else {
                    blocks
                }
            }
            other => vec![json!({"type": "json", "json": other})],
        }
    } else {
        vec![json!({"type": "text", "text": content})]
    }
}

fn build_thinking_config(
    request: &LLMRequest,
    anthropic_config: &AnthropicConfig,
    default_model: &str,
) -> (Option<ThinkingConfig>, Option<Value>) {
    let thinking_enabled =
        anthropic_config.extended_thinking_enabled && supports_reasoning_effort(&request.model, default_model);

    if thinking_enabled {
        let max_thinking_tokens: Option<u32> = env::var(env_vars::MAX_THINKING_TOKENS)
            .ok()
            .and_then(|v| v.parse().ok());

        let budget = if let Some(explicit_budget) = request.thinking_budget {
            explicit_budget
        } else if let Some(env_budget) = max_thinking_tokens {
            env_budget
        } else if let Some(effort) = request.reasoning_effort {
            match effort {
                ReasoningEffortLevel::None => 0,
                ReasoningEffortLevel::Minimal => 1024,
                ReasoningEffortLevel::Low => 4096,
                ReasoningEffortLevel::Medium => 8192,
                ReasoningEffortLevel::High => 16384,
                ReasoningEffortLevel::XHigh => 32768,
            }
        } else {
            anthropic_config.interleaved_thinking_budget_tokens
        };

        if budget >= 1024 {
            let max_tokens = request.max_tokens.unwrap_or(16000);
            let effective_budget = budget.min(max_tokens.saturating_sub(100)).max(1024);
            return (
                Some(ThinkingConfig::Enabled {
                    budget_tokens: effective_budget,
                }),
                None,
            );
        }
    } else if let Some(effort) = request.reasoning_effort {
        use crate::config::models::Provider;
        if let Some(payload) = reasoning_parameters_for(Provider::Anthropic, effort) {
            return (None, Some(payload));
        } else {
            return (None, Some(json!({ "effort": effort.as_str() })));
        }
    }

    (None, None)
}

fn build_tool_choice(
    request: &LLMRequest,
    thinking_val: &Option<ThinkingConfig>,
) -> Option<Value> {
    let mut final_tool_choice = request
        .tool_choice
        .as_ref()
        .map(|tc| tc.to_provider_format("anthropic"));

    if thinking_val.is_some() {
        if let Some(ref choice) = final_tool_choice {
            let choice_type = choice.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if choice_type != "auto" && choice_type != "none" && !choice_type.is_empty() {
                final_tool_choice = Some(json!({"type": "auto"}));
            }
        }
    }

    if request.output_format.is_some() && thinking_val.is_none() {
        final_tool_choice = Some(json!({
            "type": "tool",
            "name": "structured_output"
        }));
    }

    final_tool_choice
}
