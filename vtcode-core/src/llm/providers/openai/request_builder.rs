//! Chat Completions request builder for OpenAI-compatible APIs.
//!
//! Keeps JSON shaping for chat payloads out of the main provider.

use crate::config::constants::models::openai as openai_models;
use crate::config::core::OpenAIHostedShellConfig;
use crate::config::models::Provider as ModelProvider;
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::common::serialize_message_content_openai_for_model;
use crate::llm::rig_adapter::RigProviderCapabilities;
use crate::prompts::system::default_system_prompt;
use hashbrown::HashSet;
use serde_json::{Value, json};

use super::responses_api::build_standard_responses_payload;
use super::tool_serialization;
use super::types::MAX_COMPLETION_TOKENS_FIELD;

const NONE_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
];
const MEDIUM_REASONING_EFFORT_MODELS: &[&str] = &[openai_models::GPT_5, openai_models::GPT_5_4_PRO];
const LOW_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
    openai_models::GPT_5_4_PRO,
    openai_models::GPT_5_3_CODEX,
];
const PHASE_REPLAY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_4,
    openai_models::GPT_5_4_PRO,
    openai_models::GPT_5_3_CODEX,
];
const GATED_SAMPLING_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
];
const SAMPLING_DISABLED_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_4_PRO,
    openai_models::GPT_5_MINI,
    openai_models::GPT_5_NANO,
];

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub base_url: &'a str,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub default_service_tier: Option<&'a str>,
}

pub(crate) struct ResponsesRequestContext<'a> {
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub supports_reasoning_effort: bool,
    pub supports_reasoning: bool,
    pub is_responses_api_model: bool,
    pub include_max_output_tokens: bool,
    pub include_previous_response_id: bool,
    pub include_output_types: bool,
    pub include_sampling_parameters: bool,
    pub force_response_store_false: bool,
    pub include_assistant_phase: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub include_prompt_cache_retention: bool,
    pub prompt_cache_retention: Option<&'a str>,
    pub default_service_tier: Option<&'a str>,
    pub default_response_store: Option<bool>,
    pub default_responses_include: Option<&'a [String]>,
    pub hosted_shell: Option<&'a OpenAIHostedShellConfig>,
    pub include_structured_history_in_input: bool,
    pub preserve_structured_history_on_replay: bool,
    pub preserve_assistant_phase_on_replay: bool,
}

fn strip_non_native_assistant_phase(input: &mut [Value]) {
    for item in input {
        if let Some(map) = item.as_object_mut() {
            map.remove("phase");
        }
    }
}

fn is_gpt5_codex_model(model: &str) -> bool {
    model == openai_models::GPT_5_CODEX
        || (model.starts_with(openai_models::GPT_5) && model.contains("codex"))
}

fn is_openai_gpt_responses_model(model: &str) -> bool {
    model == openai_models::GPT || model.starts_with(openai_models::GPT_5)
}

fn supports_assistant_phase_replay(model: &str) -> bool {
    PHASE_REPLAY_MODELS.contains(&model)
}

fn default_replay_instructions(model: &str) -> Option<String> {
    if is_gpt5_codex_model(model) {
        Some(format!(
            "You are Codex, based on GPT-5. {}",
            default_system_prompt()
        ))
    } else {
        None
    }
}

fn default_reasoning_effort_for_model(model: &str) -> Option<ReasoningEffortLevel> {
    if NONE_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::None)
    } else if MEDIUM_REASONING_EFFORT_MODELS.contains(&model) || is_gpt5_codex_model(model) {
        Some(ReasoningEffortLevel::Medium)
    } else {
        None
    }
}

fn supports_text_verbosity(model: &str) -> bool {
    default_text_verbosity_for_model(model).is_some()
}

fn default_text_verbosity_for_model(model: &str) -> Option<VerbosityLevel> {
    if LOW_VERBOSITY_MODELS.contains(&model) {
        Some(VerbosityLevel::Low)
    } else {
        None
    }
}

fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn allows_sampling_parameters(model: &str, reasoning_effort: Option<ReasoningEffortLevel>) -> bool {
    if GATED_SAMPLING_MODELS.contains(&model) {
        matches!(
            reasoning_effort.unwrap_or(ReasoningEffortLevel::None),
            ReasoningEffortLevel::None
        )
    } else {
        !SAMPLING_DISABLED_MODELS.contains(&model)
    }
}

pub(crate) fn build_chat_request(
    request: &provider::LLMRequest,
    ctx: &ChatRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    for message in &request.messages {
        if let provider::MessageContent::Parts(parts) = &message.content {
            for part in parts {
                if let provider::ContentPart::File {
                    file_url: Some(_), ..
                } = part
                {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Chat Completions does not support file_url inputs; use Responses API or file_id/file_data",
                    );
                    return Err(provider::LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    });
                }
            }
        }
    }

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
    let effective_reasoning_effort = request
        .reasoning_effort
        .or_else(|| default_reasoning_effort_for_model(&request.model));

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
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        openai_request["temperature"] = json!(temperature);
    }

    if ModelProvider::OpenAI.supports_service_tier(&request.model)
        && let Some(service_tier) =
            trimmed_non_empty(request.service_tier.as_deref().or(ctx.default_service_tier))
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

