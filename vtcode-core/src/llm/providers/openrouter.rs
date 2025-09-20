use crate::config::constants::{models, urls};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition, Usage,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value, json};

use super::extract_reasoning_trace;

#[derive(Default, Clone)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    fn finalize(self, fallback_index: usize) -> Option<ToolCall> {
        let name = self.name?;
        let id = self
            .id
            .unwrap_or_else(|| format!("tool_call_{}", fallback_index));
        let arguments = if self.arguments.is_empty() {
            "{}".to_string()
        } else {
            self.arguments
        };
        Some(ToolCall::function(id, name, arguments))
    }
}

fn update_tool_calls(builders: &mut Vec<ToolCallBuilder>, deltas: &[Value]) {
    for (index, delta) in deltas.iter().enumerate() {
        if builders.len() <= index {
            builders.push(ToolCallBuilder::default());
        }
        let builder = builders
            .get_mut(index)
            .expect("tool call builder must exist after push");

        if let Some(id) = delta.get("id").and_then(|v| v.as_str()) {
            builder.id = Some(id.to_string());
        }

        if let Some(function) = delta.get("function") {
            if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                builder.name = Some(name.to_string());
            }

            if let Some(arguments_value) = function.get("arguments") {
                if let Some(arguments) = arguments_value.as_str() {
                    builder.arguments.push_str(arguments);
                } else if arguments_value.is_object() || arguments_value.is_array() {
                    builder.arguments.push_str(&arguments_value.to_string());
                }
            }
        }
    }
}

fn finalize_tool_calls(builders: Vec<ToolCallBuilder>) -> Option<Vec<ToolCall>> {
    let calls: Vec<ToolCall> = builders
        .into_iter()
        .enumerate()
        .filter_map(|(index, builder)| builder.finalize(index))
        .collect();

    if calls.is_empty() { None } else { Some(calls) }
}

fn push_reasoning_segment(segments: &mut Vec<String>, text: String) {
    let trimmed = text.trim();
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

fn apply_tool_call_delta_from_content(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
) {
    let (index, delta_source) = if let Some(tool_call_value) = container.get("tool_call") {
        match tool_call_value.as_object() {
            Some(tool_call) => {
                let idx = tool_call
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0) as usize;
                (idx, tool_call)
            }
            None => (0usize, container),
        }
    } else {
        let idx = container
            .get("index")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as usize;
        (idx, container)
    };

    let mut delta_map = Map::new();

    if let Some(id_value) = delta_source.get("id") {
        delta_map.insert("id".to_string(), id_value.clone());
    }

    if let Some(function_value) = delta_source.get("function") {
        delta_map.insert("function".to_string(), function_value.clone());
    }

    if delta_map.is_empty() {
        return;
    }

    if builders.len() <= index {
        builders.resize_with(index + 1, ToolCallBuilder::default);
    }

    let mut deltas = vec![Value::Null; index + 1];
    deltas[index] = Value::Object(delta_map);
    update_tool_calls(builders, &deltas);
}

fn process_content_object(
    map: &Map<String, Value>,
    aggregated_content: &mut String,
    reasoning_segments: &mut Vec<String>,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    delta_text: &mut String,
) {
    if let Some(content_type) = map.get("type").and_then(|value| value.as_str()) {
        match content_type {
            "reasoning" => {
                if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
                    push_reasoning_segment(reasoning_segments, text_value.to_string());
                }
                return;
            }
            "tool_call_delta" | "tool_call" => {
                apply_tool_call_delta_from_content(tool_call_builders, map);
                return;
            }
            _ => {}
        }
    }

    if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
        if !text_value.is_empty() {
            aggregated_content.push_str(text_value);
            delta_text.push_str(text_value);
        }
        return;
    }

    if let Some(inner_content) = map.get("content") {
        let nested_delta = process_content_value(
            inner_content,
            aggregated_content,
            reasoning_segments,
            tool_call_builders,
        );
        if !nested_delta.is_empty() {
            delta_text.push_str(&nested_delta);
        }
    }
}

