//! Chat Completions request builder for OpenAI-compatible APIs.
//!
//! Keeps JSON shaping for chat payloads out of the main provider.

use crate::config::constants::models::openai as openai_models;
use crate::config::core::OpenAIHostedShellConfig;
use crate::config::models::{Provider as ModelProvider, ProviderModelSupport};
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::common::serialize_message_content_openai_for_model;
use crate::llm::rig_adapter::RigProviderCapabilities;
use crate::prompts::system::{default_system_prompt, openai_gpt55_contract_addendum};
use hashbrown::HashSet;
use serde_json::{Value, json};

use super::responses_adapter::{
    PromptCacheOverlay, ResponsesItemAdapterOptions, apply_prompt_cache_overlay,
    map_include_fields, map_request_items_to_responses, merge_rig_supported_state,
    rig_supported_state_parameters, strip_assistant_phase_overlay,
};
use super::tool_serialization;
use super::types::{MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload};

const NONE_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
];
const MEDIUM_REASONING_EFFORT_MODELS: &[&str] = &[openai_models::GPT_5, openai_models::GPT_5_4_PRO];
const TEXT_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
    openai_models::GPT_5_4_PRO,
    openai_models::GPT_5_3_CODEX,
];
const LOW_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_2,
    openai_models::GPT_5_4,
    openai_models::GPT_5_4_PRO,
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
    openai_models::GPT_5_5,
    openai_models::GPT_5_5_DATED,
];
const SAMPLING_DISABLED_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_4_PRO,
    openai_models::GPT_5_MINI,
    openai_models::GPT_5_NANO,
];

pub(crate) struct ChatRequestContext<'a> {
    pub model: &'a str,
    pub is_native_openai: bool,
    pub supports_tools: bool,
    pub supports_parallel_tool_config: bool,
    pub supports_temperature: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub default_service_tier: Option<&'a str>,
}

pub(crate) struct ResponsesRequestContext<'a> {
    pub supports_tools: bool,
    pub supports_allowed_tools: bool,
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
    pub include_encrypted_reasoning: bool,
    pub hosted_shell: Option<&'a OpenAIHostedShellConfig>,
    pub include_structured_history_in_input: bool,
    pub preserve_structured_history_on_replay: bool,
    pub preserve_assistant_phase_on_replay: bool,
}

fn is_gpt5_codex_model(model: &str) -> bool {
    model == openai_models::GPT_5_CODEX
        || (model.starts_with(openai_models::GPT_5) && model.contains("codex"))
}

fn is_gpt55_model(model: &str) -> bool {
    model == openai_models::GPT_5_5 || model == openai_models::GPT_5_5_DATED
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
    } else if is_gpt55_model(model) {
        Some(default_system_prompt().to_string())
    } else {
        None
    }
}

fn default_reasoning_effort_for_model(model: &str) -> Option<ReasoningEffortLevel> {
    if NONE_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::None)
    } else if is_gpt5_codex_model(model) {
        Some(ReasoningEffortLevel::High)
    } else if MEDIUM_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::Medium)
    } else {
        None
    }
}

fn supports_text_verbosity(model: &str) -> bool {
    TEXT_VERBOSITY_MODELS.contains(&model)
}

