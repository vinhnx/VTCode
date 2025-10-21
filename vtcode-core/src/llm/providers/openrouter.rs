use crate::config::constants::{models, urls};
use crate::config::core::{OpenRouterPromptCacheSettings, PromptCachingConfig};
use crate::config::models::{ModelId, Provider};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client as HttpClient, Response, StatusCode};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::str::FromStr;
#[cfg(debug_assertions)]
use tracing::debug;

use super::{
    ReasoningBuffer,
    common::{extract_prompt_cache_settings, override_base_url, resolve_model},
    extract_reasoning_trace, gpt5_codex_developer_prompt,
    shared::{
        StreamAssemblyError, StreamDelta, StreamFragment, StreamTelemetry, ToolCallBuilder,
        append_text_with_reasoning, apply_tool_call_delta_from_content, extract_data_payload,
        finalize_tool_calls, find_sse_boundary, update_tool_calls,
    },
    split_reasoning_from_text,
};

#[derive(Default)]
struct OpenRouterStreamTelemetry;

impl StreamTelemetry for OpenRouterStreamTelemetry {
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_content_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            length = delta.len(),
            "content delta received"
        );
    }

    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_reasoning_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            length = delta.len(),
            "reasoning delta received"
        );
    }

    fn on_tool_call_delta(&self) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            "tool call delta received"
        );
    }
}

fn append_reasoning_segment(segments: &mut Vec<String>, text: &str) {
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if segments
            .last()
            .map(|last| last.as_str() == trimmed)
            .unwrap_or(false)
        {
            continue;
        }
        segments.push(trimmed.to_string());
    }
}

fn process_content_object(
    map: &Map<String, Value>,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(content_type) = map.get("type").and_then(|value| value.as_str()) {
        match content_type {
            "reasoning" | "thinking" | "analysis" => {
                if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
                    if let Some(delta) = reasoning.push(text_value) {
                        telemetry.on_reasoning_delta(&delta);
                        deltas.push_reasoning(&delta);
                    }
                } else if let Some(text_value) =
                    map.get("output_text").and_then(|value| value.as_str())
                {
                    if let Some(delta) = reasoning.push(text_value) {
                        telemetry.on_reasoning_delta(&delta);
                        deltas.push_reasoning(&delta);
                    }
                }
                return;
            }
            "tool_call_delta" | "tool_call" => {
                apply_tool_call_delta_from_content(tool_call_builders, map, telemetry);
                return;
            }
            _ => {}
        }
    }

    if let Some(tool_call_value) = map.get("tool_call").and_then(|value| value.as_object()) {
        apply_tool_call_delta_from_content(tool_call_builders, tool_call_value, telemetry);
        return;
    }

    if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    if let Some(text_value) = map.get("output_text").and_then(|value| value.as_str()) {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    if let Some(text_value) = map
        .get("output_text_delta")
        .and_then(|value| value.as_str())
    {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    for key in ["content", "items", "output", "outputs", "delta"] {
        if let Some(inner) = map.get(key) {
            process_content_value(
                inner,
                aggregated_content,
                reasoning,
                tool_call_builders,
                deltas,
                telemetry,
            );
        }
    }
}

fn process_content_part(
    part: &Value,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(text) = part.as_str() {
        append_text_with_reasoning(text, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    if let Some(map) = part.as_object() {
        process_content_object(
            map,
            aggregated_content,
            reasoning,
            tool_call_builders,
            deltas,
            telemetry,
        );
        return;
    }

    if part.is_array() {
        process_content_value(
            part,
            aggregated_content,
            reasoning,
            tool_call_builders,
            deltas,
            telemetry,
        );
    }
}

fn process_content_value(
    value: &Value,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    match value {
        Value::String(text) => {
            append_text_with_reasoning(text, aggregated_content, reasoning, deltas, telemetry);
        }
        Value::Array(parts) => {
            for part in parts {
                process_content_part(
                    part,
                    aggregated_content,
                    reasoning,
                    tool_call_builders,
                    deltas,
                    telemetry,
                );
            }
        }
        Value::Object(map) => {
            process_content_object(
                map,
                aggregated_content,
                reasoning,
                tool_call_builders,
                deltas,
                telemetry,
            );
        }
        _ => {}
    }
}

fn extract_tool_calls_from_content(message: &Value) -> Option<Vec<ToolCall>> {
    let parts = message.get("content").and_then(|value| value.as_array())?;
    let mut calls: Vec<ToolCall> = Vec::new();

    for (index, part) in parts.iter().enumerate() {
        let map = match part.as_object() {
            Some(value) => value,
            None => continue,
        };

        let content_type = map.get("type").and_then(|value| value.as_str());
        let is_tool_call = matches!(content_type, Some("tool_call") | Some("function_call"))
            || (content_type.is_none()
                && map.contains_key("name")
                && map.contains_key("arguments"));

        if !is_tool_call {
            continue;
        }

        let id = map
            .get("id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("tool_call_{}", index));

        let (name, arguments_value) =
            if let Some(function) = map.get("function").and_then(|value| value.as_object()) {
                (
                    function
                        .get("name")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string()),
                    function.get("arguments"),
                )
            } else {
                (
                    map.get("name")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string()),
                    map.get("arguments"),
                )
            };

        let Some(name) = name else {
            continue;
        };

        let arguments = arguments_value
            .map(|value| {
                if let Some(text) = value.as_str() {
                    text.to_string()
                } else if value.is_null() {
                    "{}".to_string()
                } else {
                    value.to_string()
                }
            })
            .unwrap_or_else(|| "{}".to_string());

        calls.push(ToolCall::function(id, name, arguments));
    }

    if calls.is_empty() { None } else { Some(calls) }
}

fn extract_reasoning_from_message_content(message: &Value) -> Option<String> {
    let parts = message.get("content")?.as_array()?;
    let mut segments: Vec<String> = Vec::new();

    fn push_segment(segments: &mut Vec<String>, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        if segments
            .last()
            .map(|last| last.as_str() == trimmed)
            .unwrap_or(false)
        {
            return;
        }
        segments.push(trimmed.to_string());
    }

    for part in parts {
        match part {
            Value::Object(map) => {
                let part_type = map
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");

                if matches!(part_type, "reasoning" | "thinking" | "analysis") {
                    if let Some(extracted) = extract_reasoning_trace(part) {
                        if !extracted.trim().is_empty() {
                            segments.push(extracted);
                            continue;
                        }
                    }

                    if let Some(text) = map.get("text").and_then(|value| value.as_str()) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            push_segment(&mut segments, trimmed);
                        }
                    }
                }
            }
            Value::String(text) => {
                let (mut markup_segments, cleaned) = split_reasoning_from_text(text);
                if !markup_segments.is_empty() {
                    for segment in markup_segments.drain(..) {
                        push_segment(&mut segments, &segment);
                    }
                    if let Some(cleaned_text) = cleaned {
                        push_segment(&mut segments, &cleaned_text);
                    }
                } else {
                    push_segment(&mut segments, text);
                }
            }
            _ => {}
        }
    }

    if segments.is_empty() {
        None
    } else {
        let mut combined = String::new();
        for (idx, segment) in segments.iter().enumerate() {
            if idx > 0 {
                combined.push('\n');
            }
            combined.push_str(segment);
        }
        Some(combined)
    }
}

