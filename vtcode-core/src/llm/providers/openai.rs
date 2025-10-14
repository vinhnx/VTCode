use crate::config::constants::{models, urls};
use crate::config::core::{OpenAIPromptCacheSettings, PromptCachingConfig};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole, ToolCall,
    ToolChoice, ToolDefinition,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_COMPLETION_TOKENS_FIELD: &str = "max_completion_tokens";

use super::{extract_reasoning_trace, gpt5_codex_developer_prompt};

pub struct OpenAIProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
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

    fn is_reasoning_model(model: &str) -> bool {
        models::openai::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == model)
    }

    fn uses_responses_api(model: &str) -> bool {
        Self::is_gpt5_codex_model(model) || Self::is_reasoning_model(model)
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::openai::DEFAULT_MODEL.to_string(), None)
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let mut provider = if let Some(model_value) = model {
            Self::with_model_internal(api_key_value, model_value, prompt_cache)
        } else {
            Self::with_model_internal(
                api_key_value,
                models::openai::DEFAULT_MODEL.to_string(),
                prompt_cache,
            )
        };
        if let Some(base) = base_url {
            provider.base_url = base;
        }
        provider
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) =
            Self::extract_prompt_cache_settings(prompt_cache);

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: urls::OPENAI_API_BASE.to_string(),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn extract_prompt_cache_settings(
        prompt_cache: Option<PromptCachingConfig>,
    ) -> (bool, OpenAIPromptCacheSettings) {
        if let Some(cfg) = prompt_cache {
            let provider_settings = cfg.providers.openai;
            let enabled = cfg.enabled && provider_settings.enabled;
            (enabled, provider_settings)
        } else {
            (false, OpenAIPromptCacheSettings::default())
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
            .and_then(ReasoningEffortLevel::from_str)
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|r| r.get("effort"))
                    .and_then(|effort| effort.as_str())
                    .and_then(ReasoningEffortLevel::from_str)
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
            if request.temperature.is_some() && Self::supports_temperature_parameter(&request.model)
            {
                if let Some(temperature) = request.temperature {
                    openai_request["temperature"] = json!(temperature);
                }
            }
            openai_request[MAX_COMPLETION_TOKENS_FIELD] = json!(max_tokens);
        }

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
        let input = if Self::is_gpt5_codex_model(&request.model) {
            build_codex_responses_input_openai(request)?
        } else {
            build_standard_responses_input_openai(request)?
        };

        if input.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No messages provided for Responses API");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut openai_request = json!({
            "model": request.model,
            "input": input,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            if request.temperature.is_some() && Self::supports_temperature_parameter(&request.model)
            {
                if let Some(temperature) = request.temperature {
                    openai_request["temperature"] = json!(temperature);
                }
            }
            openai_request["max_output_tokens"] = json!(max_tokens);
        }

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

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                if let Some(payload) = reasoning_parameters_for(Provider::OpenAI, effort) {
                    openai_request["reasoning"] = payload;
                } else {
                    openai_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        if Self::is_reasoning_model(&request.model) {
            openai_request["reasoning"] = json!({ "effort": "medium" });
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
            let formatted_error =
                error_display::format_llm_error("OpenAI", "No output in response");
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
                                let serialized =
                                    arguments_value.map_or("{}".to_string(), |value| {
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
            let cached_prompt_tokens =
                if self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

fn build_standard_responses_input_openai(request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
    let mut input = Vec::new();
    let mut active_tool_call_ids: HashSet<String> = HashSet::new();

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

    Ok(input)
}

fn build_codex_responses_input_openai(request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
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

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        false
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

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut request = request;
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        if Self::uses_responses_api(&request.model) {
            let openai_request = self.convert_to_openai_responses_format(&request)?;
            let url = format!("{}/responses", self.base_url);

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

            self.parse_openai_responses_response(openai_response)
        } else {
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

            self.parse_openai_response(openai_response)
        }
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