pub(crate) fn build_responses_request(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    let preserve_structured_history = ctx.include_structured_history_in_input
        || (ctx.preserve_structured_history_on_replay
            && is_openai_gpt_responses_model(&request.model));
    let mut responses_payload =
        build_standard_responses_payload(request, preserve_structured_history)?;
    if responses_payload.instructions.is_none()
        && preserve_structured_history
        && let Some(instructions) = default_replay_instructions(&request.model)
    {
        responses_payload.instructions = Some(instructions);
    }

    let mut input = responses_payload.input;
    let instructions = responses_payload.instructions;
    if !(ctx.include_assistant_phase
        || ctx.preserve_assistant_phase_on_replay
            && supports_assistant_phase_replay(&request.model))
    {
        strip_non_native_assistant_phase(&mut input);
    }

    if input.is_empty() {
        let formatted_error =
            error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
        return Err(provider::LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    let mut openai_request = json!({
        "model": request.model,
        "input": input,
        "stream": request.stream,
    });
    let effective_reasoning_effort = request
        .reasoning_effort
        .or_else(|| default_reasoning_effort_for_model(&request.model));

    if ctx.include_max_output_tokens
        && let Some(max_tokens) = request.max_tokens
    {
        openai_request["max_output_tokens"] = json!(max_tokens);
    }

    if ctx.include_output_types {
        // `output_types` constrains which native item types GPT-5 may emit.
        let mut output_types = vec!["message", "tool_call"];
        if ctx.hosted_shell.is_some() {
            output_types.push("shell_call");
        }
        openai_request["output_types"] = json!(output_types);
    }

    if let Some(instructions) = instructions
        && !instructions.trim().is_empty()
    {
        openai_request["instructions"] = json!(instructions);
    }

    if ctx.include_previous_response_id
        && let Some(previous_response_id) =
            trimmed_non_empty(request.previous_response_id.as_deref())
    {
        openai_request["previous_response_id"] = json!(previous_response_id);
    }

    if ModelProvider::OpenAI.supports_service_tier(&request.model)
        && let Some(service_tier) =
            trimmed_non_empty(request.service_tier.as_deref().or(ctx.default_service_tier))
    {
        openai_request["service_tier"] = json!(service_tier);
    }

    if ctx.force_response_store_false {
        openai_request["store"] = json!(false);
    } else if let Some(store) = request.response_store.or(ctx.default_response_store) {
        openai_request["store"] = json!(store);
    }

    if let Some(include_fields) = request
        .responses_include
        .as_deref()
        .or(ctx.default_responses_include)
    {
        let include_values: Vec<String> = include_fields
            .iter()
            .map(|field| field.trim())
            .filter(|field| !field.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if !include_values.is_empty() {
            openai_request["include"] = json!(include_values);
        }
    }

    if let Some(context_management) = &request.context_management {
        openai_request["context_management"] = context_management.clone();
    }

    let mut sampling_parameters = json!({});
    let mut has_sampling = false;

    if let Some(temperature) = request.temperature
        && ctx.supports_temperature
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        sampling_parameters["temperature"] = json!(temperature);
        has_sampling = true;
    }

    if let Some(top_p) = request.top_p
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
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

    if ctx.include_sampling_parameters && has_sampling {
        openai_request["sampling_parameters"] = sampling_parameters;
    }

    if ctx.supports_tools
        && let Some(tools) = &request.tools
        && let Some(serialized) =
            tool_serialization::serialize_tools_for_responses(tools, ctx.hosted_shell)
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
            if let Some(payload) =
                RigProviderCapabilities::new(ModelProvider::OpenAI, &request.model)
                    .reasoning_parameters(effort)
            {
                openai_request["reasoning"] = payload;
            } else {
                openai_request["reasoning"] = json!({ "effort": effort.as_str() });
            }
        } else if openai_request.get("reasoning").is_none()
            && let Some(default_effort) = default_reasoning_effort_for_model(&request.model)
        {
            openai_request["reasoning"] = json!({ "effort": default_effort.as_str() });
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

    if supports_text_verbosity(&request.model)
        && let Some(verbosity) = request.verbosity
    {
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

    if !has_format_options
        && let Some(default_verbosity) = default_text_verbosity_for_model(&request.model)
    {
        text_format["verbosity"] = json!(default_verbosity.as_str());
        has_format_options = true;
    }

    if has_format_options {
        openai_request["text"] = text_format;
    }

    if let Some(prompt_cache_key) = trimmed_non_empty(ctx.prompt_cache_key) {
        openai_request["prompt_cache_key"] = json!(prompt_cache_key);
    }

    // If configured, include the `prompt_cache_retention` value in the Responses API
    // request. This allows the user to extend the server-side prompt cache window
    // (e.g., "24h") to increase cache reuse and reduce cost/latency on GPT-5.
    // Only include prompt_cache_retention when both configured and when the selected
    // model uses the OpenAI Responses API.
    if ctx.include_prompt_cache_retention
        && ctx.is_responses_api_model
        && let Some(retention) = ctx.prompt_cache_retention
        && !retention.trim().is_empty()
    {
        openai_request["prompt_cache_retention"] = json!(retention);
    }

    Ok(openai_request)
}