fn process_content_part(
    part: &Value,
    aggregated_content: &mut String,
    reasoning_segments: &mut Vec<String>,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    delta_text: &mut String,
) {
    if let Some(text) = part.as_str() {
        if !text.is_empty() {
            aggregated_content.push_str(text);
            delta_text.push_str(text);
        }
        return;
    }

    if let Some(map) = part.as_object() {
        process_content_object(
            map,
            aggregated_content,
            reasoning_segments,
            tool_call_builders,
            delta_text,
        );
        return;
    }

    if part.is_array() {
        let nested_delta = process_content_value(
            part,
            aggregated_content,
            reasoning_segments,
            tool_call_builders,
        );
        if !nested_delta.is_empty() {
            delta_text.push_str(&nested_delta);
        }
    }
}

fn process_content_value(
    value: &Value,
    aggregated_content: &mut String,
    reasoning_segments: &mut Vec<String>,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
) -> String {
    let mut delta_text = String::new();

    match value {
        Value::String(text) => {
            if !text.is_empty() {
                aggregated_content.push_str(text);
                delta_text.push_str(text);
            }
        }
        Value::Array(parts) => {
            for part in parts {
                process_content_part(
                    part,
                    aggregated_content,
                    reasoning_segments,
                    tool_call_builders,
                    &mut delta_text,
                );
            }
        }
        Value::Object(map) => {
            process_content_object(
                map,
                aggregated_content,
                reasoning_segments,
                tool_call_builders,
                &mut delta_text,
            );
        }
        _ => {}
    }

    delta_text
}

fn parse_usage_value(value: &Value) -> Usage {
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
    }
}

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        other => FinishReason::Error(other.to_string()),
    }
}

