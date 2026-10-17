//! StepFun provider backed by the native Responses API
//! (`POST {base}/responses`).
//!
//! StepFun's `/responses` surface is wire-compatible with the OpenAI Responses
//! API: history is replayed as `input` items (`message`, `function_call`,
//! `function_call_output`), the system prompt travels as top-level
//! `instructions`, reasoning effort is encoded as `reasoning.effort`, and the
//! output-token budget uses `max_output_tokens`.
//!
//! This replaces the previous chat-completions implementation. The provider
//! keeps the registration contract expected by `impl_standard_provider_constructor!`
//! (type name + 7-argument `from_config`) and reuses the shared Responses
//! streaming adapter so normalized stream events stay consistent with the native
//! OpenAI provider.

use crate::error_display;
use crate::provider::{
    FinishReason, LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    MessageContent, MessageRole, ToolChoice, ToolDefinition, Usage,
};
use crate::providers::common::{
    ensure_model, impl_llm_client, override_base_url, parse_json_response, read_provider_error_body, resolve_model,
    sampling_param_f64, validate_supported_models,
};
use crate::providers::error_handling::{format_network_error, format_parse_error};
use crate::providers::openai::tool_serialization::sanitize_openai_function_parameters;
use crate::providers::shared::{
    ResponsesNormalizedStreamOptions, StreamAggregator, Utf8StreamDecoder, create_responses_normalized_stream,
    extract_data_payload, find_sse_boundary_bytes, function_output_value_from_message_content,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use vtcode_config::types::ReasoningEffortLevel;

const PROVIDER_NAME: &str = "StepFun";
const PROVIDER_KEY: &str = "stepfun";
const LEGACY_API_KEY_ENV: &str = "STEP_API_KEY";

/// Maps VT Code's effort ladder onto StepFun's `reasoning.effort` vocabulary.
fn reasoning_effort_value(effort: ReasoningEffortLevel) -> Option<&'static str> {
    match effort {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
        ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => Some("low"),
        ReasoningEffortLevel::Medium => Some("medium"),
        ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh | ReasoningEffortLevel::Max => Some("high"),
    }
}

/// Render one user/assistant content block list for the `input` array.
fn user_content_parts(content: &MessageContent) -> Vec<Value> {
    use crate::provider::ContentPart;

    let mut parts = Vec::new();
    match content {
        MessageContent::Text(text) => {
            if !text.trim().is_empty() {
                parts.push(json!({"type": "input_text", "text": text}));
            }
        }
        MessageContent::Parts(content_parts) => {
            for part in content_parts {
                match part {
                    ContentPart::Text { text } => {
                        if !text.trim().is_empty() {
                            parts.push(json!({"type": "input_text", "text": text}));
                        }
                    }
                    ContentPart::Image { data, mime_type, image_url, detail, .. } => {
                        let image_url = match image_url {
                            Some(url) => url.clone(),
                            None => format!("data:{mime_type};base64,{data}"),
                        };
                        let mut image_part = json!({"type": "input_image", "image_url": image_url});
                        if let Some(detail) = detail {
                            image_part["detail"] = json!(detail.as_str());
                        }
                        parts.push(image_part);
                    }
                    ContentPart::File { filename, file_id, file_url, .. } => {
                        // StepFun documents no file input block for the Responses
                        // API; degrade to a text note rather than sending an
                        // unsupported payload the server would reject.
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        parts.push(json!({
                            "type": "input_text",
                            "text": format!("[File input not directly supported: {fallback}]")
                        }));
                    }
                }
            }
        }
    }
    parts
}

/// `function_call_output.output` must be a string on the StepFun wire.
fn function_output_string(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

/// Flatten VT Code tool definitions into StepFun's `function` tool shape.
fn serialize_tools(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.function.as_ref()?;
            let mut serialized = json!({
                "type": "function",
                "name": function.name,
                "description": function.description,
                "parameters": sanitize_openai_function_parameters(function.parameters.clone(), true),
            });
            if tool.strict == Some(true) {
                serialized["strict"] = json!(true);
            }
            Some(serialized)
        })
        .collect()
}

