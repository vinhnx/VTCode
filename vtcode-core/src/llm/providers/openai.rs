use crate::config::constants::{models, urls};
use crate::config::core::{OpenAIPromptCacheSettings, PromptCachingConfig};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
#[cfg(debug_assertions)]
use tracing::debug;

use openai_harmony::chat::{
    Author as HarmonyAuthor, Content as HarmonyContent, Conversation, DeveloperContent,
    Message as HarmonyMessage, Role as HarmonyRole, ToolDescription,
};
use openai_harmony::{HarmonyEncodingName, load_harmony_encoding};

const MAX_COMPLETION_TOKENS_FIELD: &str = "max_completion_tokens";

use super::{
    ReasoningBuffer,
    common::{extract_prompt_cache_settings, override_base_url, resolve_model},
    extract_reasoning_trace, gpt5_codex_developer_prompt,
    shared::{
        StreamAssemblyError, StreamTelemetry, append_reasoning_segments, extract_data_payload,
        find_sse_boundary,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponsesApiState {
    Required,
    Allowed,
    Disabled,
}

#[derive(Default)]
struct OpenAIStreamTelemetry;

impl StreamTelemetry for OpenAIStreamTelemetry {
    fn on_content_delta(&self, _delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            length = delta.len(),
            "content delta received"
        );
    }

    fn on_reasoning_delta(&self, _delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            length = delta.len(),
            "reasoning delta received"
        );
    }

    fn on_tool_call_delta(&self) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            "tool call delta received"
        );
    }
}

fn is_responses_api_unsupported(status: StatusCode, body: &str) -> bool {
    matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) || body.contains("model does not exist")
        || body.contains("model not found")
        || body.contains("not enabled for the Responses API")
}