fn parse_usage_value(value: &Value) -> Usage {
    let cache_read_tokens = value
        .get("prompt_cache_read_tokens")
        .or_else(|| value.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let cache_creation_tokens = value
        .get("prompt_cache_write_tokens")
        .or_else(|| value.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Usage {
        prompt_tokens: value
            .get("prompt_tokens")
            .and_then(|pt| pt.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: value
            .get("completion_tokens")
            .and_then(|ct| ct.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: value
            .get("total_tokens")
            .and_then(|tt| tt.as_u64())
            .unwrap_or(0) as u32,
        cached_prompt_tokens: cache_read_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    }
}

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" | "completed" | "done" | "finished" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        other => FinishReason::Error(other.to_string()),
    }
}

fn push_reasoning_value(
    reasoning: &mut ReasoningBuffer,
    value: &Value,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(reasoning_text) = extract_reasoning_trace(value) {
        if let Some(delta) = reasoning.push(&reasoning_text) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    } else if let Some(text_value) = value.get("text").and_then(|v| v.as_str()) {
        if let Some(delta) = reasoning.push(text_value) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    }
}

fn parse_chat_completion_chunk(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> StreamDelta {
    let mut deltas = StreamDelta::default();

    if let Some(choices) = payload.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(delta) = choice.get("delta") {
                if let Some(content_value) = delta.get("content") {
                    process_content_value(
                        content_value,
                        aggregated_content,
                        reasoning,
                        tool_call_builders,
                        &mut deltas,
                        telemetry,
                    );
                }

                if let Some(reasoning_value) = delta.get("reasoning") {
                    push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
                }

                if let Some(tool_calls_value) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    update_tool_calls(tool_call_builders, tool_calls_value);
                }
            }

            if let Some(reasoning_value) = choice.get("reasoning") {
                push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
            }

            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                *finish_reason = map_finish_reason(reason);
            }
        }
    }

    deltas
}