/// Map VT Code's provider-agnostic `output_format` onto StepFun's
/// `text.format` descriptor.
fn text_format_from_output_format(output_format: &Value) -> Value {
    if let Some(json_schema) = output_format.get("json_schema") {
        let name = json_schema.get("name").and_then(Value::as_str).unwrap_or("response");
        let schema = json_schema.get("schema").cloned().unwrap_or_else(|| json!({"type": "object"}));
        return json!({"type": "json_schema", "name": name, "schema": schema});
    }

    if let Some(format_type) = output_format.get("type").and_then(Value::as_str) {
        if format_type == "json_schema"
            && let Some(schema) = output_format.get("schema")
        {
            let name = output_format.get("name").and_then(Value::as_str).unwrap_or("response");
            return json!({"type": "json_schema", "name": name, "schema": schema});
        }
        if format_type == "json_object" {
            return json!({"type": "json_object"});
        }
    }

    json!({"type": "json_object"})
}

pub struct StepFunProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: String,
    model_behavior: Option<ModelConfig>,
}

impl StepFunProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::stepfun::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, TimeoutsConfig::default(), None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: HttpClient,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            http_client,
            base_url,
            model,
            api_key,
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key = resolve_api_key(api_key);
        let resolved_model = resolve_model(model, models::stepfun::DEFAULT_MODEL);
        Self::with_model_internal(api_key, resolved_model, base_url, timeouts.unwrap_or_default(), model_behavior)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::http_client::HttpClientFactory;

        Self {
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(urls::STEPFUN_API_BASE, base_url, Some(env_vars::STEPFUN_BASE_URL)),
            model,
            api_key,
            model_behavior,
        }
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url.trim_end_matches('/'))
    }

    fn reasoning_enabled(model: &str) -> bool {
        models::stepfun::REASONING_MODELS.contains(&model)
    }

    fn model_behavior_flag(
        model_behavior: &Option<ModelConfig>,
        select: impl Fn(&ModelConfig) -> Option<bool>,
    ) -> bool {
        model_behavior.as_ref().and_then(select).unwrap_or(false)
    }

    /// Builds the `/responses` request payload.
    fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut instructions_segments: Vec<String> = Vec::new();
        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                instructions_segments.push(trimmed.to_owned());
            }
        }

        let mut input: Vec<Value> = Vec::new();
        let mut active_tool_calls: HashSet<String> = HashSet::new();
        let mut deferred_tool_outputs: HashMap<String, String> = HashMap::new();

        for (index, message) in request.messages.iter().enumerate() {
            match message.role {
                MessageRole::System => {
                    let text = message.content.as_text();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        instructions_segments.push(trimmed.to_owned());
                    }
                }
                MessageRole::User => {
                    let parts = user_content_parts(&message.content);
                    if !parts.is_empty() {
                        input.push(json!({"role": "user", "content": parts}));
                    }
                }
                MessageRole::Assistant => {
                    let text = message.content.as_text();
                    if !text.trim().is_empty() {
                        input.push(json!({"role": "assistant", "content": text.to_string()}));
                    }

                    if let Some(tool_calls) = &message.tool_calls {
                        for (call_index, call) in tool_calls.iter().enumerate() {
                            let Some(function) = &call.function else {
                                continue;
                            };

                            input.push(json!({
                                "type": "function_call",
                                "id": format!("fc_{index}_{call_index}"),
                                "call_id": call.id,
                                "name": function.name,
                                "arguments": function.arguments,
                            }));
                            active_tool_calls.insert(call.id.clone());

                            if let Some(output) = deferred_tool_outputs.remove(&call.id) {
                                active_tool_calls.remove(&call.id);
                                input.push(json!({
                                    "type": "function_call_output",
                                    "call_id": call.id,
                                    "output": output,
                                }));
                            }
                        }
                    }
                }
                MessageRole::Tool => {
                    let Some(call_id) = message.tool_call_id.as_ref() else {
                        return Err(LLMError::InvalidRequest {
                            message: error_display::format_llm_error(
                                PROVIDER_NAME,
                                "Tool messages must include tool_call_id for the Responses API",
                            ),
                            metadata: None,
                        });
                    };
                    let output = function_output_string(function_output_value_from_message_content(&message.content));

                    if active_tool_calls.remove(call_id) {
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": output,
                        }));
                    } else {
                        deferred_tool_outputs.insert(call_id.clone(), output);
                    }
                }
            }
        }

        // Every replayed `function_call` needs a paired output; synthesize one
        // so a partially paired history cannot fail replay.
        for call_id in active_tool_calls {
            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": "aborted",
            }));
        }

        let mut payload = json!({
            "model": request.model,
            "input": input,
            "stream": stream,
        });

        if !instructions_segments.is_empty() {
            payload["instructions"] = json!(instructions_segments.join("\n\n"));
        }

        let reasoning_active = request
            .reasoning_effort
            .is_some_and(|effort| effort != ReasoningEffortLevel::None);
        if !reasoning_active {
            if let Some(temperature) = request.temperature {
                payload["temperature"] = json!(sampling_param_f64(temperature));
            }
            if let Some(top_p) = request.top_p {
                payload["top_p"] = json!(sampling_param_f64(top_p));
            }
        }

        if let Some(max_tokens) = request.max_tokens {
            payload["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(effort) = request.reasoning_effort.and_then(reasoning_effort_value) {
            payload["reasoning"] = json!({"effort": effort});
        }

        // Keep tool definitions on the wire even when tools are disabled so the
        // rendered prefix stays cache-stable. StepFun accepts only
        // `tool_choice: "auto"`, so omit the choice field for ToolChoice::None
        // instead of removing the catalog (OpenAI caching guidance).
        if let Some(tools) = &request.tools {
            let serialized = serialize_tools(tools);
            if !serialized.is_empty() {
                payload["tools"] = Value::Array(serialized);
                if !matches!(request.tool_choice, Some(ToolChoice::None)) {
                    // StepFun currently accepts only the string "auto".
                    payload["tool_choice"] = json!("auto");
                }
            }
        }

        if let Some(output_format) = &request.output_format {
            payload["text"] = json!({"format": text_format_from_output_format(output_format)});
        }

        Ok(payload)
    }

    /// Parses a non-streaming `/responses` body (also used as the final-event
    /// parser for normalized streaming).
    fn parse_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let output = response_json
            .get("output")
            .and_then(Value::as_array)
            .ok_or_else(|| LLMError::Provider {
                message: error_display::format_llm_error(PROVIDER_NAME, "Invalid response: missing output array"),
                metadata: None,
            })?;

        let status = response_json.get("status").and_then(Value::as_str).unwrap_or("completed");
        if status == "failed" {
            let message = response_json
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("Response generation failed");
            return Err(LLMError::Provider {
                message: error_display::format_llm_error(PROVIDER_NAME, message),
                metadata: None,
            });
        }

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut tool_references = Vec::new();

        for item in output {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(content_parts) = item.get("content").and_then(Value::as_array) {
                        for part in content_parts {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                content.push_str(text);
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("id").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string();
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let arguments = item
                        .get("arguments")
                        .map(|value| {
                            if value.is_string() {
                                value.as_str().unwrap_or("{}").to_string()
                            } else {
                                value.to_string()
                            }
                        })
                        .unwrap_or_else(|| "{}".to_string());
                    tool_calls.push(crate::provider::ToolCall::function(call_id, name, arguments));
                }
                "reasoning" => {
                    if let Some(text) = item.get("content").and_then(Value::as_str) {
                        content_reasoning_append(&mut reasoning, text);
                    }
                    if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                        for part in summary {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                content_reasoning_append(&mut reasoning, text);
                            }
                        }
                    }
                }
                "tool_search_output" => {
                    crate::providers::shared::collect_tool_references_from_tool_search_output(
                        item,
                        &mut tool_references,
                    );
                }
                _ => {}
            }
        }

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            match status {
                "incomplete" => FinishReason::Length,
                _ => FinishReason::Stop,
            }
        };

        let usage = response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_value
                .get("input_tokens")
                .or_else(|| usage_value.get("prompt_tokens"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            completion_tokens: usage_value
                .get("output_tokens")
                .or_else(|| usage_value.get("completion_tokens"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            // StepFun advertises no prompt-cache metrics; do not surface them.
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            iterations: None,
        });

        Ok(LLMResponse {
            content: (!content.is_empty()).then_some(content),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            model,
            usage,
            finish_reason,
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
            reasoning_details: None,
            tool_references,
            request_id: response_json.get("id").and_then(Value::as_str).map(ToOwned::to_owned),
            organization_id: None,
            compaction: None,
        })
    }

    async fn generate_request(&self, request: &LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(request, false)?;

        let response = self
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| format_network_error(PROVIDER_NAME, &error))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error(PROVIDER_NAME, &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let json = parse_json_response(response, PROVIDER_NAME).await?;
        Self::parse_response(json, model)
    }

    /// Legacy `LLMStream` path over the StepFun Responses SSE wire.
    async fn stream_request(&self, request: &LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(request, true)?;

        let response = self
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| format_network_error(PROVIDER_NAME, &error))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error(PROVIDER_NAME, &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();
            let mut offset = 0usize;
            let mut decoder = Utf8StreamDecoder::new();
            let mut aggregator = StreamAggregator::new(model.clone());

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|error| format_network_error(PROVIDER_NAME, &error))?;
                decoder.push_bytes(&chunk, &mut buffer);

                while let Some((split_idx, delimiter_len)) = find_sse_boundary_bytes(&buffer, offset) {
                    let event = std::str::from_utf8(&buffer[offset..split_idx]).expect("valid utf-8 stream data");
                    offset = split_idx + delimiter_len;

                    let Some(data_payload) = extract_data_payload(event) else {
                        continue;
                    };
                    let trimmed = data_payload.trim();
                    if trimmed.is_empty() || trimmed == "[DONE]" {
                        continue;
                    }

                    let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                        continue;
                    };
                    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");

                    match event_type {
                        "response.output_text.delta" => {
                            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                                for stream_event in aggregator.handle_content(delta) {
                                    yield stream_event;
                                }
                            }
                        }
                        "response.reasoning.delta"
                        | "response.reasoning_content.delta"
                        | "response.reasoning_text.delta" => {
                            if let Some(delta) = event.get("delta").and_then(Value::as_str)
                                && let Some(delta) = aggregator.handle_reasoning(delta)
                            {
                                yield LLMStreamEvent::Reasoning { delta };
                            }
                        }
                        "response.reasoning_text.done" => {
                            let text = event
                                .get("text")
                                .and_then(Value::as_str)
                                .or_else(|| event.get("delta").and_then(Value::as_str));
                            if let Some(text) = text
                                && let Some(delta) = aggregator.handle_reasoning(text)
                            {
                                yield LLMStreamEvent::Reasoning { delta };
                            }
                        }
                        "response.function_call_arguments.delta" => {
                            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                                let call_id = event
                                    .get("item_id")
                                    .or_else(|| event.get("call_id"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                aggregator.handle_tool_calls(&[json!({
                                    "index": event.get("output_index").and_then(Value::as_u64).unwrap_or(0),
                                    "id": call_id,
                                    "function": {"arguments": delta},
                                })]);
                            }
                        }
                        "response.output_item.done" => {
                            if let Some(item) = event.get("item")
                                && item.get("type").and_then(Value::as_str) == Some("compaction")
                            {
                                aggregator.append_reasoning_detail(item);
                            }
                        }
                        "response.completed" => {
                            let streamed = aggregator.finalize();
                            let response = match event.get("response") {
                                Some(response_value) => Self::parse_response(response_value.clone(), model.clone())
                                    .unwrap_or(streamed),
                                None => streamed,
                            };
                            yield LLMStreamEvent::Completed { response: Box::new(response) };
                            return;
                        }
                        "response.incomplete" | "response.failed" => {
                            let message = event
                                .get("response")
                                .and_then(|response| response.get("error"))
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                                .unwrap_or("Response generation failed");
                            Err(LLMError::Provider {
                                message: error_display::format_llm_error(PROVIDER_NAME, message),
                                metadata: None,
                            })?;
                        }
                        "error" => {
                            let message = event
                                .get("error")
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                                .unwrap_or("Unknown error from StepFun Responses API");
                            Err(LLMError::Provider {
                                message: error_display::format_llm_error(PROVIDER_NAME, message),
                                metadata: None,
                            })?;
                        }
                        _ => {}
                    }
                }

                if offset > 0 {
                    buffer.drain(..offset);
                    offset = 0;
                }
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }
}

