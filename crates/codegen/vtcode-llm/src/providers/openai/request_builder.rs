//! Chat Completions request builder for OpenAI-compatible APIs.
//!
//! Keeps JSON shaping for chat payloads out of the main provider.

use crate::error_display;
use crate::provider;
use crate::providers::common::serialize_message_content_openai_for_model;
use crate::rig_adapter::RigProviderCapabilities;
use crate::system_prompt::{default_system_prompt, openai_gpt6_contract_addendum, openai_gpt56_contract_addendum};
use hashbrown::HashSet;
use rig::providers::openai::responses_api::{
    AdditionalParameters as RigResponsesAdditionalParameters, Include as RigResponsesInclude,
};
use serde_json::{Value, json};
use vtcode_config::constants::models::openai as openai_models;
use vtcode_config::core::OpenAIHostedShellConfig;
use vtcode_config::models::{Provider as ModelProvider, ProviderModelSupport};
use vtcode_config::types::{ReasoningEffortLevel, VerbosityLevel};

use super::responses_api::build_standard_responses_payload;
use super::tool_serialization;
use super::types::{InstructionSegmentKind, MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload};
use crate::providers::shared::split_dynamic_prompt_suffix;

const NONE_REASONING_EFFORT_MODELS: &[&str] = &[openai_models::GPT, openai_models::GPT_5_6, openai_models::GPT_5_6_SOL];
/// Default `prompt_cache_options.ttl` for GPT-5.6-family Responses requests.
/// Currently the only value OpenAI accepts; sent explicitly so cache intent
/// does not rely on the implicit default alone.
const DEFAULT_GPT56_PROMPT_CACHE_TTL: &str = "30m";
const MEDIUM_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
];
const HIGH_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_TERRA,
    openai_models::GPT_5_6_LUNA,
    openai_models::GPT_5_6,
];
const TEXT_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
    openai_models::GPT_5_6,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_CODEX,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_TERRA,
    openai_models::GPT_5_6_LUNA,
    openai_models::GPT_5_6,
];
const LOW_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_6,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
];
const PHASE_REPLAY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_CODEX,
];
const GATED_SAMPLING_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_5_6,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_SOL,
];
const SAMPLING_DISABLED_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_6_SOL,
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
    pub supported_reasoning_efforts: &'a [&'a str],
    pub supports_reasoning: bool,
    pub is_responses_api_model: bool,
    pub include_max_output_tokens: bool,
    pub include_output_types: bool,
    pub include_sampling_parameters: bool,
    pub force_response_store_false: bool,
    pub include_assistant_phase: bool,
    pub prompt_cache_key: Option<&'a str>,
    pub include_prompt_cache_retention: bool,
    pub prompt_cache_retention: Option<&'a str>,
    /// Emit explicit `prompt_cache_breakpoint` markers on the stable prefix.
    /// Only native api.openai.com honors them: third-party
    /// Responses-compatible endpoints (e.g. Merge Gateway via a base-URL
    /// override) reject the field with 400 `invalid_parameter`, and the
    /// ChatGPT subscription backend rejects it with 400
    /// `prompt_cache_breakpoint is not supported on this model` (observed
    /// live on `gpt-6.1-sol`). Implicit breakpoints still cover the latest
    /// message on those backends.
    pub include_explicit_cache_breakpoints: bool,
    pub default_service_tier: Option<&'a str>,
    pub default_response_store: Option<bool>,
    pub default_responses_include: Option<&'a [String]>,
    pub include_encrypted_reasoning: bool,
    pub hosted_shell: Option<&'a OpenAIHostedShellConfig>,
    pub include_structured_history_in_input: bool,
    pub preserve_structured_history_on_replay: bool,
    pub preserve_assistant_phase_on_replay: bool,
    pub reasoning_context: Option<&'a str>,
    pub safety_identifier: Option<&'a str>,
}

fn strip_non_native_assistant_phase(input: &mut [Value]) {
    for item in input {
        if let Some(map) = item.as_object_mut() {
            map.remove("phase");
        }
    }
}

fn is_gpt5_codex_model(model: &str) -> bool {
    model == openai_models::GPT_5_CODEX || (model.starts_with(openai_models::GPT_5) && model.contains("codex"))
}

fn is_gpt55_model(model: &str) -> bool {
    model == openai_models::GPT_5_6_SOL
}

fn is_gpt56_model(model: &str) -> bool {
    matches!(
        model,
        openai_models::GPT_5_6_SOL
            | openai_models::GPT_5_6_TERRA
            | openai_models::GPT_5_6_LUNA
            | openai_models::GPT_5_6
    )
}

fn is_gpt6_model(model: &str) -> bool {
    matches!(
        model,
        openai_models::GPT_6_ASTRA | openai_models::GPT_6_SOL | openai_models::GPT_6_1_SOL | openai_models::GPT_6_LUNA
    )
}

fn is_openai_gpt_responses_model(model: &str) -> bool {
    openai_models::RESPONSES_API_MODELS.contains(&model)
}

fn supports_assistant_phase_replay(model: &str) -> bool {
    PHASE_REPLAY_MODELS.contains(&model)
}

fn default_replay_instructions(model: &str) -> Option<String> {
    if is_gpt5_codex_model(model) {
        Some(format!("You are Codex, based on GPT-5. {}", default_system_prompt()))
    } else if is_gpt55_model(model)
        || is_gpt56_model(model)
        || is_gpt6_model(model)
        || vtcode_config::models::model_catalog_entry("openai", model)
            .is_some_and(|entry| entry.prompt_contract.is_some())
    {
        Some(default_system_prompt())
    } else {
        None
    }
}

