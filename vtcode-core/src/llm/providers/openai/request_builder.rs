//! Chat Completions request builder for OpenAI-compatible APIs.
//!
//! Keeps JSON shaping for chat payloads out of the main provider.

use crate::llm::error_display;
use crate::llm::provider as provider;
use serde_json::{Value, json};
use std::collections::HashSet;

use super::tool_serialization;
use super::types::MAX_COMPLETION_TOKENS_FIELD;

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub base_url: &'a str,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
}

pub(crate) fn build_chat_request(
    request: &provider::LLMRequest,
    ctx: &ChatRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    let mut active_tool_call_ids: HashSet<String> = HashSet::with_capacity(16);

    if let Some(system_prompt) = &request.system_prompt {
        messages.push(json!({
            "role": crate::config::constants::message_roles::SYSTEM,
            "content": system_prompt
        }));
    }

    for msg in &request.messages {
        let role = msg.role.as_openai_str();
        let mut message = json!({
            "role": role,
            "content": msg.content
        });
        let mut skip_message = false;

        if msg.role == provider::MessageRole::Assistant
            && let Some(tool_calls) = &msg.tool_calls
            && !tool_calls.is_empty()
        {
            let tool_calls_json: Vec<Value> = tool_calls
                .iter()
                .filter_map(|tc| {
                    tc.function.as_ref().map(|func| {
                        active_tool_call_ids.insert(tc.id.clone());
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "arguments": func.arguments
                            }
                        })
                    })
                })
                .collect();

            message["tool_calls"] = Value::Array(tool_calls_json);
        }

        if msg.role == provider::MessageRole::Tool {
            match &msg.tool_call_id {
                Some(tool_call_id) if active_tool_call_ids.contains(tool_call_id) => {
                    message["tool_call_id"] = Value::String(tool_call_id.clone());
                    active_tool_call_ids.remove(tool_call_id);
                }
                Some(_) | None => {
                    skip_message = true;
                }
            }
        }

        if !skip_message {
            messages.push(message);
        }
    }

    if messages.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No messages provided");
        return Err(provider::LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let mut openai_request = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream
    });

    let is_native_openai = ctx.base_url.contains("api.openai.com");
    let _max_tokens_field = if !is_native_openai {
        "max_tokens"
    } else {
        MAX_COMPLETION_TOKENS_FIELD
    };

    if let Some(temperature) = request.temperature
        && ctx.supports_temperature
    {
        openai_request["temperature"] = json!(temperature);
    }

    if ctx.supports_tools {
        if let Some(tools) = &request.tools
            && let Some(serialized) = tool_serialization::serialize_tools(tools, ctx.model)
        {
            openai_request["tools"] = serialized;

            let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
            if has_custom_tool {
                openai_request["parallel_tool_calls"] = Value::Bool(false);
            }

            if let Some(tool_choice) = &request.tool_choice {
                openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
            }

            if request.parallel_tool_calls.is_some()
                && openai_request.get("parallel_tool_calls").is_none()
            {
                if let Some(parallel) = request.parallel_tool_calls {
                    openai_request["parallel_tool_calls"] = Value::Bool(parallel);
                }
            }

            if ctx.supports_parallel_tool_config {
                if let Some(config) = &request.parallel_tool_config {
                    if let Ok(config_value) = serde_json::to_value(config) {
                        openai_request["parallel_tool_config"] = config_value;
                    }
                }
            }
        }
    }

    Ok(openai_request)
}