/// Appends a reasoning fragment, separating fragments with a blank line.
fn content_reasoning_append(reasoning: &mut String, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if !reasoning.is_empty() {
        reasoning.push_str("\n\n");
    }
    reasoning.push_str(text);
}

fn resolve_api_key(api_key: Option<String>) -> String {
    api_key
        .or_else(|| std::env::var("STEPFUN_API_KEY").ok().filter(|key| !key.trim().is_empty()))
        .or_else(|| std::env::var(LEGACY_API_KEY_ENV).ok().filter(|key| !key.trim().is_empty()))
        .unwrap_or_default()
}

#[async_trait]
impl LLMProvider for StepFunProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        // `/responses` services `stream: false`; pinned so the runloop's
        // stream-timeout fallback to non-streaming cannot silently regress.
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() { &self.model } else { model };
        Self::model_behavior_flag(&self.model_behavior, |behavior| behavior.model_supports_reasoning)
            || Self::reasoning_enabled(requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() { &self.model } else { model };
        Self::model_behavior_flag(&self.model_behavior, |behavior| behavior.model_supports_reasoning_effort)
            || Self::reasoning_enabled(requested)
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, _model: &str) -> bool {
        true
    }

    fn effective_context_size(&self, model: &str) -> usize {
        crate::provider::catalog_context_window(PROVIDER_KEY, model, 262_144)
    }

    fn supported_models(&self) -> Vec<String> {
        models::stepfun::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_supported_models(request, PROVIDER_NAME, PROVIDER_KEY, models::stepfun::SUPPORTED_MODELS)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        self.generate_request(&request).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        request.stream = true;
        self.stream_request(&request).await
    }

    async fn stream_normalized(&self, mut request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;

        let response = self
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| format_network_error(PROVIDER_NAME, &error))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error(PROVIDER_NAME, &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let emit_reasoning = self.supports_reasoning(&model);
        Ok(create_responses_normalized_stream(
            response,
            ResponsesNormalizedStreamOptions {
                provider_name: PROVIDER_NAME,
                model: model.clone(),
                emit_reasoning,
                include_cached_prompt_metrics: false,
            },
            move |value| Self::parse_response(value, model.clone()),
        ))
    }
}