fn parse_response_chunk(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> StreamDelta {
    let mut deltas = StreamDelta::default();

    if let Some(delta_value) = payload.get("delta") {
        process_content_value(
            delta_value,
            aggregated_content,
            reasoning,
            tool_call_builders,
            &mut deltas,
            telemetry,
        );
    }

    if let Some(event_type) = payload.get("type").and_then(|v| v.as_str()) {
        match event_type {
            "response.reasoning.delta" => {
                if let Some(delta_value) = payload.get("delta") {
                    push_reasoning_value(reasoning, delta_value, &mut deltas, telemetry);
                }
            }
            "response.tool_call.delta" => {
                if let Some(delta_object) = payload.get("delta").and_then(|v| v.as_object()) {
                    apply_tool_call_delta_from_content(tool_call_builders, delta_object, telemetry);
                }
            }
            "response.completed" | "response.done" | "response.finished" => {
                if let Some(response_obj) = payload.get("response") {
                    if aggregated_content.is_empty() {
                        process_content_value(
                            response_obj,
                            aggregated_content,
                            reasoning,
                            tool_call_builders,
                            &mut deltas,
                            telemetry,
                        );
                    }

                    if let Some(reason) = response_obj
                        .get("stop_reason")
                        .and_then(|value| value.as_str())
                        .or_else(|| response_obj.get("status").and_then(|value| value.as_str()))
                    {
                        *finish_reason = map_finish_reason(reason);
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(response_obj) = payload.get("response") {
        if aggregated_content.is_empty() {
            if let Some(content_value) = response_obj
                .get("output_text")
                .or_else(|| response_obj.get("output"))
                .or_else(|| response_obj.get("content"))
            {
                process_content_value(
                    content_value,
                    aggregated_content,
                    reasoning,
                    tool_call_builders,
                    &mut deltas,
                    telemetry,
                );
            }
        }
    }

    if let Some(reasoning_value) = payload.get("reasoning") {
        push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
    }

    deltas
}

fn update_usage_from_value(source: &Value, usage: &mut Option<Usage>) {
    if let Some(usage_value) = source.get("usage") {
        *usage = Some(parse_usage_value(usage_value));
    }
}

fn parse_stream_payload(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    usage: &mut Option<Usage>,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> Option<StreamDelta> {
    let mut emitted_delta = StreamDelta::default();

    let chat_delta = parse_chat_completion_chunk(
        payload,
        aggregated_content,
        tool_call_builders,
        reasoning,
        finish_reason,
        telemetry,
    );
    emitted_delta.extend(chat_delta);

    let response_delta = parse_response_chunk(
        payload,
        aggregated_content,
        tool_call_builders,
        reasoning,
        finish_reason,
        telemetry,
    );
    emitted_delta.extend(response_delta);

    update_usage_from_value(payload, usage);
    if let Some(response_obj) = payload.get("response") {
        update_usage_from_value(response_obj, usage);
        if let Some(reason) = response_obj
            .get("finish_reason")
            .and_then(|value| value.as_str())
        {
            *finish_reason = map_finish_reason(reason);
        }
    }

    if emitted_delta.is_empty() {
        None
    } else {
        Some(emitted_delta)
    }
}

fn finalize_stream_response(
    aggregated_content: String,
    tool_call_builders: Vec<ToolCallBuilder>,
    usage: Option<Usage>,
    finish_reason: FinishReason,
    reasoning: ReasoningBuffer,
) -> LLMResponse {
    let content = if aggregated_content.is_empty() {
        None
    } else {
        Some(aggregated_content)
    };

    let reasoning = reasoning.finalize();

    LLMResponse {
        content,
        tool_calls: finalize_tool_calls(tool_call_builders),
        usage,
        finish_reason,
        reasoning,
    }
}

pub struct OpenRouterProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenRouterPromptCacheSettings,
}

impl OpenRouterProvider {
    const TOOL_UNSUPPORTED_ERROR: &'static str = "No endpoints found that support tool use";

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openrouter::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openrouter::DEFAULT_MODEL);

        Self::with_model_internal(api_key_value, model_value, prompt_cache, base_url)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openrouter,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(urls::OPENROUTER_API_BASE, base_url),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn default_request(&self, prompt: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user(prompt.to_string())],
            system_prompt: None,
            tools: None,
            model: self.model.clone(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        }
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        let trimmed = prompt.trim_start();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                if let Some(request) = self.parse_chat_request(&value) {
                    return request;
                }
            }
        }

        self.default_request(prompt)
    }

    fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openrouter::OPENAI_GPT_5_CODEX
    }

    fn resolve_model<'a>(&'a self, request: &'a LLMRequest) -> &'a str {
        if request.model.trim().is_empty() {
            self.model.as_str()
        } else {
            request.model.as_str()
        }
    }

    fn uses_responses_api_for(&self, request: &LLMRequest) -> bool {
        Self::is_gpt5_codex_model(self.resolve_model(request))
    }

    fn request_includes_tools(request: &LLMRequest) -> bool {
        request
            .tools
            .as_ref()
            .map(|tools| !tools.is_empty())
            .unwrap_or(false)
    }

    fn enforce_tool_capabilities<'a>(&'a self, request: &'a LLMRequest) -> Cow<'a, LLMRequest> {
        let resolved_model = self.resolve_model(request);
        let tools_requested = Self::request_includes_tools(request);
        let tool_restricted = if let Ok(model_id) = ModelId::from_str(resolved_model) {
            !model_id.supports_tool_calls()
        } else {
            models::openrouter::TOOL_UNAVAILABLE_MODELS
                .iter()
                .any(|candidate| *candidate == resolved_model)
        };

        if tools_requested && tool_restricted {
            Cow::Owned(Self::tool_free_request(request))
        } else {
            Cow::Borrowed(request)
        }
    }

    fn tool_free_request(original: &LLMRequest) -> LLMRequest {
        let mut sanitized = original.clone();
        sanitized.tools = None;
        sanitized.tool_choice = Some(ToolChoice::None);
        sanitized.parallel_tool_calls = None;
        sanitized.parallel_tool_config = None;

        let mut normalized_messages: Vec<Message> = Vec::with_capacity(original.messages.len());

        for message in &original.messages {
            match message.role {
                MessageRole::Assistant => {
                    let mut cleaned = message.clone();
                    cleaned.tool_calls = None;
                    cleaned.tool_call_id = None;

                    let has_content = !cleaned.content.trim().is_empty();
                    if has_content || cleaned.reasoning.is_some() {
                        normalized_messages.push(cleaned);
                    }
                }
                MessageRole::Tool => {
                    if message.content.trim().is_empty() {
                        continue;
                    }

                    let mut converted = Message::user(message.content.clone());
                    converted.reasoning = message.reasoning.clone();
                    normalized_messages.push(converted);
                }
                _ => {
                    normalized_messages.push(message.clone());
                }
            }
        }

        sanitized.messages = normalized_messages;
        sanitized
    }

    fn build_provider_payload(&self, request: &LLMRequest) -> Result<(Value, String), LLMError> {
        if self.uses_responses_api_for(request) {
            Ok((
                self.convert_to_openrouter_responses_format(request)?,
                format!("{}/responses", self.base_url),
            ))
        } else {
            Ok((
                self.convert_to_openrouter_format(request)?,
                format!("{}/chat/completions", self.base_url),
            ))
        }
    }

    async fn dispatch_request(&self, url: &str, payload: &Value) -> Result<Response, LLMError> {
        self.http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })
    }

    fn is_tool_unsupported_error(status: StatusCode, body: &str) -> bool {
        status == StatusCode::NOT_FOUND && body.contains(Self::TOOL_UNSUPPORTED_ERROR)
    }

    async fn send_with_tool_fallback(
        &self,
        request: &LLMRequest,
        stream_override: Option<bool>,
    ) -> Result<Response, LLMError> {
        let adjusted_request = self.enforce_tool_capabilities(request);
        let request_ref = adjusted_request.as_ref();
        let request_with_tools = Self::request_includes_tools(request_ref);

        let (mut payload, url) = self.build_provider_payload(request_ref)?;
        if let Some(stream_flag) = stream_override {
            payload["stream"] = Value::Bool(stream_flag);
        }

        let response = self.dispatch_request(&url, &payload).await?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        if status.as_u16() == 429 || error_text.contains("quota") {
            return Err(LLMError::RateLimit);
        }

        if request_with_tools && Self::is_tool_unsupported_error(status, &error_text) {
            let fallback_request = Self::tool_free_request(request_ref);
            let (mut fallback_payload, fallback_url) =
                self.build_provider_payload(&fallback_request)?;
            if let Some(stream_flag) = stream_override {
                fallback_payload["stream"] = Value::Bool(stream_flag);
            }

            let fallback_response = self
                .dispatch_request(&fallback_url, &fallback_payload)
                .await?;
            if fallback_response.status().is_success() {
                return Ok(fallback_response);
            }

            let fallback_status = fallback_response.status();
            let fallback_text = fallback_response.text().await.unwrap_or_default();

            if fallback_status.as_u16() == 429 || fallback_text.contains("quota") {
                return Err(LLMError::RateLimit);
            }

            let combined_error = format!(
                "HTTP {}: {} | Tool fallback failed with HTTP {}: {}",
                status, error_text, fallback_status, fallback_text
            );
            let formatted_error = error_display::format_llm_error("OpenRouter", &combined_error);
            return Err(LLMError::Provider(formatted_error));
        }

        let formatted_error = error_display::format_llm_error(
            "OpenRouter",
            &format!("HTTP {}: {}", status, error_text),
        );
        Err(LLMError::Provider(formatted_error))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = None;
        let mut messages = Vec::new();

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(crate::config::constants::message_roles::USER);
            let content = entry.get("content");
            let text_content = content.map(Self::extract_content_text).unwrap_or_default();

            match role {
                "system" => {
                    if system_prompt.is_none() && !text_content.is_empty() {
                        system_prompt = Some(text_content);
                    }
                }
                "assistant" => {
                    let tool_calls = entry
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                        .map(|calls| {
                            calls
                                .iter()
                                .filter_map(|call| {
                                    let id = call.get("id").and_then(|v| v.as_str())?;
                                    let function = call.get("function")?;
                                    let name = function.get("name").and_then(|v| v.as_str())?;
                                    let arguments = function.get("arguments");
                                    let serialized = arguments.map_or("{}".to_string(), |value| {
                                        if value.is_string() {
                                            value.as_str().unwrap_or("").to_string()
                                        } else {
                                            value.to_string()
                                        }
                                    });
                                    Some(ToolCall::function(
                                        id.to_string(),
                                        name.to_string(),
                                        serialized,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        })
                        .filter(|calls| !calls.is_empty());

                    let message = if let Some(calls) = tool_calls {
                        Message {
                            role: MessageRole::Assistant,
                            content: text_content,
                            reasoning: None,
                            tool_calls: Some(calls),
                            tool_call_id: None,
                        }
                    } else {
                        Message::assistant(text_content)
                    };
                    messages.push(message);
                }
                "tool" => {
                    let tool_call_id = entry
                        .get("tool_call_id")
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string());
                    let content_value = entry
                        .get("content")
                        .map(|value| {
                            if text_content.is_empty() {
                                value.to_string()
                            } else {
                                text_content.clone()
                            }
                        })
                        .unwrap_or_else(|| text_content.clone());
                    messages.push(Message {
                        role: MessageRole::Tool,
                        content: content_value,
                        reasoning: None,
                        tool_calls: None,
                        tool_call_id,
                    });
                }
                _ => {
                    messages.push(Message::user(text_content));
                }
            }
        }

        if messages.is_empty() {
            return None;
        }

        let tools = value.get("tools").and_then(|tools_value| {
            let tools_array = tools_value.as_array()?;
            let converted: Vec<_> = tools_array
                .iter()
                .filter_map(|tool| {
                    let function = tool.get("function")?;
                    let name = function.get("name").and_then(|n| n.as_str())?;
                    let description = function
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let parameters = function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    Some(ToolDefinition::function(
                        name.to_string(),
                        description,
                        parameters,
                    ))
                })
                .collect();

            if converted.is_empty() {
                None
            } else {
                Some(converted)
            }
        });

        let max_tokens = value
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let stream = value
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tool_choice = value.get("tool_choice").and_then(Self::parse_tool_choice);
        let parallel_tool_calls = value.get("parallel_tool_calls").and_then(|v| v.as_bool());
        let reasoning_effort = value
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .and_then(ReasoningEffortLevel::parse)
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|r| r.get("effort"))
                    .and_then(|effort| effort.as_str())
                    .and_then(ReasoningEffortLevel::parse)
            });

        let model = value
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.model)
            .to_string();

        Some(LLMRequest {
            messages,
            system_prompt,
            tools,
            model,
            max_tokens,
            temperature,
            stream,
            tool_choice,
            parallel_tool_calls,
            parallel_tool_config: None,
            reasoning_effort,
        })
    }

    fn extract_content_text(content: &Value) -> String {
        match content {
            Value::String(text) => text.to_string(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|part| {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        Some(text.to_string())
                    } else if let Some(Value::String(text)) = part.get("content") {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    fn parse_tool_choice(choice: &Value) -> Option<ToolChoice> {
        match choice {
            Value::String(value) => match value.as_str() {
                "auto" => Some(ToolChoice::auto()),
                "none" => Some(ToolChoice::none()),
                "required" => Some(ToolChoice::any()),
                _ => None,
            },
            Value::Object(map) => {
                let choice_type = map.get("type").and_then(|t| t.as_str())?;
                match choice_type {
                    "function" => map
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|name| ToolChoice::function(name.to_string())),
                    "auto" => Some(ToolChoice::auto()),
                    "none" => Some(ToolChoice::none()),
                    "any" | "required" => Some(ToolChoice::any()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn build_standard_responses_input(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        let mut input = Vec::new();

        if let Some(system_prompt) = &request.system_prompt {
            if !system_prompt.trim().is_empty() {
                input.push(json!({
                    "role": "developer",
                    "content": [{
                        "type": "input_text",
                        "text": system_prompt.clone()
                    }]
                }));
            }
        }

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    if !msg.content.trim().is_empty() {
                        input.push(json!({
                            "role": "developer",
                            "content": [{
                                "type": "input_text",
                                "text": msg.content.clone()
                            }]
                        }));
                    }
                }
                MessageRole::User => {
                    input.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": msg.content.clone()
                        }]
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts = Vec::new();
                    if !msg.content.is_empty() {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.clone()
                        }));
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            content_parts.push(json!({
                                "type": "tool_call",
                                "id": call.id.clone(),
                                "name": call.function.name.clone(),
                                "arguments": call.function.arguments.clone()
                            }));
                        }
                    }

                    if !content_parts.is_empty() {
                        input.push(json!({
                            "role": "assistant",
                            "content": content_parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let tool_call_id = msg.tool_call_id.clone().ok_or_else(|| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            "Tool messages must include tool_call_id for Responses API",
                        );
                        LLMError::InvalidRequest(formatted_error)
                    })?;

                    let mut tool_content = Vec::new();
                    if !msg.content.trim().is_empty() {
                        tool_content.push(json!({
                            "type": "output_text",
                            "text": msg.content.clone()
                        }));
                    }

                    let mut tool_result = json!({
                        "type": "tool_result",
                        "tool_call_id": tool_call_id
                    });

                    if !tool_content.is_empty() {
                        if let Value::Object(ref mut map) = tool_result {
                            map.insert("content".to_string(), json!(tool_content));
                        }
                    }

                    input.push(json!({
                        "role": "tool",
                        "content": [tool_result]
                    }));
                }
            }
        }

        Ok(input)
    }

    fn build_codex_responses_input(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        let mut additional_guidance = Vec::new();

        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                additional_guidance.push(trimmed.to_string());
            }
        }

        let mut input = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    let trimmed = msg.content.trim();
                    if !trimmed.is_empty() {
                        additional_guidance.push(trimmed.to_string());
                    }
                }
                MessageRole::User => {
                    input.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": msg.content.clone()
                        }]
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts = Vec::new();
                    if !msg.content.is_empty() {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.clone()
                        }));
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            content_parts.push(json!({
                                "type": "tool_call",
                                "id": call.id.clone(),
                                "name": call.function.name.clone(),
                                "arguments": call.function.arguments.clone()
                            }));
                        }
                    }

                    if !content_parts.is_empty() {
                        input.push(json!({
                            "role": "assistant",
                            "content": content_parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let tool_call_id = msg.tool_call_id.clone().ok_or_else(|| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            "Tool messages must include tool_call_id for Responses API",
                        );
                        LLMError::InvalidRequest(formatted_error)
                    })?;

                    let mut tool_content = Vec::new();
                    if !msg.content.trim().is_empty() {
                        tool_content.push(json!({
                            "type": "output_text",
                            "text": msg.content.clone()
                        }));
                    }

                    let mut tool_result = json!({
                        "type": "tool_result",
                        "tool_call_id": tool_call_id
                    });

                    if !tool_content.is_empty() {
                        if let Value::Object(ref mut map) = tool_result {
                            map.insert("content".to_string(), json!(tool_content));
                        }
                    }

                    input.push(json!({
                        "role": "tool",
                        "content": [tool_result]
                    }));
                }
            }
        }

        let developer_prompt = gpt5_codex_developer_prompt(&additional_guidance);
        input.insert(
            0,
            json!({
                "role": "developer",
                "content": [{
                    "type": "input_text",
                    "text": developer_prompt
                }]
            }),
        );

        Ok(input)
    }

    fn convert_to_openrouter_responses_format(
        &self,
        request: &LLMRequest,
    ) -> Result<Value, LLMError> {
        let resolved_model = self.resolve_model(request);
        let input = if Self::is_gpt5_codex_model(resolved_model) {
            self.build_codex_responses_input(request)?
        } else {
            self.build_standard_responses_input(request)?
        };

        if input.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                "No messages provided for Responses API",
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut provider_request = json!({
            "model": resolved_model,
            "input": input,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            provider_request["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            provider_request["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let tools_json: Vec<Value> = tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.function.name,
                                "description": tool.function.description,
                                "parameters": tool.function.parameters
                            }
                        })
                    })
                    .collect();
                provider_request["tools"] = Value::Array(tools_json);
            }
        }

        if let Some(tool_choice) = &request.tool_choice {
            provider_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls {
            provider_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(resolved_model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenRouter, effort) {
                    provider_request["reasoning"] = payload;
                } else {
                    provider_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        if Self::is_gpt5_codex_model(resolved_model) {
            provider_request["reasoning"] = json!({ "effort": "medium" });
        }

        Ok(provider_request)
    }

    fn convert_to_openrouter_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let resolved_model = self.resolve_model(request);
        let mut messages = Vec::new();

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

            if msg.role == MessageRole::Assistant {
                if let Some(tool_calls) = &msg.tool_calls {
                    if !tool_calls.is_empty() {
                        let tool_calls_json: Vec<Value> = tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.function.name,
                                        "arguments": tc.function.arguments
                                    }
                                })
                            })
                            .collect();
                        message["tool_calls"] = Value::Array(tool_calls_json);
                    }
                }
            }

            if msg.role == MessageRole::Tool {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    message["tool_call_id"] = Value::String(tool_call_id.clone());
                }
            }

            messages.push(message);
        }

        if messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "No messages provided");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut provider_request = json!({
            "model": resolved_model,
            "messages": messages,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            provider_request["max_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            provider_request["temperature"] = json!(temperature);
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let tools_json: Vec<Value> = tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.function.name,
                                "description": tool.function.description,
                                "parameters": tool.function.parameters
                            }
                        })
                    })
                    .collect();
                provider_request["tools"] = Value::Array(tools_json);
            }
        }

        if let Some(tool_choice) = &request.tool_choice {
            provider_request["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls {
            provider_request["parallel_tool_calls"] = Value::Bool(parallel);
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(resolved_model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenRouter, effort) {
                    provider_request["reasoning"] = payload;
                } else {
                    provider_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        Ok(provider_request)
    }

    fn parse_openrouter_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        if let Some(choices) = response_json
            .get("choices")
            .and_then(|value| value.as_array())
        {
            if choices.is_empty() {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", "No choices in response");
                return Err(LLMError::Provider(formatted_error));
            }

            let choice = &choices[0];
            let message = choice.get("message").ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenRouter",
                    "Invalid response format: missing message",
                );
                LLMError::Provider(formatted_error)
            })?;

            let mut content = match message.get("content") {
                Some(Value::String(text)) => Some(text.to_string()),
                Some(Value::Array(parts)) => {
                    let text = parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("");
                    if text.is_empty() { None } else { Some(text) }
                }
                _ => None,
            };

            let tool_calls = message
                .get("tool_calls")
                .and_then(|tc| tc.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| {
                            let id = call.get("id").and_then(|v| v.as_str())?;
                            let function = call.get("function")?;
                            let name = function.get("name").and_then(|v| v.as_str())?;
                            let arguments = function.get("arguments");
                            let serialized = arguments.map_or("{}".to_string(), |value| {
                                if value.is_string() {
                                    value.as_str().unwrap_or("").to_string()
                                } else {
                                    value.to_string()
                                }
                            });
                            Some(ToolCall::function(
                                id.to_string(),
                                name.to_string(),
                                serialized,
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|calls| !calls.is_empty());

            let mut reasoning_segments: Vec<String> = Vec::new();

            if let Some(initial) = message
                .get("reasoning")
                .and_then(extract_reasoning_trace)
                .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
            {
                append_reasoning_segment(&mut reasoning_segments, &initial);
            }

            if reasoning_segments.is_empty() {
                if let Some(from_content) = extract_reasoning_from_message_content(message) {
                    append_reasoning_segment(&mut reasoning_segments, &from_content);
                }
            } else if let Some(extra) = extract_reasoning_from_message_content(message) {
                append_reasoning_segment(&mut reasoning_segments, &extra);
            }

            if let Some(original_content) = content.take() {
                let (markup_segments, cleaned) = split_reasoning_from_text(&original_content);
                for segment in markup_segments {
                    append_reasoning_segment(&mut reasoning_segments, &segment);
                }
                content = match cleaned {
                    Some(cleaned_text) => {
                        if cleaned_text.is_empty() {
                            None
                        } else {
                            Some(cleaned_text)
                        }
                    }
                    None => Some(original_content),
                };
            }

            let reasoning = if reasoning_segments.is_empty() {
                None
            } else {
                Some(reasoning_segments.join("\n"))
            };

            let finish_reason = choice
                .get("finish_reason")
                .and_then(|fr| fr.as_str())
                .map(map_finish_reason)
                .unwrap_or(FinishReason::Stop);

            let usage = response_json.get("usage").map(parse_usage_value);

            return Ok(LLMResponse {
                content,
                tool_calls,
                usage,
                finish_reason,
                reasoning,
            });
        }

        self.parse_responses_api_response(&response_json)
    }

    fn parse_responses_api_response(&self, payload: &Value) -> Result<LLMResponse, LLMError> {
        let response_container = payload.get("response").unwrap_or(payload);

        let outputs = response_container
            .get("output")
            .or_else(|| response_container.get("outputs"))
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenRouter",
                    "Invalid response format: missing output",
                );
                LLMError::Provider(formatted_error)
            })?;

        if outputs.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "No output in response");
            return Err(LLMError::Provider(formatted_error));
        }

        let message = outputs
            .iter()
            .find(|value| {
                value
                    .get("role")
                    .and_then(|role| role.as_str())
                    .map(|role| role == "assistant")
                    .unwrap_or(true)
            })
            .unwrap_or(&outputs[0]);

        let mut aggregated_content = String::new();
        let mut reasoning_buffer = ReasoningBuffer::default();
        let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
        let mut deltas = StreamDelta::default();
        let telemetry = OpenRouterStreamTelemetry::default();

        if let Some(content_value) = message.get("content") {
            process_content_value(
                content_value,
                &mut aggregated_content,
                &mut reasoning_buffer,
                &mut tool_call_builders,
                &mut deltas,
                &telemetry,
            );
        } else {
            process_content_value(
                message,
                &mut aggregated_content,
                &mut reasoning_buffer,
                &mut tool_call_builders,
                &mut deltas,
                &telemetry,
            );
        }

        let mut tool_calls = finalize_tool_calls(tool_call_builders);
        if tool_calls.is_none() {
            tool_calls = extract_tool_calls_from_content(message);
        }

        let mut reasoning_segments: Vec<String> = Vec::new();

        if let Some(buffer_reasoning) = reasoning_buffer.finalize() {
            append_reasoning_segment(&mut reasoning_segments, &buffer_reasoning);
        }

        let fallback_reasoning = extract_reasoning_from_message_content(message)
            .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
            .or_else(|| payload.get("reasoning").and_then(extract_reasoning_trace));

        if reasoning_segments.is_empty() {
            if let Some(extra) = fallback_reasoning {
                append_reasoning_segment(&mut reasoning_segments, &extra);
            }
        } else if let Some(extra) = fallback_reasoning {
            append_reasoning_segment(&mut reasoning_segments, &extra);
        }

        let mut content = if aggregated_content.is_empty() {
            message
                .get("output_text")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        } else {
            Some(aggregated_content)
        };

        if let Some(original_content) = content.take() {
            let (markup_segments, cleaned) = split_reasoning_from_text(&original_content);
            for segment in markup_segments {
                append_reasoning_segment(&mut reasoning_segments, &segment);
            }
            content = match cleaned {
                Some(cleaned_text) => {
                    if cleaned_text.is_empty() {
                        None
                    } else {
                        Some(cleaned_text)
                    }
                }
                None => Some(original_content),
            };
        }

        let reasoning = if reasoning_segments.is_empty() {
            None
        } else {
            Some(reasoning_segments.join("\n"))
        };

        let mut usage = payload.get("usage").map(parse_usage_value);
        if usage.is_none() {
            usage = response_container.get("usage").map(parse_usage_value);
        }

        let finish_reason = payload
            .get("stop_reason")
            .or_else(|| payload.get("finish_reason"))
            .or_else(|| payload.get("status"))
            .or_else(|| response_container.get("stop_reason"))
            .or_else(|| response_container.get("finish_reason"))
            .or_else(|| message.get("stop_reason"))
            .or_else(|| message.get("finish_reason"))
            .and_then(|value| value.as_str())
            .map(map_finish_reason)
            .unwrap_or(FinishReason::Stop);

        Ok(LLMResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
            reasoning,
        })
    }
}