fn augment_openai_instructions(model: &str, instructions: String) -> String {
    let addendum =
        match vtcode_config::models::model_catalog_entry("openai", model).and_then(|entry| entry.prompt_contract) {
            Some("gpt6") => Some(openai_gpt6_contract_addendum()),
            Some("gpt56") => Some(openai_gpt56_contract_addendum()),
            _ if is_gpt6_model(model) => Some(openai_gpt6_contract_addendum()),
            _ if is_gpt56_model(model) => Some(openai_gpt56_contract_addendum()),
            _ => None,
        };

    let Some(addendum) = addendum else {
        return instructions;
    };
    let trimmed_addendum = addendum.trim();
    if instructions.contains(trimmed_addendum) {
        instructions
    } else if instructions.trim().is_empty() {
        addendum
    } else {
        format!("{instructions}\n\n{addendum}")
    }
}

fn default_reasoning_effort_for_model(model: &str) -> Option<ReasoningEffortLevel> {
    if NONE_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::None)
    } else if is_gpt5_codex_model(model) {
        Some(ReasoningEffortLevel::High)
    } else if MEDIUM_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::Medium)
    } else if HIGH_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::High)
    } else {
        None
    }
}

fn supports_text_verbosity(model: &str) -> bool {
    TEXT_VERBOSITY_MODELS.contains(&model)
}

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
    let supports_sampling = vtcode_config::models::model_catalog_entry("openai", model)
        .map(|entry| entry.supports_sampling)
        .unwrap_or_else(|| !SAMPLING_DISABLED_MODELS.contains(&model));
    if !supports_sampling {
        false
    } else if GATED_SAMPLING_MODELS.contains(&model) {
        matches!(reasoning_effort.unwrap_or(ReasoningEffortLevel::None), ReasoningEffortLevel::None)
    } else {
        !SAMPLING_DISABLED_MODELS.contains(&model)
    }
}

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

    if ModelProvider::OpenAI.supports_service_tier(&request.model)
        && let Some(service_tier) = trimmed_non_empty(request.service_tier.as_deref().or(ctx.default_service_tier))
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

/// Retained custom Responses item/history boundary.
///
/// Rig 0.40 has typed Responses input items, but does not preserve VTCode's
/// assistant `phase` replay field, open-ended include strings such as
/// `output_text.annotations`, synthetic missing tool outputs, or the exact
/// instruction fallback used for ChatGPT replay parity. Protective tests:
/// `api_key_and_chatgpt_subscription_share_responses_item_history_builder`,
/// `openai_request_builder_preserves_custom_include_strings_around_typed_include`,
/// and the `responses_api` history tests. Remove this boundary once Rig exposes
/// open custom include values and VTCode-compatible structured history hooks.
fn is_explicit_cache_breakpoint_model(model: &str) -> bool {
    // GPT-5.6+ (and GPT-6 family) support explicit `prompt_cache_breakpoint`
    // markers. Older models (GPT-5.5 and earlier) reject `prompt_cache_options`
    // and `prompt_cache_breakpoint` outright, so never emit markers for them.
    is_gpt56_model(model) || is_gpt6_model(model)
}

fn is_eligible_cache_breakpoint_block(block: &Value) -> bool {
    matches!(block.get("type").and_then(Value::as_str), Some("input_text" | "input_image" | "input_file"))
}

/// Whether an input item is the relocated volatile-reminance reminder.
///
/// The reminder carries per-turn content, so it must never receive an explicit
/// breakpoint (the marker would churn every turn).
fn is_runtime_reminder_item(item: &Value) -> bool {
    item.get("content").and_then(Value::as_array).is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with(DYNAMIC_RUNTIME_REMINDER_LABEL))
        })
    })
}

fn apply_explicit_cache_breakpoints(input: &mut [Value], model: &str) {
    if !is_explicit_cache_breakpoint_model(model) || input.is_empty() {
        return;
    }

    // Stable-prefix discipline: the newest trailing item is the varying suffix
    // (covered by the implicit breakpoint). Explicit markers go on user-message
    // boundaries inside the stable prefix so growing history keeps earlier
    // markers byte-stable across turns. Cap at the 4 most recent boundaries to
    // stay within the per-request write budget; older prefixes remain readable
    // through the prior requests' writes.
    let stable_end = input.len().saturating_sub(1);
    let mut marked = 0u8;
    for item in input[..stable_end].iter_mut().rev() {
        if marked >= 4 {
            break;
        }
        if is_runtime_reminder_item(item) {
            continue;
        }
        let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let Some(block) = content.iter_mut().rev().find(|block| is_eligible_cache_breakpoint_block(block)) else {
            continue;
        };
        if block.get("prompt_cache_breakpoint").is_none() {
            block["prompt_cache_breakpoint"] = json!({ "mode": "explicit" });
        }
        marked += 1;
    }

    // Single-turn (or no eligible block in the stable prefix): mark the last
    // eligible block so the first request still writes. The trailing item is
    // the varying suffix (latest message or relocated runtime reminder), so
    // skip it whenever an earlier item exists; otherwise the marker itself
    // would churn every turn.
    if marked == 0 {
        let fallback_end = if input.len() > 1 { input.len() - 1 } else { input.len() };
        for item in input[..fallback_end].iter_mut().rev() {
            // A reminder-only input (history of only System messages) has no
            // stable content to anchor; marking it would churn per turn.
            if is_runtime_reminder_item(item) {
                continue;
            }
            let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            if let Some(block) = content.iter_mut().rev().find(|block| is_eligible_cache_breakpoint_block(block))
                && block.get("prompt_cache_breakpoint").is_none()
            {
                block["prompt_cache_breakpoint"] = json!({ "mode": "explicit" });
                break;
            }
        }
    }
}