fn parse_responses_payload(
    response_json: Value,
    include_cached_prompt_metrics: bool,
) -> Result<LLMResponse, LLMError> {
    let output = response_json
        .get("output")
        .or_else(|| response_json.get("choices"))
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "Invalid response format: missing output",
            );
            LLMError::Provider(formatted_error)
        })?;

    if output.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No output in response");
        return Err(LLMError::Provider(formatted_error));
    }

    let mut content_fragments = Vec::new();
    let mut reasoning_fragments = Vec::new();
    let mut tool_calls_vec = Vec::new();

    for item in output {
        let item_type = item
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if item_type != "message" {
            continue;
        }

        if let Some(content_array) = item.get("content").and_then(|value| value.as_array()) {
            for entry in content_array {
                let entry_type = entry
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                match entry_type {
                    "output_text" | "text" => {
                        if let Some(text) = entry.get("text").and_then(|value| value.as_str()) {
                            if !text.is_empty() {
                                content_fragments.push(text.to_string());
                            }
                        }
                    }
                    "reasoning" => {
                        if let Some(text) = entry.get("text").and_then(|value| value.as_str()) {
                            if !text.is_empty() {
                                reasoning_fragments.push(text.to_string());
                            }
                        }
                    }
                    "tool_call" => {
                        let (name_value, arguments_value) = if let Some(function) =
                            entry.get("function").and_then(|value| value.as_object())
                        {
                            let name = function.get("name").and_then(|value| value.as_str());
                            let arguments = function.get("arguments");
                            (name, arguments)
                        } else {
                            let name = entry.get("name").and_then(|value| value.as_str());
                            let arguments = entry.get("arguments");
                            (name, arguments)
                        };

                        if let Some(name) = name_value {
                            let id = entry
                                .get("id")
                                .and_then(|value| value.as_str())
                                .unwrap_or_else(|| "");
                            let serialized = arguments_value.map_or("{}".to_string(), |value| {
                                if value.is_string() {
                                    value.as_str().unwrap_or("").to_string()
                                } else {
                                    value.to_string()
                                }
                            });
                            tool_calls_vec.push(ToolCall::function(
                                id.to_string(),
                                name.to_string(),
                                serialized,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let content = if content_fragments.is_empty() {
        None
    } else {
        Some(content_fragments.join(""))
    };

    let reasoning = if reasoning_fragments.is_empty() {
        None
    } else {
        Some(reasoning_fragments.join(""))
    };

    let tool_calls = if tool_calls_vec.is_empty() {
        None
    } else {
        Some(tool_calls_vec)
    };

    let usage = response_json.get("usage").map(|usage_value| {
        let cached_prompt_tokens = if include_cached_prompt_metrics {
            usage_value
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .or_else(|| usage_value.get("prompt_cache_hit_tokens"))
                .and_then(|value| value.as_u64())
                .map(|value| value as u32)
        } else {
            None
        };

        crate::llm::provider::Usage {
            prompt_tokens: usage_value
                .get("input_tokens")
                .or_else(|| usage_value.get("prompt_tokens"))
                .and_then(|pt| pt.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("output_tokens")
                .or_else(|| usage_value.get("completion_tokens"))
                .and_then(|ct| ct.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|tt| tt.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }
    });

    let stop_reason = response_json
        .get("stop_reason")
        .and_then(|value| value.as_str())
        .or_else(|| {
            output
                .iter()
                .find_map(|item| item.get("stop_reason").and_then(|value| value.as_str()))
        })
        .unwrap_or("stop");

    let finish_reason = match stop_reason {
        "stop" => FinishReason::Stop,
        "max_output_tokens" | "length" => FinishReason::Length,
        "tool_use" | "tool_calls" => FinishReason::ToolCalls,
        other => FinishReason::Error(other.to_string()),
    };

    Ok(LLMResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
        reasoning,
    })
}

struct OpenAIResponsesPayload {
    input: Vec<Value>,
    instructions: Option<String>,
}

pub struct OpenAIProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    responses_api_modes: Mutex<HashMap<String, ResponsesApiState>>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenAIPromptCacheSettings,
}

impl OpenAIProvider {
    fn serialize_tools(tools: &[ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let serialized_tools = tools
            .iter()
            .map(|tool| {
                if tool.tool_type == "function" {
                    let name = &tool.function.name;
                    let description = &tool.function.description;
                    let parameters = &tool.function.parameters;

                    json!({
                        "type": &tool.tool_type,
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                        "function": {
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                        }
                    })
                } else {
                    json!(tool)
                }
            })
            .collect::<Vec<Value>>();

        Some(Value::Array(serialized_tools))
    }

    fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openai::GPT_5_CODEX
    }

    fn is_responses_api_model(model: &str) -> bool {
        models::openai::RESPONSES_API_MODELS
            .iter()
            .any(|candidate| *candidate == model)
    }

    fn normalized_harmony_model(model: &str) -> String {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let without_provider = trimmed.rsplit('/').next().unwrap_or(trimmed);
        let without_annotation = without_provider
            .split('@')
            .next()
            .unwrap_or(without_provider);
        let without_variant = without_annotation
            .split(':')
            .next()
            .unwrap_or(without_annotation);

        without_variant.to_ascii_lowercase()
    }

    fn uses_harmony(model: &str) -> bool {
        let normalized = Self::normalized_harmony_model(model);
        if normalized.is_empty() {
            return false;
        }

        models::openai::HARMONY_MODELS
            .iter()
            .any(|candidate| *candidate == normalized)
    }

    fn convert_to_harmony_conversation(
        &self,
        request: &LLMRequest,
    ) -> Result<Conversation, LLMError> {
        let mut harmony_messages = Vec::new();
        let mut tool_call_authors: HashMap<String, String> = HashMap::new();

        // Add system message if present
        if let Some(system_prompt) = &request.system_prompt {
            harmony_messages.push(HarmonyMessage::from_role_and_content(
                HarmonyRole::System,
                system_prompt.clone(),
            ));
        }

        let mut developer_message = request.tools.as_ref().and_then(|tools| {
            let tool_descriptions: Vec<ToolDescription> = tools
                .iter()
                .filter_map(|tool| {
                    if tool.tool_type != "function" {
                        return None;
                    }

                    Some(ToolDescription::new(
                        tool.function.name.clone(),
                        tool.function.description.clone(),
                        Some(tool.function.parameters.clone()),
                    ))
                })
                .collect();

            if tool_descriptions.is_empty() {
                None
            } else {
                Some(HarmonyMessage::from_role_and_content(
                    HarmonyRole::Developer,
                    DeveloperContent::new().with_function_tools(tool_descriptions),
                ))
            }
        });
        let mut developer_inserted = developer_message.is_none();

        // Convert messages
        for msg in &request.messages {
            if !developer_inserted && msg.role != MessageRole::System {
                if let Some(dev_msg) = developer_message.take() {
                    harmony_messages.push(dev_msg);
                    developer_inserted = true;
                }
            }

            match msg.role {
                MessageRole::System => {
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::System,
                        msg.content.clone(),
                    ));
                }
                MessageRole::User => {
                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::User,
                        msg.content.clone(),
                    ));
                }
                MessageRole::Assistant => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            tool_call_authors.insert(
                                call.id.clone(),
                                format!("functions.{}", call.function.name),
                            );
                        }
                    }

                    harmony_messages.push(HarmonyMessage::from_role_and_content(
                        HarmonyRole::Assistant,
                        msg.content.clone(),
                    ));
                }
                MessageRole::Tool => {
                    let author_name = msg
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_call_authors.get(id))
                        .cloned()
                        .or_else(|| msg.tool_call_id.clone());

                    let author = author_name
                        .map(|name| HarmonyAuthor::new(HarmonyRole::Tool, name))
                        .unwrap_or_else(|| HarmonyAuthor::from(HarmonyRole::Tool));

                    harmony_messages.push(HarmonyMessage::from_author_and_content(
                        author,
                        msg.content.clone(),
                    ));
                }
            }
        }

        if let Some(dev_msg) = developer_message {
            harmony_messages.push(dev_msg);
        }

        Ok(Conversation::from_messages(harmony_messages))
    }

    fn requires_responses_api(model: &str) -> bool {
        model == models::openai::GPT_5 || model == models::openai::GPT_5_CODEX
    }

    fn default_responses_state(model: &str) -> ResponsesApiState {
        if Self::requires_responses_api(model) {
            ResponsesApiState::Required
        } else if Self::is_responses_api_model(model) {
            ResponsesApiState::Allowed
        } else {
            ResponsesApiState::Disabled
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openai::DEFAULT_MODEL.to_string(),
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
        let model_value = resolve_model(model, models::openai::DEFAULT_MODEL);

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
            |providers| &providers.openai,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let mut responses_api_modes = HashMap::new();
        responses_api_modes.insert(model.clone(), Self::default_responses_state(&model));

        Self {
            api_key,
            http_client: HttpClient::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| HttpClient::new()),
            base_url: override_base_url(urls::OPENAI_API_BASE, base_url),
            model,
            responses_api_modes: Mutex::new(responses_api_modes),
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn supports_temperature_parameter(model: &str) -> bool {
        // GPT-5 variants and GPT-5 Codex models don't support temperature parameter
        // All other OpenAI models generally support it
        !Self::is_gpt5_codex_model(model)
            && model != models::openai::GPT_5
            && model != models::openai::GPT_5_MINI
            && model != models::openai::GPT_5_NANO
    }

    fn responses_api_state(&self, model: &str) -> ResponsesApiState {
        let mut modes = self
            .responses_api_modes
            .lock()
            .expect("OpenAI responses_api_modes mutex poisoned");
        *modes
            .entry(model.to_string())
            .or_insert_with(|| Self::default_responses_state(model))
    }

    fn set_responses_api_state(&self, model: &str, state: ResponsesApiState) {
        let mut modes = self
            .responses_api_modes
            .lock()
            .expect("OpenAI responses_api_modes mutex poisoned");
        modes.insert(model.to_string(), state);
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
        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let max_tokens = value
            .get(MAX_COMPLETION_TOKENS_FIELD)
            .or_else(|| value.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
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

    fn convert_to_openai_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut messages = Vec::new();
        let mut active_tool_call_ids: HashSet<String> = HashSet::new();

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

            if msg.role == MessageRole::Assistant {
                if let Some(tool_calls) = &msg.tool_calls {
                    if !tool_calls.is_empty() {
                        let tool_calls_json: Vec<Value> = tool_calls
                            .iter()
                            .map(|tc| {
                                active_tool_call_ids.insert(tc.id.clone());
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
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut openai_request = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            openai_request[MAX_COMPLETION_TOKENS_FIELD] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            if Self::supports_temperature_parameter(&request.model) {
                openai_request["temperature"] = json!(temperature);
            }
        }

        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools {
                if let Some(serialized) = Self::serialize_tools(tools) {
                    openai_request["tools"] = serialized;
                }
            }

            if let Some(tool_choice) = &request.tool_choice {
                openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
            }

            if let Some(parallel) = request.parallel_tool_calls {
                openai_request["parallel_tool_calls"] = Value::Bool(parallel);
            }

            if self.supports_parallel_tool_config(&request.model) {
                if let Some(config) = &request.parallel_tool_config {
                    if let Ok(config_value) = serde_json::to_value(config) {
                        openai_request["parallel_tool_config"] = config_value;
                    }
                }
            }
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenAI, effort) {
                    openai_request["reasoning"] = payload;
                } else {
                    openai_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        Ok(openai_request)
    }

    fn convert_to_openai_responses_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let responses_payload = if Self::is_gpt5_codex_model(&request.model) {
            build_codex_responses_payload(request)?
        } else {
            build_standard_responses_payload(request)?
        };

        if responses_payload.input.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut openai_request = json!({
            "model": request.model,
            "input": responses_payload.input,
            "stream": request.stream
        });

        if let Some(instructions) = responses_payload.instructions {
            if !instructions.trim().is_empty() {
                openai_request["instructions"] = json!(instructions);
            }
        }

        if let Some(max_tokens) = request.max_tokens {
            openai_request["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            if Self::supports_temperature_parameter(&request.model) {
                openai_request["temperature"] = json!(temperature);
            }
        }

        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools {
                if let Some(serialized) = Self::serialize_tools(tools) {
                    openai_request["tools"] = serialized;
                }
            }

            if let Some(tool_choice) = &request.tool_choice {
                openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
            }

            if let Some(parallel) = request.parallel_tool_calls {
                openai_request["parallel_tool_calls"] = Value::Bool(parallel);
            }

            if self.supports_parallel_tool_config(&request.model) {
                if let Some(config) = &request.parallel_tool_config {
                    if let Ok(config_value) = serde_json::to_value(config) {
                        openai_request["parallel_tool_config"] = config_value;
                    }
                }
            }
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenAI, effort) {
                    openai_request["reasoning"] = payload;
                } else {
                    openai_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        if self.supports_reasoning_effort(&request.model)
            && openai_request.get("reasoning").is_none()
        {
            openai_request["reasoning"] =
                json!({ "effort": ReasoningEffortLevel::default().as_str() });
        }

        Ok(openai_request)
    }

    fn parse_openai_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let choices = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    "Invalid response format: missing choices",
                );
                LLMError::Provider(formatted_error)
            })?;

        if choices.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No choices in response");
            return Err(LLMError::Provider(formatted_error));
        }

        let choice = &choices[0];
        let message = choice.get("message").ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
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
            .map(|fr| match fr {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolCalls,
                "content_filter" => FinishReason::ContentFilter,
                other => FinishReason::Error(other.to_string()),
            })
            .unwrap_or(FinishReason::Stop);

        Ok(LLMResponse {
            content,
            tool_calls,
            usage: response_json.get("usage").map(|usage_value| {
                let cached_prompt_tokens =
                    if self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics {
                        usage_value
                            .get("prompt_tokens_details")
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(|value| value.as_u64())
                            .map(|value| value as u32)
                    } else {
                        None
                    };

                crate::llm::provider::Usage {
                    prompt_tokens: usage_value
                        .get("prompt_tokens")
                        .and_then(|pt| pt.as_u64())
                        .unwrap_or(0) as u32,
                    completion_tokens: usage_value
                        .get("completion_tokens")
                        .and_then(|ct| ct.as_u64())
                        .unwrap_or(0) as u32,
                    total_tokens: usage_value
                        .get("total_tokens")
                        .and_then(|tt| tt.as_u64())
                        .unwrap_or(0) as u32,
                    cached_prompt_tokens,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                }
            }),
            finish_reason,
            reasoning,
        })
    }

    fn parse_openai_responses_response(
        &self,
        response_json: Value,
    ) -> Result<LLMResponse, LLMError> {
        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        parse_responses_payload(response_json, include_metrics)
    }

    async fn generate_with_harmony(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        // Load harmony encoding
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss).map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to load harmony encoding: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        // Convert to harmony conversation
        let conversation = self.convert_to_harmony_conversation(&request)?;

        // Render conversation for completion
        let prompt_tokens = encoding
            .render_conversation_for_completion(&conversation, HarmonyRole::Assistant, None)
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to render conversation: {}", e),
                );
                LLMError::Provider(formatted_error)
            })?;

        // Send tokens to inference server
        let completion_tokens = self
            .send_harmony_tokens_to_inference_server(
                &prompt_tokens,
                request.max_tokens,
                request.temperature,
            )
            .await?;

        // Parse completion tokens back into messages
        let parsed_messages = encoding
            .parse_messages_from_completion_tokens(
                completion_tokens.clone(),
                Some(HarmonyRole::Assistant),
            )
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Failed to parse completion tokens: {}", e),
                );
                LLMError::Provider(formatted_error)
            })?;

        // Extract content from parsed messages
        let mut content = None;
        let mut tool_calls = Vec::new();

        let extract_text_content = |parts: &[HarmonyContent]| -> Option<String> {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    HarmonyContent::Text(text_part) => Some(text_part.text.clone()),
                    _ => None,
                })
                .collect::<String>();

            if text.is_empty() { None } else { Some(text) }
        };

        let normalize_json_arguments = |raw: String| -> String {
            match serde_json::from_str::<Value>(&raw) {
                Ok(parsed) => parsed.to_string(),
                Err(_) => raw,
            }
        };

        for message in parsed_messages {
            match message.author.role {
                HarmonyRole::Assistant => {
                    if let Some(channel) = &message.channel {
                        match channel.as_str() {
                            "final" => {
                                // This is the final response content
                                // Extract text from content Vec<Content>
                                if let Some(text_content) = extract_text_content(&message.content) {
                                    content = Some(text_content);
                                }
                            }
                            "commentary" => {
                                // Check if this is a tool call
                                if let Some(recipient) = &message.recipient {
                                    if recipient.starts_with("functions.") {
                                        // This is a tool call with functions. prefix
                                        let function_name = recipient
                                            .strip_prefix("functions.")
                                            .unwrap_or(recipient);
                                        let arguments = extract_text_content(&message.content)
                                            .map(normalize_json_arguments)
                                            .unwrap_or_else(|| "{}".to_string());

                                        tool_calls.push(ToolCall::function(
                                            format!("call_{}", tool_calls.len()),
                                            function_name.to_string(),
                                            arguments,
                                        ));
                                    } else {
                                        // Check if this is a harmony format tool call (to=tool_name)
                                        // The recipient might be the tool name directly
                                        let tool_name = Self::parse_harmony_tool_name(recipient);
                                        if !tool_name.is_empty() {
                                            let arguments = extract_text_content(&message.content)
                                                .map(normalize_json_arguments)
                                                .unwrap_or_else(|| "{}".to_string());

                                            tool_calls.push(ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                } else {
                                    // Check if the content itself contains harmony tool call format
                                    if let Some(text_content) = extract_text_content(&message.content) {
                                        if let Some((tool_name, args)) = Self::parse_harmony_tool_call_from_text(&text_content) {
                                            let arguments = serde_json::to_string(&args)
                                                .unwrap_or_else(|_| "{}".to_string());

                                            tool_calls.push(ToolCall::function(
                                                format!("call_{}", tool_calls.len()),
                                                tool_name,
                                                arguments,
                                            ));
                                        }
                                    }
                                }
                            }
                            _ => {} // Other channels like "analysis" are for reasoning
                        }
                    }
                }
                _ => {} // Skip other message types for now
            }
        }

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        Ok(LLMResponse {
            content,
            tool_calls,
            usage: Some(crate::llm::provider::Usage {
                prompt_tokens: prompt_tokens.len() as u32,
                completion_tokens: completion_tokens.len() as u32,
                total_tokens: (prompt_tokens.len() + completion_tokens.len()) as u32,
                cached_prompt_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            }),
            finish_reason: FinishReason::Stop,
            reasoning: None,
        })
    }

}

impl OpenAIProvider {
    /// Sends harmony-formatted tokens to an inference server for GPT-OSS models.
    ///
    /// This method handles the HTTP communication with inference servers that support
    /// harmony-formatted token inputs (such as vLLM or Transformers serve).
    ///
    /// # Configuration
    ///
    /// Set the `HARMONY_INFERENCE_SERVER_URL` environment variable to configure
    /// the inference server endpoint. Defaults to `http://localhost:8000` for local vLLM.
    ///
    /// # Supported Servers
    ///
    /// - **vLLM**: Set `HARMONY_INFERENCE_SERVER_URL=http://localhost:8000`
    /// - **Transformers serve**: Configure appropriate endpoint URL
    /// - **Custom servers**: Any server accepting `{"prompt_token_ids": [...], "max_tokens": N, ...}`
    ///
    /// # Example
    ///
    /// ```bash
    /// export HARMONY_INFERENCE_SERVER_URL=http://localhost:8000
    /// vtcode ask --model openai/gpt-oss-20b "Explain quantum computing"
    /// ```
    async fn send_harmony_tokens_to_inference_server(
        &self,
        tokens: &[u32],
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<Vec<u32>, LLMError> {
        // Get harmony inference server URL from environment variable
        // Default to localhost vLLM server if not configured
        let server_url = std::env::var("HARMONY_INFERENCE_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:8000".to_string());

        // Load harmony encoding to get stop tokens
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss).map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to load harmony encoding for stop tokens: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        let stop_token_ids = encoding.stop_tokens_for_assistant_actions().map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to get stop tokens: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        // Convert HashSet to Vec for JSON serialization
        let stop_token_ids_vec: Vec<u32> = stop_token_ids.into_iter().collect();

        // Prepare request body for vLLM-style inference server
        let request_body = json!({
            "prompt_token_ids": tokens,
            "max_tokens": max_tokens.unwrap_or(1024),
            "temperature": temperature.unwrap_or(0.7),
            "stop_token_ids": stop_token_ids_vec,
            // Additional parameters that might be needed
            "stream": false,
            "logprobs": null,
            "echo": false
        });

        // Send HTTP request to inference server
        let response = self
            .http_client
            .post(&format!("{}/generate", server_url))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!(
                        "Failed to send request to harmony inference server at {}: {}",
                        server_url, e
                    ),
                );
                LLMError::Network(formatted_error)
            })?;

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!(
                    "Harmony inference server error ({}): {}",
                    status, error_text
                ),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        // Parse response JSON
        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse harmony inference response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        // Extract completion tokens from response
        // vLLM returns tokens in different formats depending on the response structure
        let completion_tokens =
            if let Some(tokens_array) = response_json.get("tokens").and_then(|t| t.as_array()) {
                // Direct tokens array
                tokens_array
                    .iter()
                    .filter_map(|v| v.as_u64().map(|u| u as u32))
                    .collect::<Vec<u32>>()
            } else if let Some(outputs) = response_json.get("outputs").and_then(|o| o.as_array()) {
                // vLLM nested outputs format
                outputs
                    .first()
                    .and_then(|output| output.get("token_ids"))
                    .and_then(|token_ids| token_ids.as_array())
                    .map(|token_ids| {
                        token_ids
                            .iter()
                            .filter_map(|v| v.as_u64().map(|u| u as u32))
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            } else {
                // Fallback: try to find tokens in any nested structure
                let mut found_tokens = Vec::new();
                if let Some(obj) = response_json.as_object() {
                    for (_, value) in obj {
                        if let Some(arr) = value.as_array() {
                            if arr.iter().all(|v| v.is_u64()) {
                                found_tokens = arr
                                    .iter()
                                    .filter_map(|v| v.as_u64().map(|u| u as u32))
                                    .collect();
                                break;
                            }
                        }
                    }
                }
                found_tokens
            };

        if completion_tokens.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "No completion tokens received from harmony inference server",
            );
            return Err(LLMError::Provider(formatted_error));
        }

        Ok(completion_tokens)
    }

    /// Parse harmony tool name from recipient or tool reference
    fn parse_harmony_tool_name(recipient: &str) -> String {
        // Handle formats like "repo_browser.list_files" or "container.exec"
        match recipient {
            "repo_browser.list_files" | "list_files" => "list_files".to_string(),
            "repo_browser.read_file" | "read_file" => "read_file".to_string(),
            "repo_browser.write_file" | "write_file" => "write_file".to_string(),
            "container.exec" | "exec" => "run_terminal_cmd".to_string(),
            "bash" => "bash".to_string(),
            "curl" => "curl".to_string(),
            "grep" => "grep_file".to_string(),
            _ => {
                // Try to extract the function name after the last dot
                if let Some(dot_pos) = recipient.rfind('.') {
                    recipient[dot_pos + 1..].to_string()
                } else {
                    recipient.to_string()
                }
            }
        }
    }

    /// Parse harmony tool call from raw text content
    fn parse_harmony_tool_call_from_text(text: &str) -> Option<(String, serde_json::Value)> {
        // Look for harmony format: to=tool_name followed by JSON
        if let Some(to_pos) = text.find("to=") {
            let after_to = &text[to_pos + 3..];
            if let Some(space_pos) = after_to.find(' ') {
                let tool_ref = &after_to[..space_pos];
                let tool_name = Self::parse_harmony_tool_name(tool_ref);
                
                // Look for JSON in the remaining text
                let remaining = &after_to[space_pos..];
                if let Some(json_start) = remaining.find('{') {
                    if let Some(json_end) = remaining.rfind('}') {
                        let json_text = &remaining[json_start..=json_end];
                        if let Ok(args) = serde_json::from_str(json_text) {
                            return Some((tool_name, args));
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ParallelToolConfig;

    fn sample_tool() -> ToolDefinition {
        ToolDefinition::function(
            "search_workspace".to_string(),
            "Search project files".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        )
    }

    fn sample_request(model: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            system_prompt: None,
            tools: Some(vec![sample_tool()]),
            model: model.to_string(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn serialize_tools_wraps_function_definition() {
        let tools = vec![sample_tool()];
        let serialized = OpenAIProvider::serialize_tools(&tools).expect("tools should serialize");
        let serialized_tools = serialized
            .as_array()
            .expect("serialized tools should be an array");
        assert_eq!(serialized_tools.len(), 1);

        let tool_value = serialized_tools[0]
            .as_object()
            .expect("tool should be serialized as object");
        assert_eq!(
            tool_value.get("type").and_then(Value::as_str),
            Some("function")
        );
        assert!(tool_value.contains_key("function"));
        assert_eq!(
            tool_value.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
        assert_eq!(
            tool_value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "Search project files"
        );

        let function_value = tool_value
            .get("function")
            .and_then(Value::as_object)
            .expect("function payload missing");
        assert_eq!(
            function_value.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
        assert!(function_value.contains_key("parameters"));
        assert_eq!(
            tool_value.get("parameters").and_then(Value::as_object),
            function_value.get("parameters").and_then(Value::as_object)
        );
    }

    #[test]
    fn chat_completions_payload_uses_function_wrapper() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
        let request = sample_request(models::openai::DEFAULT_MODEL);
        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools should exist on payload");
        let tool_object = tools[0].as_object().expect("tool entry should be object");
        assert!(tool_object.contains_key("function"));
        assert_eq!(
            tool_object.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
    }

    #[test]
    fn responses_payload_uses_function_wrapper() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::GPT_5_CODEX.to_string());
        let request = sample_request(models::openai::GPT_5_CODEX);
        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        let instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .expect("instructions should be set for codex");
        assert!(instructions.contains("You are Codex, based on GPT-5"));

        let tools = payload
            .get("tools")
            .and_then(Value::as_array)
            .expect("tools should exist on payload");
        let tool_object = tools[0].as_object().expect("tool entry should be object");
        assert!(tool_object.contains_key("function"));
        assert_eq!(
            tool_object.get("name").and_then(Value::as_str),
            Some("search_workspace")
        );
    }

    #[test]
    fn responses_payload_sets_instructions_from_system_prompt() {
        let provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_5.to_string());
        let mut request = sample_request(models::openai::GPT_5);
        request.system_prompt = Some("You are a helpful assistant.".to_string());

        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        let instructions = payload
            .get("instructions")
            .and_then(Value::as_str)
            .expect("instructions should be present");
        assert!(instructions.contains("You are a helpful assistant."));

        let input = payload
            .get("input")
            .and_then(Value::as_array)
            .expect("input should be serialized as array");
        assert_eq!(
            input
                .first()
                .and_then(|value| value.get("role"))
                .and_then(Value::as_str),
            Some("user")
        );
    }

    #[test]
    fn harmony_detection_handles_common_variants() {
        assert!(OpenAIProvider::uses_harmony("gpt-oss-20b"));
        assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b"));
        assert!(OpenAIProvider::uses_harmony("openai/gpt-oss-20b:free"));
        assert!(OpenAIProvider::uses_harmony("OPENAI/GPT-OSS-120B"));
        assert!(OpenAIProvider::uses_harmony("gpt-oss-120b@openrouter"));

        assert!(!OpenAIProvider::uses_harmony("gpt-5"));
        assert!(!OpenAIProvider::uses_harmony("gpt-oss:20b"));
    }

    #[test]
    fn test_parse_harmony_tool_name() {
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("repo_browser.list_files"), "list_files");
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("container.exec"), "run_terminal_cmd");
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("bash"), "bash");
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("unknown.tool"), "tool");
        assert_eq!(OpenAIProvider::parse_harmony_tool_name("simple"), "simple");
    }

    #[test]
    fn test_parse_harmony_tool_call_from_text() {
        let text = r#"to=repo_browser.list_files {"path":"", "recursive":"true"}"#;
        let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
        assert!(result.is_some());
        
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "list_files");
        assert_eq!(args["path"], serde_json::json!(""));
        assert_eq!(args["recursive"], serde_json::json!("true"));
    }

    #[test]
    fn test_parse_harmony_tool_call_from_text_container_exec() {
        let text = r#"to=container.exec {"cmd":["ls", "-la"]}"#;
        let result = OpenAIProvider::parse_harmony_tool_call_from_text(text);
        assert!(result.is_some());
        
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_terminal_cmd");
        assert_eq!(args["cmd"], serde_json::json!(["ls", "-la"]));
    }

    #[test]
    fn chat_completions_uses_max_completion_tokens_field() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::DEFAULT_MODEL.to_string());
        let mut request = sample_request(models::openai::DEFAULT_MODEL);
        request.max_tokens = Some(512);

        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        let max_tokens_value = payload
            .get(MAX_COMPLETION_TOKENS_FIELD)
            .and_then(Value::as_u64)
            .expect("max completion tokens should be set");
        assert_eq!(max_tokens_value, 512);
        assert!(payload.get("max_tokens").is_none());
    }

    #[test]
    fn chat_completions_applies_temperature_independent_of_max_tokens() {
        let provider = OpenAIProvider::with_model(
            String::new(),
            models::openai::CODEX_MINI_LATEST.to_string(),
        );
        let mut request = sample_request(models::openai::CODEX_MINI_LATEST);
        request.temperature = Some(0.4);

        let payload = provider
            .convert_to_openai_format(&request)
            .expect("conversion should succeed");

        assert!(payload.get(MAX_COMPLETION_TOKENS_FIELD).is_none());
        let temperature_value = payload
            .get("temperature")
            .and_then(Value::as_f64)
            .expect("temperature should be present");
        assert!((temperature_value - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn responses_payload_omits_parallel_tool_config_when_not_supported() {
        let provider =
            OpenAIProvider::with_model(String::new(), models::openai::GPT_5_CODEX.to_string());
        let mut request = sample_request(models::openai::GPT_5_CODEX);
        request.parallel_tool_calls = Some(true);
        request.parallel_tool_config = Some(ParallelToolConfig {
            disable_parallel_tool_use: true,
            max_parallel_tools: Some(2),
            encourage_parallel: false,
        });

        let payload = provider
            .convert_to_openai_responses_format(&request)
            .expect("conversion should succeed");

        assert_eq!(payload.get("parallel_tool_calls"), Some(&Value::Bool(true)));
        assert!(
            payload.get("parallel_tool_config").is_none(),
            "OpenAI payload should not include parallel_tool_config"
        );
    }
}

fn build_standard_responses_payload(
    request: &LLMRequest,
) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();
    let mut instructions_segments = Vec::new();

    if let Some(system_prompt) = &request.system_prompt {
        let trimmed = system_prompt.trim();
        if !trimmed.is_empty() {
            instructions_segments.push(trimmed.to_string());
        }
    }

    for msg in &request.messages {
        match msg.role {
            MessageRole::System => {
                let trimmed = msg.content.trim();
                if !trimmed.is_empty() {
                    instructions_segments.push(trimmed.to_string());
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
                        active_tool_call_ids.insert(call.id.clone());
                        content_parts.push(json!({
                            "type": "tool_call",
                            "id": call.id.clone(),
                            "function": {
                                "name": call.function.name.clone(),
                                "arguments": call.function.arguments.clone()
                            }
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
                        "OpenAI",
                        "Tool messages must include tool_call_id for Responses API",
                    );
                    LLMError::InvalidRequest(formatted_error)
                })?;

                if !active_tool_call_ids.contains(&tool_call_id) {
                    continue;
                }

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

                active_tool_call_ids.remove(&tool_call_id);

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

    let instructions = if instructions_segments.is_empty() {
        None
    } else {
        Some(instructions_segments.join("\n\n"))
    };

    Ok(OpenAIResponsesPayload {
        input,
        instructions,
    })
}

fn build_codex_responses_payload(request: &LLMRequest) -> Result<OpenAIResponsesPayload, LLMError> {
    let mut additional_guidance = Vec::new();

    if let Some(system_prompt) = &request.system_prompt {
        let trimmed = system_prompt.trim();
        if !trimmed.is_empty() {
            additional_guidance.push(trimmed.to_string());
        }
    }

    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();

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
                        active_tool_call_ids.insert(call.id.clone());
                        content_parts.push(json!({
                            "type": "tool_call",
                            "id": call.id.clone(),
                            "function": {
                                "name": call.function.name.clone(),
                                "arguments": call.function.arguments.clone()
                            }
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
                        "OpenAI",
                        "Tool messages must include tool_call_id for Responses API",
                    );
                    LLMError::InvalidRequest(formatted_error)
                })?;

                if !active_tool_call_ids.contains(&tool_call_id) {
                    continue;
                }

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

                active_tool_call_ids.remove(&tool_call_id);

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

    let instructions = gpt5_codex_developer_prompt(&additional_guidance);

    Ok(OpenAIResponsesPayload {
        input,
        instructions: Some(instructions),
    })
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        !matches!(
            self.responses_api_state(&self.model),
            ResponsesApiState::Disabled
        )
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };
        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_tools(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        !models::openai::TOOL_UNAVAILABLE_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        let responses_state = self.responses_api_state(&request.model);
        let prefer_responses_stream = matches!(responses_state, ResponsesApiState::Required)
            || (matches!(responses_state, ResponsesApiState::Allowed)
                && request.tools.as_ref().map_or(true, Vec::is_empty));

        if !prefer_responses_stream {
            request.stream = false;
            let response = self.generate(request).await?;
            let stream = try_stream! {
                yield LLMStreamEvent::Completed { response };
            };
            return Ok(Box::pin(stream));
        }

        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;

        let mut openai_request = self.convert_to_openai_responses_format(&request)?;
        openai_request["stream"] = Value::Bool(true);
        #[cfg(debug_assertions)]
        let debug_model = request.model.clone();
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                stream = true,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching streaming Responses request"
            );
        }

        let url = format!("{}/responses", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("OpenAI-Beta", "responses=v1")
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if matches!(responses_state, ResponsesApiState::Allowed)
                && is_responses_api_unsupported(status, &error_text)
            {
                #[cfg(debug_assertions)]
                debug!(
                    target = "vtcode::llm::openai",
                    model = %request.model,
                    "Responses API unsupported; falling back to Chat Completions for streaming"
                );
                self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                request.stream = false;
                let response = self.generate(request).await?;
                let stream = try_stream! {
                    yield LLMStreamEvent::Completed { response };
                };
                return Ok(Box::pin(stream));
            }

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        #[cfg(debug_assertions)]
        {
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                status = %response.status(),
                handshake_ms = request_timer.elapsed().as_millis(),
                "Streaming response headers received"
            );
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregated_content = String::new();
            let mut reasoning_buffer = ReasoningBuffer::default();
            let mut final_response: Option<Value> = None;
            let mut done = false;
            #[cfg(debug_assertions)]
            let mut streamed_events_counter: usize = 0;
            let telemetry = OpenAIStreamTelemetry::default();

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|err| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Streaming error: {}", err),
                    );
                    LLMError::Network(formatted_error)
                })?;

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);
                    #[cfg(debug_assertions)]
                    {
                        streamed_events_counter = streamed_events_counter.saturating_add(1);
                    }

                    if let Some(data_payload) = extract_data_payload(&event) {
                        let trimmed_payload = data_payload.trim();
                        if trimmed_payload.is_empty() {
                            continue;
                        }

                        if trimmed_payload == "[DONE]" {
                            done = true;
                            break;
                        }

                        let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                            StreamAssemblyError::InvalidPayload(err.to_string())
                                .into_llm_error("OpenAI")
                        })?;

                        if let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) {
                            match event_type {
                                "response.output_text.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    aggregated_content.push_str(delta);
                                    telemetry.on_content_delta(delta);
                                    yield LLMStreamEvent::Token { delta: delta.to_string() };
                                }
                                "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                                    let delta = payload
                                        .get("delta")
                                        .and_then(|value| value.as_str())
                                        .ok_or_else(|| {
                                            StreamAssemblyError::MissingField("delta")
                                                .into_llm_error("OpenAI")
                                        })?;
                                    for fragment in append_reasoning_segments(&mut reasoning_buffer, delta, &telemetry) {
                                        yield LLMStreamEvent::Reasoning { delta: fragment };
                                    }
                                }
                                "response.completed" => {
                                    if let Some(response_value) = payload.get("response") {
                                        final_response = Some(response_value.clone());
                                    }
                                    done = true;
                                }
                                "response.failed" | "response.incomplete" => {
                                    let message = payload
                                        .get("response")
                                        .and_then(|value| value.get("error"))
                                        .and_then(|error| error.get("message"))
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("Streaming response failed");
                                    let formatted_error = error_display::format_llm_error("OpenAI", message);
                                    Err(LLMError::Provider(formatted_error))?;
                                }
                                "error" => {
                                    let message = payload
                                        .get("message")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or("Streaming request failed");
                                    let formatted_error = error_display::format_llm_error("OpenAI", message);
                                    Err(LLMError::Provider(formatted_error))?;
                                }
                                _ => {}
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
                                .into_llm_error("OpenAI")
                        })?;

                        if payload
                            .get("type")
                            .and_then(|value| value.as_str())
                            .map(|event_type| event_type == "response.completed")
                            .unwrap_or(false)
                        {
                            if let Some(response_value) = payload.get("response") {
                                final_response = Some(response_value.clone());
                            }
                        }
                    }
                }
            }

            let response_value = match final_response {
                Some(value) => value,
                None => {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        "Stream ended without a completion event",
                    );
                    Err(LLMError::Provider(formatted_error))?
                }
            };

            let mut response = parse_responses_payload(response_value, include_metrics)?;

            if response.content.is_none() && !aggregated_content.is_empty() {
                response.content = Some(aggregated_content.clone());
            }

            if let Some(reasoning_text) = reasoning_buffer.finalize() {
                response.reasoning = Some(reasoning_text);
            }

            #[cfg(debug_assertions)]
            {
                debug!(
                    target = "vtcode::llm::openai",
                    model = %debug_model,
                    elapsed_ms = request_timer.elapsed().as_millis(),
                    events = streamed_events_counter,
                    content_len = aggregated_content.len(),
                    "Completed streaming response"
                );
            }

            yield LLMStreamEvent::Completed { response };
        };

        Ok(Box::pin(stream))
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut request = request;
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        if !self.supports_parallel_tool_config(&request.model) {
            request.parallel_tool_config = None;
        }

        // Check if this is a harmony model (GPT-OSS)
        if Self::uses_harmony(&request.model) {
            return self.generate_with_harmony(request).await;
        }

        let responses_state = self.responses_api_state(&request.model);
        let attempt_responses = !matches!(responses_state, ResponsesApiState::Disabled)
            && (matches!(responses_state, ResponsesApiState::Required)
                || request.tools.as_ref().map_or(true, Vec::is_empty));
        #[cfg(debug_assertions)]
        let request_timer = Instant::now();
        #[cfg(debug_assertions)]
        {
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = attempt_responses,
                messages = request.messages.len(),
                tools = tool_count,
                "Dispatching non-streaming OpenAI request"
            );
        }

        if attempt_responses {
            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);

            let response = self
                .http_client
                .post(&url)
                .bearer_auth(&self.api_key)
                .header("OpenAI-Beta", "responses=v1")
                .json(&openai_request)
                .send()
                .await
                .map_err(|e| {
                    let formatted_error =
                        error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                    LLMError::Network(formatted_error)
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();

                if matches!(responses_state, ResponsesApiState::Allowed)
                    && is_responses_api_unsupported(status, &error_text)
                {
                    #[cfg(debug_assertions)]
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %request.model,
                        "Responses API unsupported; disabling for future requests"
                    );
                    self.set_responses_api_state(&request.model, ResponsesApiState::Disabled);
                } else if status.as_u16() == 429
                    || error_text.contains("insufficient_quota")
                    || error_text.contains("quota")
                    || error_text.contains("rate limit")
                {
                    return Err(LLMError::RateLimit);
                } else {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("HTTP {}: {}", status, error_text),
                    );
                    return Err(LLMError::Provider(formatted_error));
                }
            } else {
                let openai_response: Value = response.json().await.map_err(|e| {
                    let formatted_error = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Failed to parse response: {}", e),
                    );
                    LLMError::Provider(formatted_error)
                })?;

                let response = self.parse_openai_responses_response(openai_response)?;
                #[cfg(debug_assertions)]
                {
                    let content_len = response.content.as_ref().map_or(0, |c| c.len());
                    debug!(
                        target = "vtcode::llm::openai",
                        model = %request.model,
                        responses_api = true,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        content_len = content_len,
                        finish_reason = ?response.finish_reason,
                        "Completed non-streaming OpenAI request"
                    );
                }
                return Ok(response);
            }
        } else {
            #[cfg(debug_assertions)]
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                "Skipping Responses API (disabled); using Chat Completions"
            );
        }

        let openai_request = self.convert_to_openai_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenAI", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let openai_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        let response = self.parse_openai_response(openai_response)?;
        #[cfg(debug_assertions)]
        {
            let content_len = response.content.as_ref().map_or(0, |c| c.len());
            debug!(
                target = "vtcode::llm::openai",
                model = %request.model,
                responses_api = false,
                elapsed_ms = request_timer.elapsed().as_millis(),
                content_len = content_len,
                finish_reason = ?response.finish_reason,
                "Completed non-streaming OpenAI request"
            );
        }
        Ok(response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::openai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        if !self.supported_models().contains(&request.model) {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("OpenAI", &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for OpenAIProvider {
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
        llm_types::BackendKind::OpenAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