#[async_trait]
impl LLMProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };
        if let Ok(model_id) = ModelId::from_str(requested) {
            return model_id.is_reasoning_variant();
        }
        models::openrouter::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        if let Ok(model_id) = ModelId::from_str(requested) {
            return model_id.supports_tool_calls();
        }

        !models::openrouter::TOOL_UNAVAILABLE_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let response = self.send_with_tool_fallback(&request, Some(true)).await?;

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregated_content = String::new();
            let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
            let mut reasoning = ReasoningBuffer::default();
            let mut usage: Option<Usage> = None;
            let mut finish_reason = FinishReason::Stop;
            let mut done = false;
            let telemetry = OpenRouterStreamTelemetry::default();

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|err| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenRouter",
                        &format!("Streaming error: {}", err),
                    );
                    LLMError::Network(formatted_error)
                })?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = extract_data_payload(&event) {
                        let trimmed_payload = data_payload.trim();
                        if trimmed_payload == "[DONE]" {
                            done = true;
                            break;
                        }

                        if !trimmed_payload.is_empty() {
                            let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                                StreamAssemblyError::InvalidPayload(err.to_string())
                                    .into_llm_error("OpenRouter")
                            })?;

                            if let Some(delta) = parse_stream_payload(
                                &payload,
                                &mut aggregated_content,
                                &mut tool_call_builders,
                                &mut reasoning,
                                &mut usage,
                                &mut finish_reason,
                                &telemetry,
                            ) {
                                for fragment in delta.into_fragments() {
                                    match fragment {
                                        StreamFragment::Content(text) if !text.is_empty() => {
                                            yield LLMStreamEvent::Token { delta: text };
                                        }
                                        StreamFragment::Reasoning(text) if !text.is_empty() => {
                                            yield LLMStreamEvent::Reasoning { delta: text };
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                if done {
                    break;
                }
            }

            if !done && !buffer.trim().is_empty() {
                if let Some(data_payload) = extract_data_payload(&buffer) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload != "[DONE]" && !trimmed_payload.is_empty() {
                        let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                            StreamAssemblyError::InvalidPayload(err.to_string())
                                .into_llm_error("OpenRouter")
                        })?;

                        if let Some(delta) = parse_stream_payload(
                            &payload,
                            &mut aggregated_content,
                            &mut tool_call_builders,
                            &mut reasoning,
                            &mut usage,
                            &mut finish_reason,
                            &telemetry,
                        ) {
                            for fragment in delta.into_fragments() {
                                match fragment {
                                    StreamFragment::Content(text) if !text.is_empty() => {
                                        yield LLMStreamEvent::Token { delta: text };
                                    }
                                    StreamFragment::Reasoning(text) if !text.is_empty() => {
                                        yield LLMStreamEvent::Reasoning { delta: text };
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            let response = finalize_stream_response(
                aggregated_content,
                tool_call_builders,
                usage,
                finish_reason,
                reasoning,
            );

            yield LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if self.prompt_cache_enabled && self.prompt_cache_settings.propagate_provider_capabilities {
            // When enabled, vtcode forwards provider-specific cache_control markers directly
            // through the OpenRouter payload without further transformation.
        }

        if self.prompt_cache_enabled && self.prompt_cache_settings.report_savings {
            // Cache savings are surfaced via usage metrics parsed later in the response cycle.
        }

        let response = self.send_with_tool_fallback(&request, None).await?;

        let openrouter_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        self.parse_openrouter_response(openrouter_response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::openrouter::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("OpenRouter", &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        if request.model.trim().is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "Model must be provided");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OpenRouterProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = self.parse_client_prompt(prompt);
        let request_model = request.model.clone();
        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: request_model,
            usage: response.usage.map(|u| llm_types::Usage {
                prompt_tokens: u.prompt_tokens as usize,
                completion_tokens: u.completion_tokens as usize,
                total_tokens: u.total_tokens as usize,
                cached_prompt_tokens: u.cached_prompt_tokens.map(|v| v as usize),
                cache_creation_tokens: u.cache_creation_tokens.map(|v| v as usize),
                cache_read_tokens: u.cache_read_tokens.map(|v| v as usize),
            }),
            reasoning: response.reasoning,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenRouter
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::shared::NoopStreamTelemetry;
    use serde_json::json;

    fn sample_tool() -> ToolDefinition {
        ToolDefinition::function(
            "fetch_data".to_string(),
            "Fetch data".to_string(),
            json!({
                "type": "object",
                "properties": {}
            }),
        )
    }

    fn request_with_tools(model: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hi".to_string())],
            system_prompt: None,
            tools: Some(vec![sample_tool()]),
            model: model.to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: Some(ToolChoice::Any),
            parallel_tool_calls: Some(true),
            parallel_tool_config: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn enforce_tool_capabilities_disables_tools_for_restricted_models() {
        let provider = OpenRouterProvider::with_model(
            "test-key".to_string(),
            models::openrouter::MOONSHOTAI_KIMI_K2_FREE.to_string(),
        );
        let request = request_with_tools(models::openrouter::MOONSHOTAI_KIMI_K2_FREE);

        match provider.enforce_tool_capabilities(&request) {
            Cow::Borrowed(_) => panic!("expected sanitized request"),
            Cow::Owned(sanitized) => {
                assert!(sanitized.tools.is_none());
                assert!(matches!(sanitized.tool_choice, Some(ToolChoice::None)));
                assert!(sanitized.parallel_tool_calls.is_none());
                assert_eq!(sanitized.model, models::openrouter::MOONSHOTAI_KIMI_K2_FREE);
                assert_eq!(sanitized.messages, request.messages);
            }
        }
    }

    #[test]
    fn enforce_tool_capabilities_keeps_tools_for_supported_models() {
        let provider = OpenRouterProvider::with_model(
            "test-key".to_string(),
            models::openrouter::OPENAI_GPT_5.to_string(),
        );
        let request = request_with_tools(models::openrouter::OPENAI_GPT_5);

        match provider.enforce_tool_capabilities(&request) {
            Cow::Borrowed(borrowed) => {
                assert!(std::ptr::eq(borrowed, &request));
                assert!(borrowed.tools.as_ref().is_some());
            }
            Cow::Owned(_) => panic!("should not sanitize supported models"),
        }
    }

    #[test]
    fn test_parse_stream_payload_chat_chunk() {
        let payload = json!({
            "choices": [{
                "delta": {
                    "content": [
                        {"type": "output_text", "text": "Hello"}
                    ]
                }
            }]
        });

        let mut aggregated = String::new();
        let mut builders = Vec::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut usage = None;
        let mut finish_reason = FinishReason::Stop;
        let telemetry = NoopStreamTelemetry::default();

        let delta = parse_stream_payload(
            &payload,
            &mut aggregated,
            &mut builders,
            &mut reasoning,
            &mut usage,
            &mut finish_reason,
            &telemetry,
        );

        let fragments = delta.expect("delta should exist").into_fragments();
        assert_eq!(
            fragments,
            vec![StreamFragment::Content("Hello".to_string())]
        );
        assert_eq!(aggregated, "Hello");
        assert!(builders.is_empty());
        assert!(usage.is_none());
        assert!(reasoning.finalize().is_none());
    }

    #[test]
    fn test_parse_stream_payload_response_delta() {
        let payload = json!({
            "type": "response.delta",
            "delta": {
                "type": "output_text_delta",
                "text": "Stream"
            }
        });

        let mut aggregated = String::new();
        let mut builders = Vec::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut usage = None;
        let mut finish_reason = FinishReason::Stop;
        let telemetry = NoopStreamTelemetry::default();

        let delta = parse_stream_payload(
            &payload,
            &mut aggregated,
            &mut builders,
            &mut reasoning,
            &mut usage,
            &mut finish_reason,
            &telemetry,
        );

        let fragments = delta.expect("delta should exist").into_fragments();
        assert_eq!(
            fragments,
            vec![StreamFragment::Content("Stream".to_string())]
        );
        assert_eq!(aggregated, "Stream");
    }

    #[test]
    fn test_extract_data_payload_joins_multiline_events() {
        let event = ": keep-alive\n".to_string() + "data: {\"a\":1}\n" + "data: {\"b\":2}\n";
        let payload = extract_data_payload(&event);
        assert_eq!(payload.as_deref(), Some("{\"a\":1}\n{\"b\":2}"));
    }

    #[test]
    fn parse_usage_value_includes_cache_metrics() {
        let value = json!({
            "prompt_tokens": 120,
            "completion_tokens": 80,
            "total_tokens": 200,
            "prompt_cache_read_tokens": 90,
            "prompt_cache_write_tokens": 15
        });

        let usage = parse_usage_value(&value);
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 80);
        assert_eq!(usage.total_tokens, 200);
        assert_eq!(usage.cached_prompt_tokens, Some(90));
        assert_eq!(usage.cache_read_tokens, Some(90));
        assert_eq!(usage.cache_creation_tokens, Some(15));
    }
}
