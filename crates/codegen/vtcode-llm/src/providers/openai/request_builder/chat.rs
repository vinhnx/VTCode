//! Chat Completions request assembly for OpenAI-compatible APIs.
//!
//! Extracted from request_builder.rs beside the pre-existing history,
//! model_policy, and responses submodules. No behavior change: stable and
//! dynamic system-prompt splitting, tool-call ID tracking, and sampling and
//! service-tier shaping are verbatim, and build_chat_request is re-exported
//! from the parent.

use super::history::augment_openai_instructions;
use super::model_policy::{allows_sampling_parameters, default_reasoning_effort_for_model};
use super::{ChatRequestContext, trimmed_non_empty};
use crate::error_display;
use crate::provider;
use crate::providers::common::serialize_message_content_openai_for_model;
use crate::providers::openai::tool_serialization;
use crate::providers::openai::types::MAX_COMPLETION_TOKENS_FIELD;
use crate::providers::shared::split_dynamic_prompt_suffix;
use hashbrown::HashSet;
use serde_json::{Value, json};

pub(crate) fn build_chat_request(
    request: &provider::LLMRequest,
    ctx: &ChatRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    for message in request.messages.iter() {
        if let provider::MessageContent::Parts(parts) = &message.content {
            for part in parts {
                if let provider::ContentPart::File { file_url: Some(_), .. } = part {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Chat Completions does not support file_url inputs; use Responses API or file_id/file_data",
                    );
                    return Err(provider::LLMError::InvalidRequest { message: formatted_error, metadata: None });
                }
            }
        }
    }

    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    let mut active_tool_call_ids: HashSet<String> = HashSet::with_capacity(16);

    // Stable/dynamic split mirrors the Responses path: volatile sections move
    // to a trailing system message so per-turn runtime content stops rewriting
    // the cached leading prefix. Without dynamics this is byte-identical to
    // the legacy single system message.
    let mut trailing_system: Option<String> = None;
    if let Some(system_prompt) = &request.system_prompt {
        let (stable, dynamic) = split_dynamic_prompt_suffix(system_prompt.as_ref());
        if let Some(dynamic) = dynamic.filter(|text| !text.trim().is_empty()) {
            trailing_system = Some(dynamic);
            if !stable.trim().is_empty() {
                let stable = augment_openai_instructions(&request.model, stable);
                messages.push(json!({
                    "role": vtcode_config::constants::message_roles::SYSTEM,
                    "content": stable
                }));
            }
        } else {
            let system_prompt = augment_openai_instructions(&request.model, system_prompt.to_string());
            messages.push(json!({
                "role": vtcode_config::constants::message_roles::SYSTEM,
                "content": system_prompt
            }));
        }
    }

    for msg in request.messages.iter() {
        let role = msg.role.as_openai_str();
        let mut message = json!({
            "role": role,
            "content": serialize_message_content_openai_for_model(msg, &request.model)
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

    if let Some(dynamic) = trailing_system.filter(|text| !text.trim().is_empty()) {
        messages.push(json!({
            "role": vtcode_config::constants::message_roles::SYSTEM,
            "content": dynamic
        }));
    }

    if messages.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No messages provided");
        return Err(provider::LLMError::InvalidRequest { message: formatted_error, metadata: None });
    }

    let mut openai_request = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream
    });
    let effective_reasoning_effort = request
        .reasoning_effort
        .or_else(|| default_reasoning_effort_for_model(&request.model));

    let max_tokens_field = if !ctx.is_native_openai {
        "max_tokens"
    } else {
        MAX_COMPLETION_TOKENS_FIELD
    };

    if let Some(max_tokens) = request.max_tokens {
        openai_request[max_tokens_field] = json!(max_tokens);
    }

    if let Some(temperature) = request.temperature
        && ctx.supports_temperature
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        openai_request["temperature"] = Value::Number(crate::providers::common::float_to_json_number(temperature)?);
    }

    if ctx.supports_temperature && allows_sampling_parameters(&request.model, effective_reasoning_effort) {
        if let Some(top_p) = request.top_p {
            openai_request["top_p"] = Value::Number(crate::providers::common::float_to_json_number(top_p)?);
        }
        if let Some(presence_penalty) = request.presence_penalty {
            openai_request["presence_penalty"] =
                Value::Number(crate::providers::common::float_to_json_number(presence_penalty)?);
        }
        if let Some(frequency_penalty) = request.frequency_penalty {
            openai_request["frequency_penalty"] =
                Value::Number(crate::providers::common::float_to_json_number(frequency_penalty)?);
        }
    }

    if let Some(service_tier) =
        super::resolved_service_tier(&request.model, request.service_tier.as_deref(), ctx.default_service_tier)
    {
        openai_request["service_tier"] = json!(service_tier);
    }

    if let Some(prompt_cache_key) = trimmed_non_empty(ctx.prompt_cache_key) {
        openai_request["prompt_cache_key"] = json!(prompt_cache_key);
    }

    if ctx.supports_tools
        && let Some(tools) = &request.tools
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
            && let Some(parallel) = request.parallel_tool_calls
        {
            openai_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if ctx.supports_parallel_tool_config
            && let Some(config) = &request.parallel_tool_config
            && let Ok(config_value) = serde_json::to_value(config)
        {
            openai_request["parallel_tool_config"] = config_value;
        }
    }

    Ok(openai_request)
}
