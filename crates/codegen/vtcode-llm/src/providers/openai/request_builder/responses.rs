//! Native Responses request assembly over Rig typed state.
//!
//! Extracted from `request_builder.rs` beside the pre-existing `history`
//! and `model_policy` submodules. No behavior change: the shared normal
//! HTTP/SSE Responses JSON boundary (Rig 0.40 typed fields plus custom
//! `output_types`, sampling, context-management, text-verbosity, and safety
//! payloads) is verbatim, and `build_responses_request` is re-exported from
//! the parent.

use super::history::{build_responses_item_history, uses_stable_prefix_cache};
use super::model_policy::{
    allows_sampling_parameters, default_reasoning_effort_for_model, default_text_verbosity_for_model,
    supports_text_verbosity,
};
use super::{ResponsesRequestContext, trimmed_non_empty};
use crate::error_display;
use crate::provider;
use crate::providers::openai::tool_serialization;
use crate::providers::openai::types::OpenAIResponsesPayload;
use crate::rig_adapter::RigProviderCapabilities;
use hashbrown::HashSet;
use rig::providers::openai::responses_api::{
    AdditionalParameters as RigResponsesAdditionalParameters, Include as RigResponsesInclude,
};
use serde_json::{Value, json};
use vtcode_config::models::Provider as ModelProvider;

fn push_unique_include(include_values: &mut Vec<String>, field: &str) {
    let field = field.trim();
    if field.is_empty() || include_values.iter().any(|value| value == field) {
        return;
    }

    include_values.push(field.to_string());
}

fn rig_include_for_field(field: &str) -> Option<RigResponsesInclude> {
    match field {
        "file_search_call.results" => Some(RigResponsesInclude::FileSearchCallResults),
        "message.input_image.image_url" => Some(RigResponsesInclude::MessageInputImageImageUrl),
        "computer_call.output.image_url" => Some(RigResponsesInclude::ComputerCallOutputOutputImageUrl),
        "reasoning.encrypted_content" => Some(RigResponsesInclude::ReasoningEncryptedContent),
        "code_interpreter_call.outputs" => Some(RigResponsesInclude::CodeInterpreterCallOutputs),
        _ => None,
    }
}

fn responses_include_value(field: &str) -> Value {
    rig_include_for_field(field)
        .and_then(|include| serde_json::to_value(include).ok())
        .unwrap_or_else(|| json!(field))
}

fn merge_typed_responses_parameters(openai_request: &mut Value, params: RigResponsesAdditionalParameters) {
    let Ok(Value::Object(fields)) = serde_json::to_value(params) else {
        return;
    };
    let Some(request) = openai_request.as_object_mut() else {
        return;
    };

    request.extend(fields);
}

fn openai_responses_allowed_tools_choice(tool_choice: &provider::ToolChoice, stable_tools: &[Value]) -> Option<Value> {
    let provider::ToolChoice::AllowedTools(choice) = tool_choice else {
        return None;
    };
    if choice.tools.is_empty() {
        return None;
    }

    let active_names: HashSet<&str> = choice.tools.iter().map(String::as_str).collect();
    let tools = stable_tools
        .iter()
        .filter_map(|tool| {
            let tool_type = tool.get("type")?.as_str()?;
            let name = tool.get("name").and_then(Value::as_str).unwrap_or(tool_type);
            if !active_names.contains(name) {
                return None;
            }
            let mut reference = json!({"type": tool_type});
            for field in ["name", "server_label"] {
                if let Some(value) = tool.get(field) {
                    reference[field] = value.clone();
                }
            }
            Some(reference)
        })
        .collect::<Vec<_>>();
    if tools.is_empty() {
        return None;
    }

    Some(json!({
        "type": "allowed_tools",
        "mode": choice.mode.as_str(),
        "tools": tools,
    }))
}