fn parse_stream_payload(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning_segments: &mut Vec<String>,
    usage: &mut Option<Usage>,
    finish_reason: &mut FinishReason,
) -> Option<String> {
    let mut emitted_delta = String::new();

    if let Some(choices) = payload.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(delta) = choice.get("delta") {
                if let Some(content_value) = delta.get("content") {
                    let text_delta = process_content_value(
                        content_value,
                        aggregated_content,
                        reasoning_segments,
                        tool_call_builders,
                    );
                    if !text_delta.is_empty() {
                        emitted_delta.push_str(&text_delta);
                    }
                }

                if let Some(reasoning_value) = delta.get("reasoning") {
                    if let Some(reasoning_text) = extract_reasoning_trace(reasoning_value) {
                        push_reasoning_segment(reasoning_segments, reasoning_text);
                    }
                }

                if let Some(tool_calls_value) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    update_tool_calls(tool_call_builders, tool_calls_value);
                }
            }

            if let Some(reasoning_value) = choice.get("reasoning") {
                if let Some(reasoning_text) = extract_reasoning_trace(reasoning_value) {
                    push_reasoning_segment(reasoning_segments, reasoning_text);
                }
            }

            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                *finish_reason = map_finish_reason(reason);
            }
        }
    }

    if let Some(usage_value) = payload.get("usage") {
        *usage = Some(parse_usage_value(usage_value));
    }

    if let Some(reasoning_value) = payload.get("reasoning") {
        if let Some(reasoning_text) = extract_reasoning_trace(reasoning_value) {
            push_reasoning_segment(reasoning_segments, reasoning_text);
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
    reasoning_segments: Vec<String>,
) -> LLMResponse {
    let content = if aggregated_content.is_empty() {
        None
    } else {
        Some(aggregated_content)
    };

    let reasoning = if reasoning_segments.is_empty() {
        None
    } else {
        Some(reasoning_segments.join("\n"))
    };

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
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::openrouter::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: urls::OPENROUTER_API_BASE.to_string(),
            model,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let mut provider = if let Some(model_value) = model {
            Self::with_model(api_key_value, model_value)
        } else {
            Self::new(api_key_value)
        };
        if let Some(base) = base_url {
            provider.base_url = base;
        }
        provider
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
            .map(|s| s.to_string())
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|r| r.get("effort"))
                    .and_then(|effort| effort.as_str())
                    .map(|s| s.to_string())
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

    fn convert_to_openrouter_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
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
            "model": if request.model.trim().is_empty() {
                &self.model
            } else {
                &request.model
            },
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

        if let Some(reasoning) = &request.reasoning_effort {
            provider_request["reasoning"] = json!({"effort": reasoning});
        }

        Ok(provider_request)
    }

    fn parse_openrouter_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let choices = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenRouter",
                    "Invalid response format: missing choices",
                );
                LLMError::Provider(formatted_error)
            })?;

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

        let content = match message.get("content") {
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

        let reasoning = message
            .get("reasoning")
            .and_then(extract_reasoning_trace)
            .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace));

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|fr| fr.as_str())
            .map(map_finish_reason)
            .unwrap_or(FinishReason::Stop);

        let usage = response_json.get("usage").map(parse_usage_value);

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

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let mut provider_request = self.convert_to_openrouter_format(&request)?;
        provider_request["stream"] = Value::Bool(true);

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&provider_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 429 || error_text.contains("quota") {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregated_content = String::new();
            let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
            let mut reasoning_segments: Vec<String> = Vec::new();
            let mut usage: Option<Usage> = None;
            let mut finish_reason = FinishReason::Stop;
            let mut done = false;

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|err| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenRouter",
                        &format!("Streaming error: {}", err),
                    );
                    LLMError::Network(formatted_error)
                })?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(split_idx) = buffer.find("\n\n") {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + 2);

                    let mut encountered_done = false;

                    for line in event.lines() {
                        if !line.starts_with("data:") {
                            continue;
                        }
                        let data = line.trim_start_matches("data:").trim();
                        if data.is_empty() {
                            continue;
                        }
                        if data == "[DONE]" {
                            encountered_done = true;
                            break;
                        }

                        let payload: Value = serde_json::from_str(data).map_err(|err| {
                            let formatted_error = error_display::format_llm_error(
                                "OpenRouter",
                                &format!("Failed to parse stream payload: {}", err),
                            );
                            LLMError::Provider(formatted_error)
                        })?;

                        if let Some(delta) = parse_stream_payload(
                            &payload,
                            &mut aggregated_content,
                            &mut tool_call_builders,
                            &mut reasoning_segments,
                            &mut usage,
                            &mut finish_reason,
                        ) {
                            if !delta.is_empty() {
                                yield LLMStreamEvent::Token { delta };
                            }
                        }
                    }

                    if encountered_done {
                        done = true;
                        break;
                    }
                }

                if done {
                    break;
                }
            }

            if !done && !buffer.trim().is_empty() {
                let mut encountered_done = false;
                for line in buffer.lines() {
                    if !line.starts_with("data:") {
                        continue;
                    }
                    let data = line.trim_start_matches("data:").trim();
                    if data.is_empty() {
                        continue;
                    }
                    if data == "[DONE]" {
                        encountered_done = true;
                        break;
                    }

                    let payload: Value = serde_json::from_str(data).map_err(|err| {
                        let formatted_error = error_display::format_llm_error(
                            "OpenRouter",
                            &format!("Failed to parse stream payload: {}", err),
                        );
                        LLMError::Provider(formatted_error)
                    })?;

                    if let Some(delta) = parse_stream_payload(
                        &payload,
                        &mut aggregated_content,
                        &mut tool_call_builders,
                        &mut reasoning_segments,
                        &mut usage,
                        &mut finish_reason,
                    ) {
                        if !delta.is_empty() {
                            yield LLMStreamEvent::Token { delta };
                        }
                    }
                }

                if encountered_done {
                    // All buffered events processed; nothing more to stream.
                }
            }

            let response = finalize_stream_response(
                aggregated_content,
                tool_call_builders,
                usage,
                finish_reason,
                reasoning_segments,
            );

            yield LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let provider_request = self.convert_to_openrouter_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&provider_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 429 || error_text.contains("quota") {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "OpenRouter",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

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