impl_llm_client!(StepFunProvider);

#[cfg(test)]
mod tests {
    use super::StepFunProvider;
    use crate::provider::{LLMProvider, LLMRequest, Message, NormalizedStreamEvent, ToolCall, ToolDefinition};
    use futures::StreamExt;
    use serde_json::{Value, json};
    use std::sync::Arc;
    use vtcode_config::TimeoutsConfig;
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, ResponseTemplate};

    fn build_payload(request: &LLMRequest) -> Value {
        let provider = StepFunProvider::new("test-key".to_string());
        provider.build_payload(request, request.stream).expect("payload should build")
    }

    fn mock_provider(base_url: &str) -> StepFunProvider {
        let http_client = reqwest::Client::builder().no_proxy().build().expect("test client should build");
        StepFunProvider::new_with_client(
            "test-key".to_string(),
            models::stepfun::STEP_3_7_FLASH.to_string(),
            http_client,
            base_url.to_string(),
            TimeoutsConfig::default(),
        )
    }

    use crate::providers::test_support::start_mock_server_or_skip;

    fn completed_response_body() -> Value {
        json!({
            "id": "resp_step_1",
            "object": "response",
            "created_at": 1772624997,
            "completed_at": 1772624998,
            "model": models::stepfun::STEP_3_7_FLASH,
            "status": "completed",
            "error": null,
            "incomplete_details": null,
            "output": [
                {
                    "type": "reasoning",
                    "id": "rs_1",
                    "summary": [],
                    "content": null,
                    "encrypted_content": null,
                    "status": null
                },
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hello from stepfun", "annotations": []}]
                },
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"city\":\"Beijing\"}",
                    "status": "completed"
                }
            ],
            "usage": {
                "input_tokens": 14,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens": 52,
                "output_tokens_details": {"reasoning_tokens": 0, "tool_output_tokens": 0},
                "total_tokens": 66
            }
        })
    }

    #[tokio::test]
    async fn generate_posts_responses_request_and_parses_output() {
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .and(body_partial_json(json!({
                "model": models::stepfun::STEP_3_7_FLASH,
                "instructions": "system guidance",
                "stream": false,
                "max_output_tokens": 512,
                "reasoning": {"effort": "high"},
                "tool_choice": "auto",
                "input": [{"role": "user", "content": [{"type": "input_text", "text": "hello"}]}],
                "tools": [{"type": "function", "name": "get_weather"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(completed_response_body()))
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&format!("{}/v1", server.uri()));
        let response = provider
            .generate(LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                system_prompt: Some(Arc::from("system guidance")),
                max_tokens: Some(512),
                reasoning_effort: Some(ReasoningEffortLevel::XHigh),
                tools: Some(Arc::new(vec![ToolDefinition::function(
                    "get_weather".to_string(),
                    "Get the weather".to_string(),
                    json!({"type": "object"}),
                )])),
                tool_choice: Some(crate::provider::ToolChoice::Auto),
                ..Default::default()
            })
            .await
            .expect("generate should succeed");

        assert_eq!(response.content.as_deref(), Some("hello from stepfun"));
        assert_eq!(response.request_id.as_deref(), Some("resp_step_1"));
        assert_eq!(response.finish_reason, crate::provider::FinishReason::ToolCalls);
        let tool_calls = response.tool_calls.expect("tool calls should exist");
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].function.as_ref().map(|f| f.name.as_str()), Some("get_weather"));
        let usage = response.usage.expect("usage should exist");
        assert_eq!(usage.prompt_tokens, 14);
        assert_eq!(usage.completion_tokens, 52);
        assert_eq!(usage.total_tokens, 66);
    }

    #[tokio::test]
    async fn generate_surfaces_provider_error_body() {
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {"code": "invalid_request", "message": "bad input"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&format!("{}/v1", server.uri()));
        let error = provider
            .generate(LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                ..Default::default()
            })
            .await
            .expect_err("HTTP 400 should fail");

        assert!(error.to_string().contains("StepFun"), "error should name the provider: {error}");
        assert!(error.to_string().contains("400"), "error should include the status: {error}");
    }

    #[tokio::test]
    async fn stream_normalized_tolerates_stepfun_reasoning_boundaries() {
        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };

        // Mirrors the documented StepFun SSE shape, including the non-summary
        // `response.reasoning_part.added`/`.done` boundaries.
        let sse_body = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_step\",\"object\":\"response\",\"created_at\":1772624997,\"model\":\"step-3.7-flash\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[],\"content\":null,\"encrypted_content\":null,\"status\":\"in_progress\"}}\n\n",
            "event: response.reasoning_part.added\n",
            "data: {\"type\":\"response.reasoning_part.added\",\"sequence_number\":2,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"part\":{\"type\":\"reasoning_text\",\"text\":\"\"}}\n\n",
            "event: response.reasoning_text.delta\n",
            "data: {\"type\":\"response.reasoning_text.delta\",\"sequence_number\":3,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"delta\":\"thinking\"}\n\n",
            "event: response.reasoning_text.done\n",
            "data: {\"type\":\"response.reasoning_text.done\",\"sequence_number\":4,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"text\":\"thinking\"}\n\n",
            "event: response.reasoning_part.done\n",
            "data: {\"type\":\"response.reasoning_part.done\",\"sequence_number\":5,\"output_index\":0,\"item_id\":\"rs_1\",\"content_index\":0,\"part\":{\"type\":\"reasoning_text\",\"text\":\"thinking\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"sequence_number\":6,\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[],\"content\":null,\"encrypted_content\":null,\"status\":\"completed\"}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"sequence_number\":7,\"output_index\":1,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"in_progress\",\"content\":[]}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"sequence_number\":8,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":9,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"delta\":\"Hello\"}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"sequence_number\":10,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"text\":\"Hello\"}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"sequence_number\":11,\"item_id\":\"msg_1\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"sequence_number\":12,\"output_index\":1,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"sequence_number\":13,\"response\":{\"id\":\"resp_step\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12}}}\n\n",
        );

        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse_body, "text/event-stream"))
            .expect(1)
            .mount(&server)
            .await;

        let provider = mock_provider(&format!("{}/v1", server.uri()));
        let mut stream = provider
            .stream_normalized(LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                ..Default::default()
            })
            .await
            .expect("stream_normalized should start");

        let mut text = String::new();
        let mut reasoning = String::new();
        let mut done = None;
        while let Some(event) = stream.next().await {
            match event.expect("stream event should not error") {
                NormalizedStreamEvent::TextDelta { delta } => text.push_str(&delta),
                NormalizedStreamEvent::ReasoningDelta { delta, .. } => reasoning.push_str(&delta),
                NormalizedStreamEvent::Done { response } => {
                    done = Some(response);
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(text, "Hello");
        assert_eq!(reasoning, "thinking");
        let response = done.expect("stream should complete");
        assert_eq!(response.content.as_deref(), Some("Hello"));
        assert_eq!(response.request_id.as_deref(), Some("resp_step"));
        assert_eq!(response.usage.map(|usage| usage.total_tokens), Some(12));
    }

    #[test]
    fn step_5_preview_uses_1m_context_and_reasoning_effort() {
        let provider = StepFunProvider::new("test-key".to_string());
        assert_eq!(provider.effective_context_size(models::stepfun::STEP_5_PREVIEW), 1_048_576);
        assert_eq!(provider.effective_context_size(models::stepfun::STEP_3_7_FLASH), 262_144);
        assert!(provider.supports_reasoning(models::stepfun::STEP_5_PREVIEW));
        assert!(provider.supports_reasoning_effort(models::stepfun::STEP_5_PREVIEW));
        assert!(provider.supports_vision(models::stepfun::STEP_5_PREVIEW));
    }

    #[test]
    fn payload_maps_reasoning_effort_and_suppresses_sampling() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            reasoning_effort: Some(ReasoningEffortLevel::XHigh),
            temperature: Some(0.5),
            ..Default::default()
        });

        assert_eq!(payload["reasoning"]["effort"], "high");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());
    }

    #[test]
    fn unknown_effort_omits_reasoning_but_still_suppresses_sampling() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            temperature: Some(0.5),
            reasoning_effort: Some(ReasoningEffortLevel::Unknown),
            ..Default::default()
        });

        assert!(payload.get("reasoning").is_none());
        assert!(payload.get("temperature").is_none());

        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            temperature: Some(0.5),
            reasoning_effort: Some(ReasoningEffortLevel::Low),
            ..Default::default()
        });

        assert_eq!(payload["reasoning"]["effort"], "low");
        assert!(payload.get("temperature").is_none());
    }

    #[test]
    fn golden_payload_basic_shape() {
        use crate::provider::ToolChoice;

        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::from("system guidance")),
            max_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.25),
            stream: true,
            tool_choice: Some(ToolChoice::Auto),
            metadata: Some(json!({"user_id": "user-42"})),
            ..Default::default()
        });

        assert_eq!(payload["model"], models::stepfun::STEP_3_7_FLASH);
        assert_eq!(payload["instructions"], "system guidance");
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
        assert_eq!(payload["max_output_tokens"], 512);
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["top_p"], 0.25);
        assert_eq!(payload["stream"], true);
        // StepFun does not accept user metadata or chat-completions fields.
        assert!(payload.get("messages").is_none());
        assert!(payload.get("user_id").is_none());
        assert!(payload.get("max_tokens").is_none());
    }

    #[test]
    fn payload_inlines_vision_parts_as_input_images() {
        use crate::provider::ContentPart;

        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message {
                role: crate::provider::MessageRole::User,
                content: crate::provider::MessageContent::Parts(vec![
                    ContentPart::Text { text: "describe".to_string() },
                    ContentPart::Image {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                        content_type: "image".to_string(),
                        detail: None,
                        image_url: None,
                    },
                ]),
                ..Default::default()
            }]
            .into(),
            ..Default::default()
        });

        let content = payload["input"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,abc");
    }

    #[test]
    fn payload_flattens_tools_into_responses_shape() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather".to_string(),
                json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}),
            )])),
            tool_choice: Some(crate::provider::ToolChoice::Auto),
            ..Default::default()
        });

        let tool = &payload["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["name"], "get_weather");
        assert_eq!(tool["description"], "Get the weather");
        assert!(tool.get("function").is_none(), "Responses tools are flat, not nested");
        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn payload_keeps_tools_but_omits_choice_for_tool_choice_none() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather".to_string(),
                json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}),
            )])),
            tool_choice: Some(crate::provider::ToolChoice::None),
            ..Default::default()
        });

        assert!(payload.get("tools").is_some(), "tool definitions stay on the wire for cache stability");
        assert!(
            payload.get("tool_choice").is_none(),
            "StepFun accepts only tool_choice=auto, so none must omit the field"
        );
    }

    #[test]
    fn payload_replays_tool_calls_and_outputs() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![
                Message::user("weather?".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "get_weather".to_string(),
                        "{\"city\":\"Beijing\"}".to_string(),
                    )],
                ),
                Message::tool_response("call_1".to_string(), "{\"temperature\":22}".to_string()),
            ]
            .into(),
            ..Default::default()
        });

        let input = payload["input"].as_array().unwrap();
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[1]["name"], "get_weather");
        assert_eq!(input[1]["arguments"], "{\"city\":\"Beijing\"}");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_1");
        assert_eq!(input[2]["output"], "{\"temperature\":22}");
    }

    #[test]
    fn payload_synthesizes_aborted_output_for_orphan_call() {
        let payload = build_payload(&LLMRequest {
            model: models::stepfun::STEP_3_7_FLASH.to_string(),
            messages: vec![
                Message::user("weather?".to_string()),
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_orphan".to_string(),
                        "get_weather".to_string(),
                        "{}".to_string(),
                    )],
                ),
            ]
            .into(),
            ..Default::default()
        });

        let input = payload["input"].as_array().unwrap();
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_orphan");
        assert_eq!(input[2]["output"], "aborted");
    }

    #[test]
    fn parse_response_extracts_reasoning_text_and_tool_calls() {
        let response = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": null, "encrypted_content": null},
                {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
                 "content": [{"type": "output_text", "text": "done", "annotations": []}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "get_weather",
                 "arguments": "{\"city\":\"Beijing\"}", "status": "completed"}
            ],
            "usage": {
                "input_tokens": 14,
                "output_tokens": 52,
                "total_tokens": 66,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens_details": {"reasoning_tokens": 0, "tool_output_tokens": 0}
            }
        });

        let parsed = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
            .expect("response should parse");

        assert_eq!(parsed.content.as_deref(), Some("done"));
        assert_eq!(parsed.request_id.as_deref(), Some("resp_1"));
        let tool_calls = parsed.tool_calls.expect("tool calls should exist");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].function.as_ref().map(|f| f.name.as_str()), Some("get_weather"));
        assert_eq!(parsed.finish_reason, crate::provider::FinishReason::ToolCalls);
        let usage = parsed.usage.expect("usage should exist");
        assert_eq!(usage.prompt_tokens, 14);
        assert_eq!(usage.completion_tokens, 52);
        assert_eq!(usage.total_tokens, 66);
    }

    #[test]
    fn parse_response_maps_incomplete_status_to_length() {
        let response = json!({
            "id": "resp_incomplete",
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": [
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": "thinking", "encrypted_content": null}
            ]
        });

        let parsed = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
            .expect("response should parse");
        assert_eq!(parsed.finish_reason, crate::provider::FinishReason::Length);
        assert_eq!(parsed.reasoning.as_deref(), Some("thinking"));
    }

    #[test]
    fn parse_response_surfaces_failed_status() {
        let response = json!({
            "id": "resp_failed",
            "status": "failed",
            "error": {"code": "server_error", "message": "backend failed"},
            "output": []
        });

        let error = StepFunProvider::parse_response(response, models::stepfun::STEP_3_7_FLASH.to_string())
            .expect_err("failed response should error");
        assert!(error.to_string().contains("backend failed"));
    }

    #[test]
    fn validate_request_rejects_unknown_models() {
        let provider = StepFunProvider::new("test-key".to_string());
        let error = provider
            .validate_request(&LLMRequest {
                model: "not-a-stepfun-model".to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                ..Default::default()
            })
            .expect_err("unknown model should be rejected");
        assert!(error.to_string().contains("not-a-stepfun-model"));
    }
}