pub(crate) fn build_responses_request(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<Value, provider::LLMError> {
    let responses_payload = build_responses_item_history(request, ctx)?;
    build_responses_request_from_history(request, ctx, responses_payload)
}

/// Shared normal HTTP/SSE Responses JSON boundary.
///
/// Rig 0.40 typed request fields are used here for compatible state such as
/// `store`, `prompt_cache_key`, `prompt_cache_retention`, and known `include`
/// enum values. The final JSON remains custom because Rig lacks typed coverage
/// for VTCode's `output_types`, nested `sampling_parameters`,
/// `context_management`, text verbosity, and provider-specific tool payloads.
/// Protective tests: `openai_request_builder_serialises_rig_typed_state_fields`,
/// `chatgpt_backend_forces_store_false_and_omits_output_sampling_cache`, and
/// `responses_payload_includes_prompt_cache_retention_for_native_openai`.
/// Remove this boundary when Rig exposes those fields with identical streaming
/// and request JSON parity. Normal HTTP/SSE requests deliberately never set
/// `previous_response_id`; WebSocket continuation has its own transport-local
/// path.
fn build_responses_request_from_history(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
    responses_payload: OpenAIResponsesPayload,
) -> Result<Value, provider::LLMError> {
    let input = responses_payload.input;
    let instructions = responses_payload.instructions;
    if input.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
        return Err(provider::LLMError::InvalidRequest { message: formatted_error, metadata: None });
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
        // Responses requires at least 16, including one-word auxiliary probes.
        openai_request["max_output_tokens"] = json!(max_tokens.max(16));
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

    let mut typed_parameters = RigResponsesAdditionalParameters::default();

    if let Some(service_tier) =
        super::resolved_service_tier(&request.model, request.service_tier.as_deref(), ctx.default_service_tier)
    {
        openai_request["service_tier"] = json!(service_tier);
    }

    if ctx.force_response_store_false {
        typed_parameters.store = Some(false);
    } else if let Some(store) = request.response_store.or(ctx.default_response_store) {
        typed_parameters.store = Some(store);
    }

    if let Some(prompt_cache_key) = trimmed_non_empty(ctx.prompt_cache_key) {
        typed_parameters.prompt_cache_key = Some(prompt_cache_key.to_string());
    }

    if ctx.supports_prompt_cache_options {
        let ttl =
            vtcode_config::core::prompt_cache::resolved_openai_prompt_cache_ttl(ctx.prompt_cache_ttl, &request.model);
        openai_request["prompt_cache_options"] = json!({ "ttl": ttl.as_str() });
    } else if ctx.include_prompt_cache_retention
        && ctx.is_responses_api_model
        && let Some(retention) = trimmed_non_empty(ctx.prompt_cache_retention)
    {
        typed_parameters.prompt_cache_retention = Some(retention.to_string());
    }

    merge_typed_responses_parameters(&mut openai_request, typed_parameters);

    if uses_stable_prefix_cache(request, ctx) {
        openai_request["prompt_cache_options"]["mode"] = json!("explicit");
    }

    let mut include_values = Vec::new();
    if let Some(include_fields) = request.responses_include.as_deref().or(ctx.default_responses_include) {
        for field in include_fields {
            push_unique_include(&mut include_values, field);
        }
    }
    if ctx.include_encrypted_reasoning {
        push_unique_include(&mut include_values, "reasoning.encrypted_content");
    }
    let supports_logprobs = vtcode_config::models::model_catalog_entry("openai", &request.model)
        .is_none_or(|entry| entry.supports_logprobs);
    if !supports_logprobs {
        include_values.retain(|field| field != "message.output_text.logprobs");
    }
    if !include_values.is_empty() {
        openai_request["include"] =
            Value::Array(include_values.iter().map(|field| responses_include_value(field)).collect());
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
        sampling_parameters["temperature"] =
            Value::Number(crate::providers::common::float_to_json_number(temperature)?);
        has_sampling = true;
    }

    if let Some(top_p) = request.top_p
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        sampling_parameters["top_p"] = Value::Number(crate::providers::common::float_to_json_number(top_p)?);
        has_sampling = true;
    }

    if let Some(presence_penalty) = request.presence_penalty
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        sampling_parameters["presence_penalty"] =
            Value::Number(crate::providers::common::float_to_json_number(presence_penalty)?);
        has_sampling = true;
    }

    if let Some(frequency_penalty) = request.frequency_penalty
        && allows_sampling_parameters(&request.model, effective_reasoning_effort)
    {
        sampling_parameters["frequency_penalty"] =
            Value::Number(crate::providers::common::float_to_json_number(frequency_penalty)?);
        has_sampling = true;
    }

    if ctx.include_sampling_parameters && has_sampling {
        openai_request["sampling_parameters"] = sampling_parameters;
    }

    if ctx.supports_tools
        && let Some(tools) = &request.tools
        && let Some(serialized) = tool_serialization::serialize_tools_for_responses(tools, ctx.hosted_shell)
    {
        openai_request["tools"] = serialized;

        // Check if any tools are custom types - if so, disable parallel tool calls
        // as per GPT-5 specification: "custom tool type does NOT support parallel tool calling"
        let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
        if has_custom_tool {
            // Override parallel tool calls to false if custom tools are present
            openai_request["parallel_tool_calls"] = Value::Bool(false);
        }

        // Only add tool_choice when tools are present. Native allowed-tools
        // filtering is advisory and derives its subset from the stable
        // catalogue above; unsupported backends/models degrade to regular
        // provider tool_choice values.
        if let Some(tool_choice) = &request.tool_choice {
            openai_request["tool_choice"] = if ctx.supports_allowed_tools {
                openai_responses_allowed_tools_choice(
                    tool_choice,
                    openai_request["tools"].as_array().map(Vec::as_slice).unwrap_or_default(),
                )
                .unwrap_or_else(|| tool_choice.to_responses_format())
            } else {
                tool_choice.to_responses_format()
            };
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
            // The typed adapter validates native catalog models. A custom
            // OpenAI-compatible route can advertise effort support for a
            // model absent from the built-in catalog; that capability was
            // already resolved by the provider trait, so preserve its exact
            // value instead of applying native catalog validation.
            let payload = if ctx.is_responses_api_model {
                RigProviderCapabilities::new(ModelProvider::OpenAI, &request.model)
                    .reasoning_parameters_for_supported_efforts(effort, ctx.supported_reasoning_efforts)?
            } else {
                Some(json!({ "effort": effort.as_str() }))
            };
            if let Some(payload) = payload {
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
        let reasoning_value = map.entry("reasoning".to_string()).or_insert(json!({}));
        if let Some(reasoning_obj) = reasoning_value.as_object_mut() {
            reasoning_obj.entry("summary".to_string()).or_insert_with(|| json!("auto"));
            // Add reasoning.context for persisted reasoning (GPT-5.6+)
            if let Some(context) = ctx.reasoning_context {
                reasoning_obj.entry("context".to_string()).or_insert_with(|| json!(context));
            }
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
        let grammar_tools: Vec<&provider::ToolDefinition> =
            tools.iter().filter(|tool| tool.tool_type == "grammar").collect();

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

    if !has_format_options && let Some(default_verbosity) = default_text_verbosity_for_model(&request.model) {
        text_format["verbosity"] = json!(default_verbosity.as_str());
        has_format_options = true;
    }

    if has_format_options {
        openai_request["text"] = text_format;
    }

    // Add safety_identifier for abuse detection (hashed, not logged)
    if let Some(safety_id) = trimmed_non_empty(ctx.safety_identifier)
        && let Some(map) = openai_request.as_object_mut()
    {
        map.entry("safety_identifier".to_string()).or_insert_with(|| json!(safety_id));
    }

    Ok(openai_request)
}
