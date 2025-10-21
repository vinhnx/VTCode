use crate::config::constants::{defaults, models, urls};
use crate::config::core::{AnthropicPromptCacheSettings, PromptCachingConfig};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
    ParallelToolConfig, ToolCall, ToolChoice, ToolDefinition,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};

use super::{
    common::{extract_prompt_cache_settings, override_base_url, resolve_model},
    extract_reasoning_trace,
};

pub struct AnthropicProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: AnthropicPromptCacheSettings,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::anthropic::DEFAULT_MODEL.to_string(),
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
        let model_value = resolve_model(model, models::anthropic::DEFAULT_MODEL);

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
            |providers| &providers.anthropic,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: override_base_url(urls::ANTHROPIC_API_BASE, base_url),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn cache_control_value(&self) -> Option<Value> {
        if !self.prompt_cache_enabled {
            return None;
        }

        if let Some(ttl) = self.prompt_cache_settings.extended_ttl_seconds {
            Some(json!({
                "type": "persistent",
                "ttl": format!("{}s", ttl)
            }))
        } else {
            Some(json!({
                "type": "ephemeral",
                "ttl": format!("{}s", self.prompt_cache_settings.default_ttl_seconds)
            }))
        }
    }

    fn prompt_cache_beta_header_value(&self) -> Option<String> {
        if !self.prompt_cache_enabled {
            return None;
        }

        let mut betas = vec!["prompt-caching-2024-07-31".to_string()];
        if self.prompt_cache_settings.extended_ttl_seconds.is_some() {
            betas.push("extended-cache-ttl-2025-04-11".to_string());
        }

        Some(betas.join(", "))
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
                if let Some(request) = self.parse_messages_request(&value) {
                    return request;
                }
            }
        }

        self.default_request(prompt)
    }

    fn parse_messages_request(&self, value: &Value) -> Option<LLMRequest> {
        let messages_value = value.get("messages")?.as_array()?;
        let mut system_prompt = value
            .get("system")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        let mut messages = Vec::new();

        for entry in messages_value {
            let role = entry
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or(crate::config::constants::message_roles::USER);

            match role {
                "assistant" => {
                    let mut text_content = String::new();
                    let mut tool_calls = Vec::new();

                    if let Some(content_array) = entry.get("content").and_then(|c| c.as_array()) {
                        for block in content_array {
                            match block.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                        text_content.push_str(text);
                                    }
                                }
                                Some("tool_use") => {
                                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                    let name =
                                        block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    let input =
                                        block.get("input").cloned().unwrap_or_else(|| json!({}));
                                    let arguments = serde_json::to_string(&input)
                                        .unwrap_or_else(|_| "{}".to_string());
                                    if !id.is_empty() && !name.is_empty() {
                                        tool_calls.push(ToolCall::function(
                                            id.to_string(),
                                            name.to_string(),
                                            arguments,
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(content_text) = entry.get("content").and_then(|c| c.as_str())
                    {
                        text_content.push_str(content_text);
                    }

                    let message = if tool_calls.is_empty() {
                        Message::assistant(text_content)
                    } else {
                        Message::assistant_with_tools(text_content, tool_calls)
                    };
                    messages.push(message);
                }
                "user" => {
                    let mut text_buffer = String::new();
                    let mut pending_tool_results = Vec::new();

                    if let Some(content_array) = entry.get("content").and_then(|c| c.as_array()) {
                        for block in content_array {
                            match block.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                        text_buffer.push_str(text);
                                    }
                                }
                                Some("tool_result") => {
                                    if !text_buffer.is_empty() {
                                        messages.push(Message::user(text_buffer.clone()));
                                        text_buffer.clear();
                                    }
                                    if let Some(tool_use_id) =
                                        block.get("tool_use_id").and_then(|id| id.as_str())
                                    {
                                        let serialized = Self::flatten_tool_result_content(block);
                                        pending_tool_results
                                            .push((tool_use_id.to_string(), serialized));
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(content_text) = entry.get("content").and_then(|c| c.as_str())
                    {
                        text_buffer.push_str(content_text);
                    }

                    if !text_buffer.is_empty() {
                        messages.push(Message::user(text_buffer));
                    }

                    for (tool_use_id, payload) in pending_tool_results {
                        messages.push(Message::tool_response(tool_use_id, payload));
                    }
                }
                "system" => {
                    if system_prompt.is_none() {
                        let extracted = if let Some(content_array) =
                            entry.get("content").and_then(|c| c.as_array())
                        {
                            content_array
                                .iter()
                                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("")
                        } else {
                            entry
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string()
                        };
                        if !extracted.is_empty() {
                            system_prompt = Some(extracted);
                        }
                    }
                }
                _ => {
                    if let Some(content_text) = entry.get("content").and_then(|c| c.as_str()) {
                        messages.push(Message::user(content_text.to_string()));
                    }
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
                    let name = tool.get("name").and_then(|n| n.as_str())?;
                    let description = tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let parameters = tool
                        .get("input_schema")
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
        let parallel_tool_config = value
            .get("parallel_tool_config")
            .cloned()
            .and_then(|cfg| serde_json::from_value::<ParallelToolConfig>(cfg).ok());
        let reasoning_effort = value
            .get("reasoning_effort")
            .and_then(|r| r.as_str())
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
            parallel_tool_config,
            reasoning_effort,
        })
    }

    fn parse_tool_choice(choice: &Value) -> Option<ToolChoice> {
        match choice {
            Value::String(value) => match value.as_str() {
                "auto" => Some(ToolChoice::auto()),
                "none" => Some(ToolChoice::none()),
                "any" => Some(ToolChoice::any()),
                _ => None,
            },
            Value::Object(map) => {
                let choice_type = map.get("type").and_then(|t| t.as_str())?;
                match choice_type {
                    "auto" => Some(ToolChoice::auto()),
                    "none" => Some(ToolChoice::none()),
                    "any" => Some(ToolChoice::any()),
                    "tool" => map
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|name| ToolChoice::function(name.to_string())),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn flatten_tool_result_content(block: &Value) -> String {
        if let Some(items) = block.get("content").and_then(|content| content.as_array()) {
            let mut aggregated = String::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    aggregated.push_str(text);
                } else {
                    aggregated.push_str(&item.to_string());
                }
            }
            if aggregated.is_empty() {
                block
                    .get("content")
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            } else {
                aggregated
            }
        } else if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
            text.to_string()
        } else {
            block.to_string()
        }
    }

    fn tool_result_blocks(content: &str) -> Vec<Value> {
        if content.trim().is_empty() {
            return vec![json!({"type": "text", "text": ""})];
        }

        if let Ok(parsed) = serde_json::from_str::<Value>(content) {
            match parsed {
                Value::String(text) => vec![json!({"type": "text", "text": text})],
                Value::Array(items) => {
                    let mut blocks = Vec::new();
                    for item in items {
                        if let Some(text) = item.as_str() {
                            blocks.push(json!({"type": "text", "text": text}));
                        } else {
                            blocks.push(json!({"type": "json", "json": item}));
                        }
                    }
                    if blocks.is_empty() {
                        vec![json!({"type": "json", "json": Value::Array(vec![])})]
                    } else {
                        blocks
                    }
                }
                other => vec![json!({"type": "json", "json": other})],
            }
        } else {
            vec![json!({"type": "text", "text": content})]
        }
    }

    fn convert_to_anthropic_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let cache_control_template = if self.prompt_cache_enabled {
            self.cache_control_value()
        } else {
            None
        };

        let mut breakpoints_remaining = cache_control_template
            .as_ref()
            .map(|_| self.prompt_cache_settings.max_breakpoints as usize)
            .unwrap_or(0);

        let mut tools_json: Option<Vec<Value>> = None;
        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let mut built_tools: Vec<Value> = tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.function.name,
                            "description": tool.function.description,
                            "input_schema": tool.function.parameters
                        })
                    })
                    .collect();

                if breakpoints_remaining > 0 {
                    if let Some(cache_control) = cache_control_template.as_ref() {
                        if let Some(last_tool) = built_tools.last_mut() {
                            last_tool["cache_control"] = cache_control.clone();
                            breakpoints_remaining -= 1;
                        }
                    }
                }

                tools_json = Some(built_tools);
            }
        }

        let mut system_value: Option<Value> = None;
        if let Some(system_prompt) = &request.system_prompt {
            if self.prompt_cache_settings.cache_system_messages && breakpoints_remaining > 0 {
                if let Some(cache_control) = cache_control_template.as_ref() {
                    let mut block = json!({
                        "type": "text",
                        "text": system_prompt
                    });
                    block["cache_control"] = cache_control.clone();
                    system_value = Some(Value::Array(vec![block]));
                    breakpoints_remaining -= 1;
                } else {
                    system_value = Some(Value::String(system_prompt.clone()));
                }
            } else {
                system_value = Some(Value::String(system_prompt.clone()));
            }
        }

        let mut messages = Vec::new();

        for msg in &request.messages {
            if msg.role == MessageRole::System {
                continue;
            }

            match msg.role {
                MessageRole::Assistant => {
                    let mut content_blocks = Vec::new();
                    if !msg.content.is_empty() {
                        content_blocks.push(json!({"type": "text", "text": msg.content}));
                    }
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            let args: Value = serde_json::from_str(&call.function.arguments)
                                .unwrap_or_else(|_| json!({}));
                            content_blocks.push(json!({
                                "type": "tool_use",
                                "id": call.id,
                                "name": call.function.name,
                                "input": args
                            }));
                        }
                    }
                    if content_blocks.is_empty() {
                        content_blocks.push(json!({"type": "text", "text": ""}));
                    }
                    messages.push(json!({
                        "role": "assistant",
                        "content": content_blocks
                    }));
                }
                MessageRole::Tool => {
                    if let Some(tool_call_id) = &msg.tool_call_id {
                        let blocks = Self::tool_result_blocks(&msg.content);
                        messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_call_id,
                                "content": blocks
                            }]
                        }));
                    } else if !msg.content.is_empty() {
                        messages.push(json!({
                            "role": "user",
                            "content": [{"type": "text", "text": msg.content}]
                        }));
                    }
                }
                _ => {
                    if msg.content.is_empty() {
                        continue;
                    }

                    let mut block = json!({
                        "type": "text",
                        "text": msg.content
                    });

                    if msg.role == MessageRole::User
                        && self.prompt_cache_settings.cache_user_messages
                        && breakpoints_remaining > 0
                    {
                        if let Some(cache_control) = cache_control_template.as_ref() {
                            block["cache_control"] = cache_control.clone();
                            breakpoints_remaining -= 1;
                        }
                    }

                    messages.push(json!({
                        "role": msg.role.as_anthropic_str(),
                        "content": [block]
                    }));
                }
            }
        }

        if messages.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                "No convertible messages for Anthropic request",
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        let mut anthropic_request = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
            "max_tokens": request
                .max_tokens
                .unwrap_or(defaults::ANTHROPIC_DEFAULT_MAX_TOKENS),
        });

        if let Some(system) = system_value {
            anthropic_request["system"] = system;
        }

        if let Some(temperature) = request.temperature {
            anthropic_request["temperature"] = json!(temperature);
        }

        if let Some(tools) = tools_json {
            anthropic_request["tools"] = Value::Array(tools);
        }

        if let Some(tool_choice) = &request.tool_choice {
            anthropic_request["tool_choice"] = tool_choice.to_provider_format("anthropic");
        }

        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
                if let Some(payload) = reasoning_parameters_for(Provider::Anthropic, effort) {
                    anthropic_request["reasoning"] = payload;
                } else {
                    anthropic_request["reasoning"] = json!({ "effort": effort.as_str() });
                }
            }
        }

        Ok(anthropic_request)
    }

    fn parse_anthropic_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let content = response_json
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted = error_display::format_llm_error(
                    "Anthropic",
                    "Invalid response format: missing content",
                );
                LLMError::Provider(formatted)
            })?;

        let mut text_parts = Vec::new();
        let mut reasoning_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                Some("thinking") => {
                    if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                        reasoning_parts.push(thinking.to_string());
                    } else if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        reasoning_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(ToolCall::function(id, name, arguments));
                    }
                }
                _ => {}
            }
        }

        let reasoning = if reasoning_parts.is_empty() {
            response_json
                .get("reasoning")
                .and_then(extract_reasoning_trace)
        } else {
            let joined = reasoning_parts.join("\n");
            let trimmed = joined.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };

        let stop_reason = response_json
            .get("stop_reason")
            .and_then(|sr| sr.as_str())
            .unwrap_or("end_turn");
        let finish_reason = match stop_reason {
            "end_turn" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "stop_sequence" => FinishReason::Stop,
            "tool_use" => FinishReason::ToolCalls,
            other => FinishReason::Error(other.to_string()),
        };

        let usage = response_json.get("usage").map(|usage_value| {
            let cache_creation_tokens = usage_value
                .get("cache_creation_input_tokens")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32);
            let cache_read_tokens = usage_value
                .get("cache_read_input_tokens")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32);

            crate::llm::provider::Usage {
                prompt_tokens: usage_value
                    .get("input_tokens")
                    .and_then(|it| it.as_u64())
                    .unwrap_or(0) as u32,
                completion_tokens: usage_value
                    .get("output_tokens")
                    .and_then(|ot| ot.as_u64())
                    .unwrap_or(0) as u32,
                total_tokens: (usage_value
                    .get("input_tokens")
                    .and_then(|it| it.as_u64())
                    .unwrap_or(0)
                    + usage_value
                        .get("output_tokens")
                        .and_then(|ot| ot.as_u64())
                        .unwrap_or(0)) as u32,
                cached_prompt_tokens: cache_read_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            }
        });

        Ok(LLMResponse {
            content: if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join(""))
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            finish_reason,
            reasoning,
        })
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
        models::anthropic::REASONING_MODELS
            .iter()
            .any(|candidate| *candidate == requested)
    }

    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        true
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let anthropic_request = self.convert_to_anthropic_format(&request)?;
        let url = format!("{}/messages", self.base_url);

        let mut request_builder = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", urls::ANTHROPIC_API_VERSION);

        if let Some(beta_header) = self.prompt_cache_beta_header_value() {
            request_builder = request_builder.header("anthropic-beta", beta_header);
        }

        let response = request_builder
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("Anthropic", &format!("Network error: {}", e));
                LLMError::Network(formatted_error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            // Handle specific HTTP status codes
            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit);
            }

            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!("HTTP {}: {}", status, error_text),
            );
            return Err(LLMError::Provider(formatted_error));
        }

        let anthropic_response: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        self.parse_anthropic_response(anthropic_response)
    }

    fn supported_models(&self) -> Vec<String> {
        models::anthropic::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("Anthropic", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        if !self.supported_models().contains(&request.model) {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("anthropic") {
                let formatted = error_display::format_llm_error("Anthropic", &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::core::PromptCachingConfig;
    use crate::llm::provider::{Message, ToolDefinition};
    use serde_json::{Value, json};

    fn base_prompt_cache_config() -> PromptCachingConfig {
        let mut config = PromptCachingConfig::default();
        config.enabled = true;
        config.providers.anthropic.enabled = true;
        config.providers.anthropic.max_breakpoints = 3;
        config.providers.anthropic.cache_user_messages = true;
        config.providers.anthropic.extended_ttl_seconds = Some(3600);
        config
    }

    fn sample_request() -> LLMRequest {
        let tool = ToolDefinition::function(
            "get_weather".to_string(),
            "Retrieve the weather for a city".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }),
        );

        LLMRequest {
            messages: vec![Message::user("What's the forecast?".to_string())],
            system_prompt: Some("You are a weather assistant".to_string()),
            tools: Some(vec![tool]),
            model: models::CLAUDE_SONNET_4_5.to_string(),
            max_tokens: Some(512),
            temperature: Some(0.2),
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn convert_to_anthropic_format_injects_cache_control() {
        let config = base_prompt_cache_config();
        let provider = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::CLAUDE_SONNET_4_5.to_string()),
            None,
            Some(config),
        );

        let request = sample_request();
        let converted = provider
            .convert_to_anthropic_format(&request)
            .expect("conversion should succeed");

        let tools = converted["tools"].as_array().expect("tools array");
        let tool_cache = tools
            .last()
            .and_then(|value| value.get("cache_control"))
            .expect("tool cache control present");
        assert_eq!(tool_cache["type"], "persistent");
        assert_eq!(tool_cache["ttl"], "3600s");

        let system = converted["system"].as_array().expect("system array");
        let system_cache = system[0]
            .get("cache_control")
            .expect("system cache control present");
        assert_eq!(system_cache["type"], "persistent");

        let messages = converted["messages"].as_array().expect("messages array");
        let user_message = messages
            .iter()
            .find(|msg| msg["role"] == "user")
            .expect("user message exists");
        let user_cache = user_message["content"][0]
            .get("cache_control")
            .expect("user cache control present");
        assert_eq!(user_cache["type"], "persistent");
    }

    #[test]
    fn cache_headers_reflect_extended_ttl() {
        let config = base_prompt_cache_config();
        let provider = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::CLAUDE_SONNET_4_5.to_string()),
            None,
            Some(config),
        );

        let beta_header = provider
            .prompt_cache_beta_header_value()
            .expect("beta header present when caching enabled");
        assert!(beta_header.contains("prompt-caching-2024-07-31"));
        assert!(beta_header.contains("extended-cache-ttl-2025-04-11"));
    }

    #[test]
    fn cache_control_absent_when_disabled() {
        let mut config = PromptCachingConfig::default();
        config.enabled = false;
        config.providers.anthropic.enabled = false;

        let provider = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::CLAUDE_SONNET_4_5.to_string()),
            None,
            Some(config),
        );

        let request = sample_request();
        let converted = provider
            .convert_to_anthropic_format(&request)
            .expect("conversion should succeed even without caching");

        assert!(
            converted["tools"].as_array().unwrap()[0]
                .get("cache_control")
                .is_none()
        );

        if let Some(system_value) = converted.get("system") {
            match system_value {
                Value::Array(blocks) => {
                    assert!(blocks[0].get("cache_control").is_none());
                }
                Value::String(_) => {}
                _ => panic!("unexpected system value"),
            }
        }

        let messages = converted["messages"].as_array().expect("messages array");
        let user_message = messages
            .iter()
            .find(|msg| msg["role"] == "user")
            .expect("user message exists");
        assert!(user_message["content"][0].get("cache_control").is_none());
    }
}

#[async_trait]
impl LLMClient for AnthropicProvider {
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
        llm_types::BackendKind::Anthropic
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