fn openai_responses_allowed_tools_choice(
    tool_choice: &provider::ToolChoice,
    stable_tools: &[provider::ToolDefinition],
) -> Option<Value> {
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
            let name = tool.function_name();
            active_names.contains(name).then(|| json!(name))
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

fn augment_openai_instructions(model: &str, instructions: String) -> String {
    if !is_gpt55_model(model) {
        return instructions;
    }

    let addendum = openai_gpt55_contract_addendum();
    if instructions.contains(addendum.trim()) {
        instructions
    } else if instructions.trim().is_empty() {
        addendum
    } else {
        format!("{instructions}\n\n{addendum}")
    }
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
        let system_prompt = augment_openai_instructions(&request.model, system_prompt.to_string());
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
    let responses_payload = build_responses_item_history(request, ctx)?;
    build_responses_request_from_history(request, ctx, responses_payload)
}

fn build_responses_item_history(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<OpenAIResponsesPayload, provider::LLMError> {
    let preserve_structured_history = ctx.include_structured_history_in_input
        || (ctx.preserve_structured_history_on_replay
            && is_openai_gpt_responses_model(&request.model));
    let mut responses_payload = map_request_items_to_responses(
        request,
        ResponsesItemAdapterOptions {
            include_structured_history_in_input: preserve_structured_history,
        },
    )?;
    if responses_payload.instructions.is_none()
        && preserve_structured_history
        && let Some(instructions) = default_replay_instructions(&request.model)
    {
        responses_payload.instructions = Some(instructions);
    }

    responses_payload.instructions = responses_payload
        .instructions
        .take()
        .map(|instructions| augment_openai_instructions(&request.model, instructions));

    if !(ctx.include_assistant_phase
        || ctx.preserve_assistant_phase_on_replay
            && supports_assistant_phase_replay(&request.model))
    {
        strip_assistant_phase_overlay(&mut responses_payload.input);
    }

    Ok(responses_payload)
}

fn build_responses_request_from_history(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
    responses_payload: OpenAIResponsesPayload,
) -> Result<Value, provider::LLMError> {
    let input = responses_payload.input;
    let instructions = responses_payload.instructions;
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

    let previous_response_id = ctx
        .include_previous_response_id
        .then(|| trimmed_non_empty(request.previous_response_id.as_deref()))
        .flatten();

    if ModelProvider::OpenAI.supports_service_tier(&request.model)
        && let Some(service_tier) =
            trimmed_non_empty(request.service_tier.as_deref().or(ctx.default_service_tier))
    {
        openai_request["service_tier"] = json!(service_tier);
    }

    if ctx.force_response_store_false {
        merge_rig_supported_state(
            &mut openai_request,
            rig_supported_state_parameters(previous_response_id, Some(false)),
        );
    } else if let Some(store) = request.response_store.or(ctx.default_response_store) {
        merge_rig_supported_state(
            &mut openai_request,
            rig_supported_state_parameters(previous_response_id, Some(store)),
        );
    } else {
        merge_rig_supported_state(
            &mut openai_request,
            rig_supported_state_parameters(previous_response_id, None),
        );
    }

    if let Some(include) = map_include_fields(
        request
            .responses_include
            .as_deref()
            .or(ctx.default_responses_include),
        ctx.include_encrypted_reasoning,
    ) {
        openai_request["include"] = include;
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

        // Only add tool_choice when tools are present. Native allowed-tools
        // filtering is advisory and derives its subset from the stable
        // catalogue above; unsupported backends/models degrade to regular
        // provider tool_choice values.
        if let Some(tool_choice) = &request.tool_choice {
            openai_request["tool_choice"] = if ctx.supports_allowed_tools {
                openai_responses_allowed_tools_choice(tool_choice, tools)
                    .unwrap_or_else(|| tool_choice.to_provider_format("openai"))
            } else {
                tool_choice.to_provider_format("openai")
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
        if let Some(reasoning_obj) = reasoning_value.as_object_mut() {
            reasoning_obj
                .entry("summary".to_string())
                .or_insert_with(|| json!("auto"));
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

    apply_prompt_cache_overlay(
        &mut openai_request,
        PromptCacheOverlay {
            prompt_cache_key: ctx.prompt_cache_key,
            include_prompt_cache_retention: ctx.include_prompt_cache_retention,
            is_responses_api_model: ctx.is_responses_api_model,
            prompt_cache_retention: ctx.prompt_cache_retention,
        },
    );

    Ok(openai_request)
}

#[cfg(test)]
mod tests {
    use super::{ResponsesRequestContext, apply_prompt_cache_overlay, build_responses_request};
    use crate::config::constants::models;
    use crate::llm::provider;
    use crate::llm::providers::openai::responses_adapter::PromptCacheOverlay;
    use serde_json::{Value, json};

    fn base_context<'a>(
        default_responses_include: Option<&'a [String]>,
    ) -> ResponsesRequestContext<'a> {
        ResponsesRequestContext {
            supports_tools: false,
            supports_allowed_tools: false,
            supports_parallel_tool_config: false,
            supports_temperature: true,
            supports_reasoning_effort: true,
            supports_reasoning: true,
            is_responses_api_model: true,
            include_max_output_tokens: true,
            include_previous_response_id: true,
            include_output_types: true,
            include_sampling_parameters: true,
            force_response_store_false: false,
            include_assistant_phase: true,
            prompt_cache_key: None,
            include_prompt_cache_retention: false,
            prompt_cache_retention: None,
            default_service_tier: None,
            default_response_store: None,
            default_responses_include,
            include_encrypted_reasoning: false,
            hosted_shell: None,
            include_structured_history_in_input: true,
            preserve_structured_history_on_replay: false,
            preserve_assistant_phase_on_replay: false,
        }
    }

    fn request() -> provider::LLMRequest {
        provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_owned())],
            model: models::openai::GPT_5.to_string(),
            stream: true,
            ..Default::default()
        }
    }

    #[test]
    fn openai_request_builder_serialises_rig_typed_state_fields() {
        let mut request = request();
        request.previous_response_id = Some("resp_previous".to_owned());
        request.response_store = Some(false);

        let payload = build_responses_request(&request, &base_context(None))
            .expect("responses request should build");

        assert_eq!(
            payload.get("previous_response_id").and_then(Value::as_str),
            Some("resp_previous")
        );
        assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn openai_request_builder_preserves_custom_include_strings_around_typed_include() {
        let default_include = vec!["output_text.annotations".to_owned()];
        let mut ctx = base_context(Some(default_include.as_slice()));
        ctx.include_encrypted_reasoning = true;

        let payload =
            build_responses_request(&request(), &ctx).expect("responses request should build");

        assert_eq!(
            payload.get("include").and_then(Value::as_array),
            Some(&vec![
                json!("output_text.annotations"),
                json!("reasoning.encrypted_content"),
            ])
        );
    }

    #[test]
    fn prompt_cache_overlay_inserts_fields_at_final_json_boundary() {
        let mut ctx = base_context(None);
        ctx.prompt_cache_key = Some("vtcode:openai:session-123");
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");

        let payload =
            build_responses_request(&request(), &ctx).expect("responses request should build");

        assert_eq!(
            payload.get("prompt_cache_key").and_then(Value::as_str),
            Some("vtcode:openai:session-123")
        );
        assert_eq!(
            payload
                .get("prompt_cache_retention")
                .and_then(Value::as_str),
            Some("24h")
        );
    }

    #[test]
    fn prompt_cache_overlay_does_not_overwrite_existing_fields() {
        let mut ctx = base_context(None);
        ctx.prompt_cache_key = Some("vtcode:openai:session-123");
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");
        let mut payload = json!({
            "prompt_cache_key": "typed-key",
            "prompt_cache_retention": "in_memory"
        });

        apply_prompt_cache_overlay(
            &mut payload,
            PromptCacheOverlay {
                prompt_cache_key: ctx.prompt_cache_key,
                include_prompt_cache_retention: ctx.include_prompt_cache_retention,
                is_responses_api_model: ctx.is_responses_api_model,
                prompt_cache_retention: ctx.prompt_cache_retention,
            },
        );

        assert_eq!(
            payload.get("prompt_cache_key").and_then(Value::as_str),
            Some("typed-key")
        );
        assert_eq!(
            payload
                .get("prompt_cache_retention")
                .and_then(Value::as_str),
            Some("in_memory")
        );
    }

    #[test]
    fn responses_request_boundary_retains_vtcode_overlays() {
        let mut request = provider::LLMRequest {
            messages: vec![
                provider::Message::user("Apply patch".to_owned()),
                provider::Message::assistant_with_tools(
                    String::new(),
                    vec![provider::ToolCall::custom(
                        "call_patch_1".to_owned(),
                        "apply_patch".to_owned(),
                        "*** Begin Patch\n*** End Patch\n".to_owned(),
                    )],
                ),
                provider::Message::tool_response("call_patch_1".to_owned(), "patched".to_owned()),
            ],
            model: models::openai::GPT_5_3_CODEX.to_owned(),
            temperature: Some(0.2),
            top_p: Some(0.9),
            context_management: Some(json!([{"type": "compaction"}])),
            prompt_cache_key: Some("vtcode:openai:session".to_owned()),
            verbosity: Some(crate::config::types::VerbosityLevel::High),
            stream: true,
            ..Default::default()
        };
        request.responses_include = Some(vec!["output_text.annotations".to_owned()]);

        let mut ctx = base_context(None);
        ctx.prompt_cache_key = request.prompt_cache_key.as_deref();
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");

        let payload =
            build_responses_request(&request, &ctx).expect("responses request should build");

        assert_eq!(payload["input"][1]["type"], "custom_tool_call");
        assert_eq!(payload["input"][2]["type"], "custom_tool_call_output");
        assert_eq!(payload["include"], json!(["output_text.annotations"]));
        assert_eq!(
            payload["context_management"],
            json!([{"type": "compaction"}])
        );
        assert_eq!(payload["prompt_cache_key"], json!("vtcode:openai:session"));
        assert_eq!(payload["prompt_cache_retention"], json!("24h"));
        assert_eq!(payload["output_types"], json!(["message", "tool_call"]));
        let temperature = payload["sampling_parameters"]["temperature"]
            .as_f64()
            .expect("temperature should be numeric");
        let top_p = payload["sampling_parameters"]["top_p"]
            .as_f64()
            .expect("top_p should be numeric");
        assert!((temperature - 0.2).abs() < 0.000_001);
        assert!((top_p - 0.9).abs() < 0.000_001);
        assert_eq!(payload["text"]["verbosity"], json!("high"));
    }
}