fn build_responses_item_history(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<OpenAIResponsesPayload, provider::LLMError> {
    let preserve_structured_history = ctx.include_structured_history_in_input
        || (ctx.preserve_structured_history_on_replay
            && (is_openai_gpt_responses_model(&request.model) || is_gpt5_codex_model(&request.model)));
    let mut responses_payload = build_standard_responses_payload(request, preserve_structured_history)?;
    if responses_payload.instructions.is_none()
        && preserve_structured_history
        && let Some(instructions) = default_replay_instructions(&request.model)
    {
        responses_payload.instructions = Some(instructions);
    }

    separate_dynamic_instructions(request, &mut responses_payload);

    if !(ctx.include_assistant_phase
        || ctx.preserve_assistant_phase_on_replay && supports_assistant_phase_replay(&request.model))
    {
        strip_non_native_assistant_phase(&mut responses_payload.input);
    }

    // GPT-5.6+ exact-match caching needs explicit breakpoints on the stable
    // prefix; the implicit breakpoint alone only covers the latest message.
    // Gated on the Responses model family (not the TTL flag) so the API-key
    // and ChatGPT backends keep sharing one item/history builder, and on the
    // backend flag so third-party endpoints never receive markers.
    // Older models reject the field outright, enforced inside the helper, and
    // non-native endpoints never receive markers (they 400 on the field).
    if ctx.is_responses_api_model && ctx.include_explicit_cache_breakpoints {
        apply_explicit_cache_breakpoints(&mut responses_payload.input, &request.model);
    }

    Ok(responses_payload)
}

/// Trailing reminder label for relocated volatile prompt content.
///
/// Constant bytes keep the reminder tail cheap to reprocess; the label itself
/// never varies across turns.
const DYNAMIC_RUNTIME_REMINDER_LABEL: &str = "[System reminder — runtime context, not a user request]";

/// Split volatile content out of Responses `instructions` into a trailing reminder.
///
/// The payload builder folds the whole system prompt (including per-turn
/// sections like `[Runtime Tool Catalog]`, `[Deferred Tools]`, and planning
/// notices) plus every history `System` message (compaction summaries, resume
/// notes) into one `instructions` string. Any per-turn change there rewrites
/// the cached prefix from byte zero, so sustained hit rates collapse.
/// Mirror the Anthropic wire split: the stable system prefix stays in
/// `instructions` (with the model contract addendum applied to the stable part
/// only) while volatile segments — labeled by the builder via
/// `InstructionSegmentKind`, so segment attribution never depends on guessing
/// at a joined-string layout — move to a trailing user-role reminder item.
/// History order is otherwise untouched, so grown histories keep the stable
/// input prefix byte-stable across turns.
///
/// Fast path (no volatile segments): `instructions` keeps the exact legacy
/// augmented value. When the builder supplied no provenance (e.g. a static
/// replay fallback injected upstream), the whole string is treated as stable.
fn separate_dynamic_instructions(request: &provider::LLMRequest, payload: &mut OpenAIResponsesPayload) {
    let Some(built) = payload.instructions.take() else {
        return;
    };
    let segments = match payload.instruction_segments.take() {
        Some(segments) => segments,
        None => {
            // No provenance: static replay fallback or a caller-built string.
            // Keep it whole — it is session-stable by construction.
            payload.instructions = Some(augment_openai_instructions(&request.model, built));
            return;
        }
    };

    // Partition by provenance into a stable prefix and a volatile tail, then
    // split the system-prompt segment on dynamic-section headers so its
    // per-turn sections (`[Harness Limits]`, `[Runtime Tool Catalog]`, …) also
    // leave the cached prefix. Volatile segments keep their wire order.
    let mut stable_parts: Vec<String> = Vec::with_capacity(segments.len());
    let mut volatile_parts: Vec<String> = Vec::new();
    for (kind, text) in &segments {
        match kind {
            InstructionSegmentKind::SystemPrompt => {
                let (stable, dynamic) = split_dynamic_prompt_suffix(text);
                if !stable.is_empty() {
                    stable_parts.push(stable);
                }
                if let Some(dynamic) = dynamic.filter(|text| !text.trim().is_empty()) {
                    volatile_parts.push(dynamic);
                }
            }
            InstructionSegmentKind::HistorySystem | InstructionSegmentKind::FoldedHistory => {
                volatile_parts.push(text.clone());
            }
        }
    }

    if volatile_parts.is_empty() {
        payload.instructions = Some(augment_openai_instructions(&request.model, built));
        return;
    }

    let stable_joined = stable_parts.join("\n\n");
    let stable_instructions = if stable_joined.trim().is_empty() {
        // A system prompt that is entirely dynamic leaves no stable text, but
        // the model contract addendum must still ride the cached prefix so it
        // is not silently dropped (legacy always appended it).
        let had_system_prompt = segments.iter().any(|(kind, _)| *kind == InstructionSegmentKind::SystemPrompt);
        let addendum_only = augment_openai_instructions(&request.model, String::new());
        (had_system_prompt && !addendum_only.trim().is_empty()).then_some(addendum_only)
    } else {
        Some(augment_openai_instructions(&request.model, stable_joined))
    };
    payload.instructions = stable_instructions;

    let tail = volatile_parts.join("\n\n");
    let mut reminder = String::with_capacity(DYNAMIC_RUNTIME_REMINDER_LABEL.len() + 1 + tail.len());
    reminder.push_str(DYNAMIC_RUNTIME_REMINDER_LABEL);
    reminder.push('\n');
    reminder.push_str(&tail);
    payload.input.push(json!({
        "role": "user",
        "content": [{ "type": "input_text", "text": reminder }]
    }));
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

    if ModelProvider::OpenAI.supports_service_tier(&request.model)
        && let Some(service_tier) = trimmed_non_empty(request.service_tier.as_deref().or(ctx.default_service_tier))
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

    let cache_ttl = vtcode_config::models::model_catalog_entry("openai", &request.model)
        .and_then(|entry| entry.prompt_cache_ttl)
        .and_then(|ttl| trimmed_non_empty(Some(ttl)));
    // GPT-5.6+ uses `prompt_cache_options.ttl` ("30m" is currently the only
    // supported value); the legacy `prompt_cache_retention` field is
    // deprecated for these models. No catalog entry sets a TTL today, so
    // default GPT-5.6-family Responses requests to "30m" to document caching
    // intent explicitly instead of relying on the implicit default alone.
    let cache_ttl = cache_ttl.or_else(|| {
        ((is_gpt56_model(&request.model) || is_gpt6_model(&request.model))
            && ctx.include_prompt_cache_retention
            && ctx.is_responses_api_model)
            .then_some(DEFAULT_GPT56_PROMPT_CACHE_TTL)
    });
    if let Some(ttl) = cache_ttl
        && ctx.include_prompt_cache_retention
        && ctx.is_responses_api_model
    {
        openai_request["prompt_cache_options"] = json!({ "ttl": ttl });
    } else if ctx.include_prompt_cache_retention
        && ctx.is_responses_api_model
        && let Some(retention) = trimmed_non_empty(ctx.prompt_cache_retention)
    {
        typed_parameters.prompt_cache_retention = Some(retention.to_string());
    }

    merge_typed_responses_parameters(&mut openai_request, typed_parameters);

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

#[cfg(test)]
mod tests {
    use super::{
        ChatRequestContext, ResponsesRequestContext, augment_openai_instructions, build_chat_request,
        build_responses_request,
    };
    use crate::provider;
    use serde_json::{Value, json};
    use vtcode_config::constants::models;

    fn base_context<'a>(default_responses_include: Option<&'a [String]>) -> ResponsesRequestContext<'a> {
        ResponsesRequestContext {
            supports_tools: false,
            supports_allowed_tools: false,
            supports_parallel_tool_config: false,
            supports_temperature: true,
            supports_reasoning_effort: true,
            supported_reasoning_efforts: &["low", "medium", "high", "xhigh", "max"],
            supports_reasoning: true,
            is_responses_api_model: true,
            include_max_output_tokens: true,
            include_output_types: true,
            include_sampling_parameters: true,
            force_response_store_false: false,
            include_assistant_phase: true,
            prompt_cache_key: None,
            include_prompt_cache_retention: false,
            prompt_cache_retention: None,
            include_explicit_cache_breakpoints: true,
            default_service_tier: None,
            default_response_store: None,
            default_responses_include,
            include_encrypted_reasoning: false,
            hosted_shell: None,
            include_structured_history_in_input: true,
            preserve_structured_history_on_replay: false,
            preserve_assistant_phase_on_replay: false,
            reasoning_context: None,
            safety_identifier: None,
        }
    }

    fn request() -> provider::LLMRequest {
        provider::LLMRequest {
            messages: vec![provider::Message::user("Hello".to_owned())].into(),
            model: models::openai::GPT_5.to_string(),
            stream: true,
            ..Default::default()
        }
    }

    fn chat_context() -> ChatRequestContext<'static> {
        ChatRequestContext {
            model: "custom-chat-model",
            is_native_openai: true,
            supports_tools: false,
            supports_parallel_tool_config: false,
            supports_temperature: true,
            prompt_cache_key: None,
            default_service_tier: None,
        }
    }

    #[test]
    fn chat_request_carries_top_p_and_penalties_with_compact_wire_form() {
        let mut request = request();
        request.model = "custom-chat-model".to_string();
        request.temperature = Some(0.7);
        request.top_p = Some(0.9);
        request.presence_penalty = Some(0.1);
        request.frequency_penalty = Some(-0.5);

        let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");

        assert_eq!(payload.get("temperature"), Some(&json!(0.7)));
        assert_eq!(
            payload.get("top_p").expect("top_p should serialize").to_string(),
            "0.9",
            "wire form must be compact"
        );
        assert_eq!(payload.get("presence_penalty").and_then(Value::as_f64), Some(0.1));
        assert_eq!(payload.get("frequency_penalty").and_then(Value::as_f64), Some(-0.5));
    }

    #[test]
    fn openai_request_builder_serialises_rig_typed_state_fields() {
        let mut request = request();
        request.previous_response_id = Some("resp_previous".to_owned());
        request.response_store = Some(true);
        let mut ctx = base_context(None);
        ctx.force_response_store_false = true;

        let payload = build_responses_request(&request, &ctx).expect("responses request should build");

        assert!(payload.get("previous_response_id").is_none());
        assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn responses_output_token_limit_normalizes_probe_and_boundary_allowances() {
        let mut request = request();
        request.model = models::openai::GPT_6_LUNA.to_string();
        let mut ctx = base_context(None);
        for (requested, expected) in [(1, 16), (8, 16), (15, 16), (16, 16), (17, 17), (512, 512)] {
            request.max_tokens = Some(requested);
            let payload = build_responses_request(&request, &ctx).expect("responses request");
            assert_eq!(payload["max_output_tokens"], expected, "requested {requested}");
        }
        request.max_tokens = None;
        assert!(
            build_responses_request(&request, &ctx)
                .unwrap()
                .get("max_output_tokens")
                .is_none()
        );
        request.max_tokens = Some(8);
        ctx.include_max_output_tokens = false;
        assert!(
            build_responses_request(&request, &ctx)
                .unwrap()
                .get("max_output_tokens")
                .is_none()
        );
    }

    #[test]
    fn openai_request_builder_preserves_custom_include_strings_around_typed_include() {
        let default_include = vec!["output_text.annotations".to_owned()];
        let mut ctx = base_context(Some(default_include.as_slice()));
        ctx.include_encrypted_reasoning = true;

        let payload = build_responses_request(&request(), &ctx).expect("responses request should build");

        assert_eq!(
            payload.get("include").and_then(Value::as_array),
            Some(&vec![json!("output_text.annotations"), json!("reasoning.encrypted_content"),])
        );
    }

    #[test]
    fn prompt_cache_fields_serialize_through_rig_typed_parameters() {
        let mut ctx = base_context(None);
        ctx.prompt_cache_key = Some("vtcode:openai:session-123");
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");

        let payload = build_responses_request(&request(), &ctx).expect("responses request should build");

        assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:openai:session-123"));
        assert_eq!(payload.get("prompt_cache_retention").and_then(Value::as_str), Some("24h"));
    }

    #[test]
    fn chatgpt_request_construction_uses_shared_normal_responses_builder() {
        let mut request = request();
        request.stream = false;
        request.max_tokens = Some(512);
        request.temperature = Some(0.7);
        request.top_p = Some(0.8);
        request.parallel_tool_calls = Some(true);
        request.previous_response_id = Some("resp_previous".to_owned());
        request.responses_include = Some(vec!["output_text.annotations".to_owned()]);

        let mut ctx = base_context(None);
        ctx.include_max_output_tokens = false;
        ctx.include_output_types = false;
        ctx.include_sampling_parameters = false;
        ctx.force_response_store_false = true;
        ctx.include_encrypted_reasoning = true;

        let payload = build_responses_request(&request, &ctx).expect("chatgpt responses request should build");

        assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(false));
        assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
        assert!(payload.get("max_output_tokens").is_none());
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());
        assert!(payload.get("parallel_tool_calls").is_none());
        assert!(payload.get("previous_response_id").is_none());
        assert_eq!(
            payload.get("include").and_then(Value::as_array),
            Some(&vec![json!("output_text.annotations"), json!("reasoning.encrypted_content"),])
        );
    }

    #[test]
    fn chatgpt_request_construction_preserves_requested_stream_mode() {
        for requested_stream in [false, true] {
            let mut request = request();
            request.stream = requested_stream;

            let payload =
                build_responses_request(&request, &base_context(None)).expect("chatgpt responses request should build");

            assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(requested_stream));
        }
    }

    fn gpt6_astra_request() -> provider::LLMRequest {
        let mut request = request();
        request.model = models::openai::GPT_6_ASTRA.to_string();
        request
    }

    #[test]
    fn gpt6_astra_defaults_to_high_reasoning_effort() {
        let payload = build_responses_request(&gpt6_astra_request(), &base_context(None))
            .expect("astra responses request should build");

        assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("high"));
    }

    #[test]
    fn gpt6_astra_omits_sampling_parameters() {
        let mut request = gpt6_astra_request();
        request.temperature = Some(0.7);
        request.top_p = Some(0.9);
        request.presence_penalty = Some(0.1);
        request.frequency_penalty = Some(-0.5);

        let payload =
            build_responses_request(&request, &base_context(None)).expect("astra responses request should build");

        assert!(payload.get("sampling_parameters").is_none());
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());
    }

    #[test]
    fn gpt6_astra_uses_prompt_cache_options_ttl() {
        let mut ctx = base_context(None);
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");

        let payload =
            build_responses_request(&gpt6_astra_request(), &ctx).expect("astra responses request should build");

        assert_eq!(payload.pointer("/prompt_cache_options/ttl").and_then(Value::as_str), Some("30m"));
        assert!(payload.get("prompt_cache_retention").is_none());
    }

    #[test]
    fn gpt6_astra_strips_logprobs_from_include() {
        let mut request = gpt6_astra_request();
        request.responses_include = Some(vec![
            "message.output_text.logprobs".to_owned(),
            "reasoning.encrypted_content".to_owned(),
        ]);

        let payload =
            build_responses_request(&request, &base_context(None)).expect("astra responses request should build");

        assert_eq!(payload.get("include").and_then(Value::as_array), Some(&vec![json!("reasoning.encrypted_content")]));
    }

    #[test]
    fn gpt6_astra_receives_dedicated_addendum() {
        let instructions = augment_openai_instructions(models::openai::GPT_6_ASTRA, "Be helpful.".to_string());

        assert!(instructions.contains("GPT-6 Astra"));
        assert!(!instructions.contains("GPT-5.6 model"));
    }

    fn gpt6_sol_request() -> provider::LLMRequest {
        let mut request = request();
        request.model = models::openai::GPT_6_SOL.to_string();
        request
    }

    fn gpt6_luna_request() -> provider::LLMRequest {
        let mut request = request();
        request.model = models::openai::GPT_6_LUNA.to_string();
        request
    }

    #[test]
    fn gpt6_sol_defaults_to_medium_reasoning_effort() {
        let payload = build_responses_request(&gpt6_sol_request(), &base_context(None))
            .expect("sol responses request should build");

        assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("medium"));
    }

    #[test]
    fn gpt6_sol_explicit_reasoning_effort_overrides_model_default() {
        let mut request = gpt6_sol_request();
        request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::High);

        let payload =
            build_responses_request(&request, &base_context(None)).expect("sol responses request should build");

        assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("high"));
    }

    #[test]
    fn gpt6_luna_defaults_to_medium_reasoning_effort() {
        let payload = build_responses_request(&gpt6_luna_request(), &base_context(None))
            .expect("luna responses request should build");

        assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("medium"));
    }

    #[test]
    fn gpt6_sol_luna_omit_sampling_and_use_cache_ttl() {
        for mut request in [gpt6_sol_request(), gpt6_luna_request()] {
            request.temperature = Some(0.7);
            request.top_p = Some(0.9);
            let payload =
                build_responses_request(&request, &base_context(None)).expect("gpt-6 responses request should build");
            assert!(payload.get("sampling_parameters").is_none());
            assert!(payload.get("temperature").is_none());
        }

        for request in [gpt6_sol_request(), gpt6_luna_request()] {
            let mut ctx = base_context(None);
            ctx.include_prompt_cache_retention = true;
            ctx.prompt_cache_retention = Some("24h");
            let payload = build_responses_request(&request, &ctx).expect("gpt-6 responses request should build");
            assert_eq!(payload.pointer("/prompt_cache_options/ttl").and_then(Value::as_str), Some("30m"));
            assert!(payload.get("prompt_cache_retention").is_none());
        }
    }

    #[test]
    fn gpt6_sol_luna_receive_gpt6_addendum() {
        // Sol/Luna reuse the shared GPT-6 contract addendum (currently
        // Astra-named). Assert the exact shared text so a future
        // model-specific addendum split updates this test deliberately.
        for model in [models::openai::GPT_6_SOL, models::openai::GPT_6_LUNA] {
            let instructions = augment_openai_instructions(model, "Be helpful.".to_string());
            assert!(instructions.contains("GPT-6 Astra"));
            assert!(!instructions.contains("GPT-5.6 model"));
        }
    }

    fn cache_ctx() -> ResponsesRequestContext<'static> {
        let mut ctx = base_context(None);
        ctx.include_prompt_cache_retention = true;
        ctx
    }

    fn gpt56_request(messages: Vec<provider::Message>) -> provider::LLMRequest {
        provider::LLMRequest {
            messages: messages.into(),
            model: models::openai::GPT_5_6_LUNA.to_string(),
            stream: true,
            ..Default::default()
        }
    }

    fn count_explicit_breakpoints(payload: &Value) -> usize {
        payload
            .get("input")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("content").and_then(Value::as_array))
                    .flat_map(|blocks| blocks.iter())
                    .filter(|block| block.get("prompt_cache_breakpoint").is_some())
                    .count()
            })
            .unwrap_or(0)
    }

    fn breakpoint_modes(payload: &Value) -> Vec<String> {
        payload
            .get("input")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("content").and_then(Value::as_array))
                    .flat_map(|blocks| blocks.iter())
                    .filter_map(|block| {
                        block
                            .pointer("/prompt_cache_breakpoint/mode")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn gpt56_marks_stable_prefix_not_trailing_message() {
        let request = gpt56_request(vec![
            provider::Message::user("stable context".to_string()),
            provider::Message::user("new question".to_string()),
        ]);

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
        let input = payload.get("input").and_then(Value::as_array).expect("input");

        assert_eq!(input.len(), 2);
        let first_blocks = input[0].get("content").and_then(Value::as_array).expect("blocks");
        assert_eq!(
            first_blocks
                .last()
                .and_then(|b| b.pointer("/prompt_cache_breakpoint/mode"))
                .and_then(Value::as_str),
            Some("explicit"),
            "stable prefix boundary must carry the explicit breakpoint"
        );
        let last_blocks = input[1].get("content").and_then(Value::as_array).expect("blocks");
        assert!(
            last_blocks.iter().all(|b| b.get("prompt_cache_breakpoint").is_none()),
            "trailing varying message stays implicit-only so the stable prefix can partial-match"
        );
    }

    #[test]
    fn non_native_endpoints_omit_explicit_cache_breakpoints() {
        // Merge Gateway and other Responses-compatible endpoints reject
        // `prompt_cache_breakpoint` with 400 `invalid_parameter`; the provider
        // only sets the flag for first-party backends.
        let request = gpt56_request(vec![
            provider::Message::user("stable context".to_string()),
            provider::Message::user("new question".to_string()),
        ]);
        let mut ctx = cache_ctx();
        ctx.include_explicit_cache_breakpoints = false;

        let payload = build_responses_request(&request, &ctx).expect("request should build");
        assert_eq!(
            count_explicit_breakpoints(&payload),
            0,
            "compat endpoints must not receive prompt_cache_breakpoint markers"
        );
    }

    #[test]
    fn gpt61_sol_is_explicit_cache_breakpoint_model() {
        assert!(super::is_explicit_cache_breakpoint_model(models::openai::GPT_6_1_SOL));
    }

    #[test]
    fn gpt56_grown_history_keeps_breakpoint_byte_stable() {
        let first = gpt56_request(vec![provider::Message::user("stable context".to_string())]);
        let second = gpt56_request(vec![
            provider::Message::user("stable context".to_string()),
            provider::Message::user("follow-up".to_string()),
        ]);

        let a = build_responses_request(&first, &cache_ctx()).expect("first should build");
        let b = build_responses_request(&second, &cache_ctx()).expect("second should build");

        let a_input = a.get("input").and_then(Value::as_array).expect("input");
        let b_input = b.get("input").and_then(Value::as_array).expect("input");
        assert_eq!(&b_input[..a_input.len()], &a_input[..], "breakpoint must not rewrite the stable prefix");
        assert!(count_explicit_breakpoints(&b) >= 1);
    }

    #[test]
    fn gpt55_and_older_get_no_explicit_breakpoint() {
        let mut request = gpt56_request(vec![
            provider::Message::user("stable context".to_string()),
            provider::Message::user("new question".to_string()),
        ]);
        request.model = models::openai::GPT_5.to_string();

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        assert_eq!(count_explicit_breakpoints(&payload), 0, "older models reject the breakpoint field");
        assert!(payload.get("prompt_cache_options").is_none());
    }

    #[test]
    fn gpt56_caps_explicit_breakpoints_at_four() {
        let messages = (0..6).map(|i| provider::Message::user(format!("question {i}"))).collect();
        let request = gpt56_request(messages);

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        assert_eq!(count_explicit_breakpoints(&payload), 4, "writes are capped at four per request");
        assert!(breakpoint_modes(&payload).iter().all(|mode| mode == "explicit"));
    }

    #[test]
    fn gpt56_single_turn_still_writes_breakpoint() {
        let request = gpt56_request(vec![provider::Message::user("only question".to_string())]);

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        assert_eq!(count_explicit_breakpoints(&payload), 1, "first request must write the prefix");
    }

    fn system_with_dynamics() -> String {
        "stable instructions\n\n[Harness Limits]\n- max_tool_calls_per_turn: 5\n\n[Runtime Tool Catalog]\n- epoch: 2"
            .to_string()
    }

    #[test]
    fn responses_splits_dynamic_suffix_into_trailing_reminder() {
        use std::sync::Arc;

        let mut request = gpt56_request(vec![
            provider::Message::system("Previous conversation summary:\n- did stuff".to_string()),
            provider::Message::user("continue".to_string()),
        ]);
        request.system_prompt = Some(Arc::from(system_with_dynamics()));

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        let instructions = payload.get("instructions").and_then(Value::as_str).expect("instructions");
        assert!(instructions.contains("stable instructions"), "stable prefix stays cached");
        assert!(!instructions.contains("[Harness Limits]"), "volatile sections leave instructions");
        assert!(!instructions.contains("Previous conversation summary"), "history system leaves instructions");

        let input = payload.get("input").and_then(Value::as_array).expect("input");
        let reminder = input.last().expect("trailing reminder");
        assert_eq!(reminder.get("role").and_then(Value::as_str), Some("user"));
        let reminder_text = reminder
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .expect("reminder text");
        assert!(reminder_text.contains("[Harness Limits]"), "volatile sections ride the tail");
        assert!(reminder_text.contains("Previous conversation summary"), "history system rides the tail");
        assert!(reminder.pointer("/content/0/prompt_cache_breakpoint").is_none(), "varying tail stays implicit-only");
    }

    #[test]
    fn responses_static_prompts_keep_legacy_shape() {
        use std::sync::Arc;

        let mut request = gpt56_request(vec![provider::Message::user("hello".to_string())]);
        request.system_prompt = Some(Arc::from("stable instructions"));

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        let instructions = payload.get("instructions").and_then(Value::as_str).expect("instructions");
        assert!(instructions.contains("stable instructions"));
        let input = payload.get("input").and_then(Value::as_array).expect("input");
        assert_eq!(input.len(), 1, "no reminder without volatile content");
    }

    #[test]
    fn responses_grown_history_keeps_stable_instructions() {
        use std::sync::Arc;

        let first = {
            let mut request = gpt56_request(vec![provider::Message::user("one".to_string())]);
            request.system_prompt = Some(Arc::from(system_with_dynamics()));
            request
        };
        let second = {
            let mut request = gpt56_request(vec![
                provider::Message::user("one".to_string()),
                provider::Message::user("two".to_string()),
            ]);
            request.system_prompt = Some(Arc::from(system_with_dynamics()));
            request
        };

        let a = build_responses_request(&first, &cache_ctx()).expect("first should build");
        let b = build_responses_request(&second, &cache_ctx()).expect("second should build");

        assert_eq!(a.get("instructions"), b.get("instructions"), "stable instructions repeat verbatim");
        let a_input = a.get("input").and_then(Value::as_array).expect("input");
        let b_input = b.get("input").and_then(Value::as_array).expect("input");
        assert_eq!(a_input[0], b_input[0], "stable history item keeps its bytes");
    }

    #[test]
    fn responses_nonstructured_folds_move_to_reminder() {
        let mut ctx = cache_ctx();
        ctx.include_structured_history_in_input = false;
        let request = gpt56_request(vec![
            provider::Message::user("run it".to_string()),
            provider::Message::assistant("did it".to_string()),
            provider::Message::user("again".to_string()),
        ]);

        let payload = build_responses_request(&request, &ctx).expect("request should build");

        let instructions = payload.get("instructions").and_then(Value::as_str).unwrap_or("");
        assert!(
            !instructions.contains("Previous assistant response:"),
            "folded per-turn history leaves instructions"
        );
        let input = payload.get("input").and_then(Value::as_array).expect("input");
        let reminder = input.last().expect("trailing reminder");
        assert!(
            reminder
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("did it")),
            "folded history rides the tail"
        );
    }

    #[test]
    fn chat_splits_dynamic_suffix_to_trailing_system_message() {
        use std::sync::Arc;

        let mut request = request();
        request.model = "custom-chat-model".to_string();
        request.system_prompt = Some(Arc::from(system_with_dynamics()));
        request.messages = vec![provider::Message::user("hello".to_string())].into();

        let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");
        let messages = payload.get("messages").and_then(Value::as_array).expect("messages");

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].get("role").and_then(Value::as_str), Some("system"));
        assert!(
            messages[0]
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("stable instructions"))
        );
        assert!(
            messages[0]
                .get("content")
                .and_then(Value::as_str)
                .is_none_or(|text| !text.contains("[Harness Limits]"))
        );
        assert_eq!(messages[1].get("role").and_then(Value::as_str), Some("user"));
        assert_eq!(messages[2].get("role").and_then(Value::as_str), Some("system"));
        assert!(
            messages[2]
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("[Harness Limits]"))
        );
    }

    #[test]
    fn chat_without_dynamics_keeps_single_system_message() {
        use std::sync::Arc;

        let mut request = request();
        request.model = "custom-chat-model".to_string();
        request.system_prompt = Some(Arc::from("stable instructions"));
        request.messages = vec![provider::Message::user("hello".to_string())].into();

        let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");
        let messages = payload.get("messages").and_then(Value::as_array).expect("messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].get("role").and_then(Value::as_str), Some("system"));
        // Fast path must be byte-identical to the legacy single system message.
        let legacy = augment_openai_instructions("custom-chat-model", "stable instructions".to_string());
        assert_eq!(messages[0].get("content").and_then(Value::as_str), Some(legacy.as_str()));
    }

    #[test]
    fn responses_splits_mid_history_system_and_dynamics_without_panicking() {
        use std::sync::Arc;

        // Regression guard (review H1/M3): a System message positioned after an
        // assistant turn used to trip the prefix-strip heuristic / debug_assert.
        // Provenance-based segmentation must split every case deterministically.
        let mut request = gpt56_request(vec![
            provider::Message::assistant("did it".to_string()),
            provider::Message::system("resume note".to_string()),
            provider::Message::user("continue".to_string()),
        ]);
        request.system_prompt = Some(Arc::from(system_with_dynamics()));

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
        let instructions = payload.get("instructions").and_then(Value::as_str).expect("instructions");
        assert!(instructions.contains("stable instructions"));
        assert!(!instructions.contains("[Harness Limits]"), "dynamic sections leave instructions");
        assert!(!instructions.contains("resume note"), "mid-history system leaves instructions");

        let input = payload.get("input").and_then(Value::as_array).expect("input");
        let reminder = input.last().expect("trailing reminder");
        let reminder_text = reminder
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .expect("reminder text");
        assert!(reminder_text.contains("[Harness Limits]"));
        assert!(reminder_text.contains("resume note"));
        // History order before the tail is untouched: assistant then user.
        assert_eq!(input[0].get("role").and_then(Value::as_str), Some("assistant"));
        assert_eq!(input[1].get("role").and_then(Value::as_str), Some("user"));
    }

    #[test]
    fn responses_all_dynamic_system_prompt_keeps_contract_addendum() {
        use std::sync::Arc;

        // Regression guard (review L1): when the system prompt is entirely
        // dynamic, the GPT-5.6 contract addendum must still be emitted rather
        // than silently dropped.
        let mut request = gpt56_request(vec![provider::Message::user("hi".to_string())]);
        request.system_prompt = Some(Arc::from("[Harness Limits]\n- max_tool_calls_per_turn: 5"));

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
        let instructions = payload.get("instructions").and_then(Value::as_str).expect("instructions");
        assert!(
            instructions.contains("GPT-5.6"),
            "contract addendum must survive an all-dynamic prompt; got: {instructions:?}"
        );
        assert!(!instructions.contains("[Harness Limits]"), "got: {instructions:?}");
    }

    #[test]
    fn reminder_only_input_receives_no_explicit_breakpoint() {
        // Regression guard (review L2): a history of only System messages leaves
        // the reminder as the sole input item; marking it would churn every turn.
        let request = gpt56_request(vec![provider::Message::system("resume note".to_string())]);

        let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

        assert_eq!(count_explicit_breakpoints(&payload), 0, "varying reminder must stay unmarked");
    }
}
