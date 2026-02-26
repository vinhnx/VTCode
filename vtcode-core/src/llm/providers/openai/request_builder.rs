//! Chat Completions request builder for OpenAI-compatible APIs.
//!
//! Keeps JSON shaping for chat payloads out of the main provider.

use crate::config::models::Provider as ModelProvider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::common::serialize_message_content_openai;
use crate::llm::rig_adapter::reasoning_parameters_for;
use serde_json::{Value, json};
use std::collections::HashSet;

use super::responses_api::build_standard_responses_payload;
use super::tool_serialization;
use super::types::MAX_COMPLETION_TOKENS_FIELD;

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub base_url: &'a str,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub supports_prompt_cache_key: bool,
    pub prompt_cache_key: Option<&'a str>,
}

pub(crate) struct ResponsesRequestContext<'a> {
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub supports_reasoning_effort: bool,
    pub supports_reasoning: bool,
    pub is_responses_api_model: bool,
    pub supports_prompt_cache_key: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub prompt_cache_retention: Option<&'a str>,
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
            "content": serialize_message_content_openai(&msg.content)
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
    let max_tokens_field = if !is_native_openai {
        "max_tokens"
    } else {
        MAX_COMPLETION_TOKENS_FIELD
    };

    if let Some(max_tokens) = request.max_tokens {
        openai_request[max_tokens_field] = json!(max_tokens);
    }

    if let Some(temperature) = request.temperature
        && ctx.supports_temperature
    {
        openai_request["temperature"] = json!(temperature);
    }

    if ctx.supports_prompt_cache_key
        && let Some(prompt_cache_key) = ctx.prompt_cache_key
    {
        let trimmed = prompt_cache_key.trim();
        if !trimmed.is_empty() {
            openai_request["prompt_cache_key"] = json!(trimmed);
        }
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

pub(crate) fn build_responses_request(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    let responses_payload = build_standard_responses_payload(request)?;

    if responses_payload.input.is_empty() {
        let formatted_error =
            error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
        return Err(provider::LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let mut openai_request = json!({
        "model": request.model,
        "input": responses_payload.input,
        "stream": request.stream,
    });

    // 'output_types' is part of the GPT-5 Responses API spec
    openai_request["output_types"] = json!(["message", "tool_call"]);

    if let Some(instructions) = responses_payload.instructions
        && !instructions.trim().is_empty()
    {
        openai_request["instructions"] = json!(instructions);
    }

    let mut sampling_parameters = json!({});
    let mut has_sampling = false;

    if let Some(temperature) = request.temperature
        && ctx.supports_temperature
    {
        sampling_parameters["temperature"] = json!(temperature);
        has_sampling = true;
    }

    if let Some(top_p) = request.top_p {
        sampling_parameters["top_p"] = json!(top_p);
        has_sampling = true;
    }

    if let Some(presence_penalty) = request.presence_penalty {
        sampling_parameters["presence_penalty"] = json!(presence_penalty);
        has_sampling = true;
    }

    if let Some(frequency_penalty) = request.frequency_penalty {
        sampling_parameters["frequency_penalty"] = json!(frequency_penalty);
        has_sampling = true;
    }

    if has_sampling {
        openai_request["sampling_parameters"] = sampling_parameters;
    }

    if ctx.supports_tools
        && let Some(tools) = &request.tools
        && let Some(serialized) = tool_serialization::serialize_tools_for_responses(tools)
    {
        openai_request["tools"] = serialized;

        // Check if any tools are custom types - if so, disable parallel tool calls
        // as per GPT-5 specification: "custom tool type does NOT support parallel tool calling"
        let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
        if has_custom_tool {
            // Override parallel tool calls to false if custom tools are present
            openai_request["parallel_tool_calls"] = Value::Bool(false);
        }

        // Only add tool_choice when tools are present
        if let Some(tool_choice) = &request.tool_choice {
            openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        // Only set parallel tool calls if not overridden due to custom tools
        if let Some(parallel) = request.parallel_tool_calls
            && openai_request.get("parallel_tool_calls").is_none()
        {
            openai_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        // Only add parallel_tool_config when tools are present
        if ctx.supports_parallel_tool_config
            && let Some(config) = &request.parallel_tool_config
            && let Ok(config_value) = serde_json::to_value(config)
        {
            openai_request["parallel_tool_config"] = config_value;
        }
    }

    if ctx.supports_reasoning_effort {
        if let Some(effort) = request.reasoning_effort {
            if let Some(payload) = reasoning_parameters_for(ModelProvider::OpenAI, effort) {
                openai_request["reasoning"] = payload;
            } else {
                openai_request["reasoning"] = json!({ "effort": effort.as_str() });
            }
        } else if openai_request.get("reasoning").is_none() {
            // Use the default reasoning effort level (medium) for native OpenAI models
            let default_effort = ReasoningEffortLevel::default().as_str();
            openai_request["reasoning"] = json!({ "effort": default_effort });
        }
    }

    // Enable reasoning summaries if supported (OpenAI GPT-5 only)
    if ctx.supports_reasoning
        && let Some(map) = openai_request.as_object_mut()
    {
        let reasoning_value = map.entry("reasoning").or_insert(json!({}));
        if let Some(reasoning_obj) = reasoning_value.as_object_mut()
            && !reasoning_obj.contains_key("summary")
        {
            reasoning_obj.insert("summary".to_string(), json!("auto"));
        }
    }

    // Add text formatting options for GPT-5 and compatible models, including verbosity and grammar
    let mut text_format = json!({});
    let mut has_format_options = false;

    if let Some(verbosity) = request.verbosity {
        text_format["verbosity"] = json!(verbosity.as_str());
        has_format_options = true;
    }

    // Add grammar constraint if tools include grammar definitions
    if let Some(ref tools) = request.tools {
        let grammar_tools: Vec<&provider::ToolDefinition> = tools
            .iter()
            .filter(|tool| tool.tool_type == "grammar")
            .collect();

        if !grammar_tools.is_empty() {
            // Use the first grammar definition found
            if let Some(grammar_tool) = grammar_tools.first()
                && let Some(ref grammar) = grammar_tool.grammar
            {
                text_format["format"] = json!({
                    "type": "grammar",
                    "syntax": grammar.syntax,
                    "definition": grammar.definition
                });
                has_format_options = true;
            }
        }
    }

    // Set default verbosity for GPT-5.2+ models if no format options specified
    if !has_format_options
        && (request.model.starts_with("gpt-5.2") || request.model.starts_with("gpt-5.3"))
    {
        text_format["verbosity"] = json!("medium");
        has_format_options = true;
    }

    if has_format_options {
        openai_request["text"] = text_format;
    }

    if ctx.supports_prompt_cache_key
        && let Some(prompt_cache_key) = ctx.prompt_cache_key
    {
        let trimmed = prompt_cache_key.trim();
        if !trimmed.is_empty() {
            openai_request["prompt_cache_key"] = json!(trimmed);
        }
    }

    // If configured, include the `prompt_cache_retention` value in the Responses API
    // request. This allows the user to extend the server-side prompt cache window
    // (e.g., "24h") to increase cache reuse and reduce cost/latency on GPT-5.
    // Only include prompt_cache_retention when both configured and when the selected
    // model uses the OpenAI Responses API.
    if ctx.is_responses_api_model
        && let Some(retention) = ctx.prompt_cache_retention
        && !retention.trim().is_empty()
    {
        openai_request["prompt_cache_retention"] = json!(retention);
    }

    Ok(openai_request)
}
