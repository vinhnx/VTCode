#![allow(clippy::result_large_err)]
use crate::config::TimeoutsConfig;
use crate::config::constants::{defaults, env_vars, models, urls};
use crate::config::core::{AnthropicConfig, AnthropicPromptCacheSettings, PromptCachingConfig};
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
use std::env;

use super::{
    common::{
        convert_usage_to_llm_types, extract_prompt_cache_settings, override_base_url,
        parse_client_prompt_common, resolve_model,
    },
    error_handling::{format_network_error, format_parse_error, handle_anthropic_http_error},
    extract_reasoning_trace,
};

pub struct AnthropicProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: AnthropicPromptCacheSettings,
    anthropic_config: AnthropicConfig,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::anthropic::DEFAULT_MODEL.to_string(),
            None,
            None,
            AnthropicConfig::default(),
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, AnthropicConfig::default())
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        anthropic_config: Option<AnthropicConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::anthropic::DEFAULT_MODEL);

        let anthropic_cfg = anthropic_config.unwrap_or_default();
        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            anthropic_cfg,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        anthropic_config: AnthropicConfig,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.anthropic,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let base_url_value = if model.as_str() == models::minimax::MINIMAX_M2 {
            Self::resolve_minimax_base_url(base_url)
        } else {
            override_base_url(
                urls::ANTHROPIC_API_BASE,
                base_url,
                Some(env_vars::ANTHROPIC_BASE_URL),
            )
        };

        Self {
            api_key,
            http_client: HttpClient::new(),
            base_url: base_url_value,
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
            anthropic_config,
        }
    }

    fn resolve_minimax_base_url(base_url: Option<String>) -> String {
        fn sanitize(value: &str) -> Option<String> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.trim_end_matches('/').to_string())
            }
        }

        let resolved = base_url
            .and_then(|value| sanitize(&value))
            .or_else(|| {
                env::var(env_vars::MINIMAX_BASE_URL)
                    .ok()
                    .and_then(|value| sanitize(&value))
            })
            .or_else(|| {
                env::var(env_vars::ANTHROPIC_BASE_URL)
                    .ok()
                    .and_then(|value| sanitize(&value))
            })
            .or_else(|| sanitize(urls::MINIMAX_API_BASE))
            .unwrap_or_else(|| urls::MINIMAX_API_BASE.trim_end_matches('/').to_string());

        let mut normalized = resolved;

        if normalized.ends_with("/messages") {
            normalized = normalized
                .trim_end_matches("/messages")
                .trim_end_matches('/')
                .to_string();
        }

        if let Some(pos) = normalized.find("/v1/") {
            normalized = normalized[..pos + 3].to_string();
        }

        if !normalized.ends_with("/v1") {
            normalized = format!("{}/v1", normalized);
        }

        normalized
    }

    /// Determines the TTL string for cache control.
    /// Anthropic only supports "5m" (5 minutes) or "1h" (1 hour).
    ///
    /// Returns:
    /// - "1h" if extended_ttl_seconds is set and >= 3600 seconds
    /// - "5m" for default or extended_ttl_seconds < 3600 seconds
    fn get_cache_ttl(&self) -> &'static str {
        self.prompt_cache_settings
            .extended_ttl_seconds
            .filter(|&ttl| ttl >= 3600)
            .map(|_| "1h")
            .unwrap_or("5m")
    }

    /// Returns the beta header value for Anthropic API prompt caching.
    /// - Always includes "prompt-caching-2024-07-31"
    /// - Adds "extended-cache-ttl-2025-04-11" only when using 1h TTL
    fn prompt_cache_beta_header_value(&self) -> Option<String> {
        if !self.prompt_cache_enabled {
            return None;
        }

        let mut betas = vec!["prompt-caching-2024-07-31"];

        // Only add extended TTL beta if we're actually using 1h cache
        if self.get_cache_ttl() == "1h" {
            betas.push("extended-cache-ttl-2025-04-11");
        }

        Some(betas.join(", "))
    }

    /// Returns true if the model is a Claude model supported by the Anthropic provider.
    #[allow(dead_code)]
    fn is_claude_model(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };
        models::anthropic::SUPPORTED_MODELS.contains(&requested)
    }

    /// Combines prompt cache betas with structured outputs beta when requested.
    /// Always includes interleaved-thinking beta for all Anthropic models.
    fn combined_beta_header_value(&self, include_structured: bool) -> Option<String> {
        let mut pieces: Vec<String> = Vec::new();
        if let Some(pc) = self.prompt_cache_beta_header_value() {
            for p in pc
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
            {
                pieces.push(p);
            }
        }
        // Always add interleaved-thinking beta header for all Anthropic models
        pieces.push(self.anthropic_config.interleaved_thinking_beta.clone());
        if include_structured {
            // Use the correct beta header for structured outputs
            pieces.push("structured-outputs-2025-11-13".to_owned());
        }
        if pieces.is_empty() {
            None
        } else {
            Some(pieces.join(", "))
        }
    }

    fn parse_client_prompt(&self, prompt: &str) -> LLMRequest {
        parse_client_prompt_common(prompt, &self.model, |value| {
            self.parse_messages_request(value)
        })
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
                                .collect::<String>()
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
                    let mut tool_def =
                        ToolDefinition::function(name.to_string(), description, parameters);
                    if let Some(strict_val) = tool.get("strict").and_then(|v| v.as_bool()) {
                        tool_def = tool_def.with_strict(strict_val);
                    }
                    Some(tool_def)
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

        let output_format = value.get("output_format").cloned();

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
            output_format,
            ..Default::default()
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

    #[allow(clippy::result_large_err)]
    fn convert_to_anthropic_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        use super::anthropic_types::{
            AnthropicContentBlock, AnthropicMessage, AnthropicRequest, AnthropicTool, CacheControl,
        };

        let cache_control_template = if self.prompt_cache_enabled {
            let ttl = self.get_cache_ttl();
            Some(CacheControl {
                control_type: "ephemeral".to_string(),
                ttl: Some(ttl.to_string()),
            })
        } else {
            None
        };

        let mut breakpoints_remaining = cache_control_template
            .as_ref()
            .map(|_| self.prompt_cache_settings.max_breakpoints as usize)
            .unwrap_or(0);

        let mut tools: Option<Vec<AnthropicTool>> = None;
        if let Some(request_tools) = &request.tools
            && !request_tools.is_empty()
        {
            let mut built_tools: Vec<AnthropicTool> = request_tools
                .iter()
                .filter_map(|tool| {
                    let func = tool.function.as_ref()?;
                    Some(AnthropicTool {
                        name: func.name.clone(),
                        description: func.description.clone(),
                        input_schema: func.parameters.clone(),
                        cache_control: None,
                    })
                })
                .collect();

            if breakpoints_remaining > 0
                && let Some(cache_control) = cache_control_template.as_ref()
                && let Some(last_tool) = built_tools.last_mut()
            {
                last_tool.cache_control = Some(cache_control.clone());
                breakpoints_remaining -= 1;
            }

            if !built_tools.is_empty() {
                tools = Some(built_tools);
            }
        }

        let mut system_value: Option<Value> = None;
        if let Some(system_prompt) = &request.system_prompt {
            if self.prompt_cache_settings.cache_system_messages && breakpoints_remaining > 0 {
                if let Some(cache_control) = cache_control_template.as_ref() {
                    let block = json!({
                        "type": "text",
                        "text": system_prompt,
                        "cache_control": cache_control
                    });
                    system_value = Some(Value::Array(vec![block]));
                    breakpoints_remaining -= 1;
                } else {
                    system_value = Some(Value::String(system_prompt.clone()));
                }
            } else {
                system_value = Some(Value::String(system_prompt.clone()));
            }
        }

        let mut messages = Vec::with_capacity(request.messages.len());

        for msg in &request.messages {
            if msg.role == MessageRole::System {
                continue;
            }

            let mut blocks = Vec::new();
            let content_text = msg.content.as_text();

            match msg.role {
                MessageRole::Assistant => {
                    if let Some(details) = &msg.reasoning_details {
                        for detail in details {
                            if detail.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                                let thinking = detail
                                    .get("thinking")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let signature = detail
                                    .get("signature")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !thinking.is_empty() && !signature.is_empty() {
                                    blocks.push(AnthropicContentBlock::Thinking {
                                        thinking,
                                        signature,
                                        cache_control: None,
                                    });
                                }
                            }
                        }
                    }

                    if !msg.content.is_empty() {
                        blocks.push(AnthropicContentBlock::Text {
                            text: content_text.to_string(),
                            cache_control: None,
                        });
                    }
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                let args: Value = serde_json::from_str(&func.arguments)
                                    .unwrap_or_else(|_| json!({}));
                                blocks.push(AnthropicContentBlock::ToolUse {
                                    id: call.id.clone(),
                                    name: func.name.clone(),
                                    input: args,
                                    cache_control: None,
                                });
                            }
                        }
                    }
                    if blocks.is_empty() {
                        blocks.push(AnthropicContentBlock::Text {
                            text: String::new(),
                            cache_control: None,
                        });
                    }
                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: blocks,
                    });
                }
                MessageRole::Tool => {
                    if let Some(tool_call_id) = &msg.tool_call_id {
                        let tool_content_blocks = Self::tool_result_blocks(&content_text);
                        let content_val = if tool_content_blocks.len() == 1
                            && tool_content_blocks[0]["type"] == "text"
                        {
                            json!(tool_content_blocks[0]["text"])
                        } else {
                            json!(tool_content_blocks)
                        };

                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: vec![AnthropicContentBlock::ToolResult {
                                tool_use_id: tool_call_id.clone(),
                                content: content_val,
                                is_error: None,
                                cache_control: None,
                            }],
                        });
                    } else if !msg.content.is_empty() {
                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: vec![AnthropicContentBlock::Text {
                                text: content_text.to_string(),
                                cache_control: None,
                            }],
                        });
                    }
                }
                _ => {
                    if msg.content.is_empty() {
                        continue;
                    }

                    let mut cache_ctrl = None;
                    if msg.role == MessageRole::User
                        && self.prompt_cache_settings.cache_user_messages
                        && breakpoints_remaining > 0
                        && let Some(template) = cache_control_template.as_ref()
                    {
                        cache_ctrl = Some(template.clone());
                        breakpoints_remaining -= 1;
                    }

                    messages.push(AnthropicMessage {
                        role: msg.role.as_anthropic_str().to_string(),
                        content: vec![AnthropicContentBlock::Text {
                            text: content_text.to_string(),
                            cache_control: cache_ctrl,
                        }],
                    });
                }
            }
        }

        if messages.is_empty() {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                "No convertible messages for Anthropic request",
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        // Enable interleaved thinking by default for all Anthropic models
        let mut reasoning_val = None;
        if self.supports_reasoning_effort(&request.model) {
            // Always enable thinking with configured budget tokens for all supported models
            reasoning_val = Some(super::common::make_anthropic_thinking_config(
                &self.anthropic_config,
            ));
        } else if let Some(effort) = request.reasoning_effort {
            // Fallback to effort-based reasoning if model doesn't support interleaved thinking
            if let Some(payload) = reasoning_parameters_for(Provider::Anthropic, effort) {
                reasoning_val = Some(payload);
            } else {
                reasoning_val = Some(json!({ "effort": effort.as_str() }));
            }
        }

        if let Some(schema) = &request.output_format
            && self.supports_structured_output(&request.model)
        {
            let structured_tool = AnthropicTool {
                name: "structured_output".to_string(),
                description: "Forces Claude to respond in a specific JSON format according to the provided schema".to_string(),
                input_schema: schema.clone(),
                cache_control: None,
            };

            if let Some(tools_vec) = &mut tools {
                tools_vec.push(structured_tool);
            } else {
                tools = Some(vec![structured_tool]);
            }
        }

        let mut final_tool_choice = request
            .tool_choice
            .as_ref()
            .map(|tc| tc.to_provider_format("anthropic"));
        if request.output_format.is_some() && self.supports_structured_output(&request.model) {
            final_tool_choice = Some(json!({
                "type": "tool",
                "name": "structured_output"
            }));
        }

        let anthropic_request = AnthropicRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request
                .max_tokens
                .unwrap_or(defaults::ANTHROPIC_DEFAULT_MAX_TOKENS),
            system: system_value,
            temperature: if self.supports_reasoning_effort(&request.model) {
                None
            } else {
                request.temperature
            },
            tools,
            tool_choice: final_tool_choice,
            reasoning: reasoning_val,
            stream: request.stream,
        };

        serde_json::to_value(anthropic_request).map_err(|e| LLMError::Provider {
            message: format!("Serialization error: {}", e),
            metadata: None,
        })
    }

    #[allow(clippy::result_large_err)]
    fn parse_anthropic_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let content = response_json
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                let formatted = error_display::format_llm_error(
                    "Anthropic",
                    "Invalid response format: missing content",
                );
                LLMError::Provider {
                    message: formatted,
                    metadata: None,
                }
            })?;

        let mut text_parts = Vec::new();
        let mut reasoning_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut reasoning_details_vec = Vec::new();

        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                Some("thinking") => {
                    // Store the raw block (including signature) for hydration
                    reasoning_details_vec.push(block.clone());

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

                    // Special handling for structured output tools
                    if name == "structured_output" {
                        // For structured output, we should treat the input as the main content
                        let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                        // Convert the structured output to text for the content field
                        let output_text =
                            serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                        text_parts.push(output_text);
                    } else {
                        // Handle regular tools
                        let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                        let arguments =
                            serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                        if !id.is_empty() && !name.is_empty() {
                            tool_calls.push(ToolCall::function(id, name, arguments));
                        }
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
                Some(text_parts.into_iter().collect())
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
            finish_reason,
            reasoning,
            reasoning_details: if reasoning_details_vec.is_empty() {
                None
            } else {
                Some(reasoning_details_vec)
            },
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

        if requested == models::minimax::MINIMAX_M2 {
            return true;
        }

        models::anthropic::REASONING_MODELS.contains(&requested)
    }

    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        true
    }

    /// Check if the Anthropic provider supports structured outputs for the given model.
    ///
    /// According to Anthropic documentation, structured outputs are available
    /// for Claude 4 and Claude 4.5 models, including Sonnet, Haiku, and Opus variants.
    ///
    /// This feature allows Claude to guarantee responses that follow a specific JSON schema,
    /// ensuring valid, parseable output for downstream processing.
    fn supports_structured_output(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        // Structured outputs are available for Claude 4.5 models and their aliases
        requested == models::anthropic::CLAUDE_SONNET_4_5
            || requested == models::anthropic::CLAUDE_SONNET_4_5_20250929
            || requested == models::anthropic::CLAUDE_OPUS_4_1
            || requested == models::anthropic::CLAUDE_OPUS_4_1_20250805
            || requested == models::anthropic::CLAUDE_HAIKU_4_5
            || requested == models::anthropic::CLAUDE_HAIKU_4_5_20251001
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let anthropic_request = self.convert_to_anthropic_format(&request)?;
        let url = format!("{}/messages", self.base_url);

        let mut request_builder = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", urls::ANTHROPIC_API_VERSION);

        let include_structured = anthropic_request.get("output_format").is_some();
        if let Some(beta_header) = self.combined_beta_header_value(include_structured) {
            request_builder = request_builder.header("anthropic-beta", beta_header);
        }

        let response = request_builder
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| format_network_error("Anthropic", &e))?;

        let response = handle_anthropic_http_error(response).await?;

        let anthropic_response: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("Anthropic", &e))?;

        self.parse_anthropic_response(anthropic_response)
    }

    fn supported_models(&self) -> Vec<String> {
        let mut supported: Vec<String> = models::anthropic::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect();

        supported.extend(
            models::minimax::SUPPORTED_MODELS
                .iter()
                .map(|s| s.to_string()),
        );

        supported.sort();
        supported.dedup();
        supported
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("Anthropic", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        if !models::anthropic::SUPPORTED_MODELS
            .iter()
            .any(|m| *m == request.model)
        {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        // Check if structured output is requested on an unsupported model
        if request.output_format.is_some() && !self.supports_structured_output(&request.model) {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!(
                    "Structured output is not supported for model '{}'. Structured outputs are only available for Claude Sonnet 4.5 and Claude Opus 4.1 models.",
                    request.model
                ),
            );
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        // Validate the schema if structured output is requested
        // This checks for Anthropic-specific JSON Schema limitations such as:
        // - No numeric constraints (minimum, maximum, multipleOf)
        // - No string length constraints (minLength, maxLength)
        // - Array minItems only supports values 0 or 1
        // - additionalProperties must be false for objects
        if let Some(ref schema) = request.output_format
            && self.supports_structured_output(&request.model)
        {
            self.validate_anthropic_schema(schema)?;
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("anthropic") {
                let formatted = error_display::format_llm_error("Anthropic", &err);
                return Err(LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
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
            ..Default::default()
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
            None,
            None,
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
        assert_eq!(tool_cache["type"], "ephemeral");
        assert_eq!(tool_cache["ttl"], "1h");

        let system = converted["system"].as_array().expect("system array");
        let system_cache = system[0]
            .get("cache_control")
            .expect("system cache control present");
        assert_eq!(system_cache["type"], "ephemeral");

        let messages = converted["messages"].as_array().expect("messages array");
        let user_message = messages
            .iter()
            .find(|msg| msg["role"] == "user")
            .expect("user message exists");
        let user_cache = user_message["content"][0]
            .get("cache_control")
            .expect("user cache control present");
        assert_eq!(user_cache["type"], "ephemeral");
    }

    #[test]
    fn cache_headers_reflect_extended_ttl() {
        let config = base_prompt_cache_config();
        let provider = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::CLAUDE_SONNET_4_5.to_string()),
            None,
            Some(config),
            None,
            None,
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
            None,
            None,
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

    #[test]
    fn test_structured_output_support() {
        let provider = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::CLAUDE_SONNET_4_5.to_string()),
            None,
            None,
            None,
            None,
        );

        // Claude Sonnet 4.5 should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_SONNET_4_5));

        // Claude Opus 4.1 should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_OPUS_4_1_20250805));

        // Claude Sonnet 4.5 should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_SONNET_4_5));

        // Claude Sonnet 4.5 (versioned) should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_SONNET_4_5_20250929));

        // Claude Opus 4.1 should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_OPUS_4_1));

        // Claude Haiku 4.5 should support structured output
        assert!(provider.supports_structured_output(models::CLAUDE_HAIKU_4_5));

        // Test with empty model string (should use provider's default)
        let provider_default = AnthropicProvider::from_config(
            Some("key".to_string()),
            Some(models::anthropic::DEFAULT_MODEL.to_string()),
            None,
            None,
            None,
            None,
        );
        assert!(provider_default.supports_structured_output(""));
    }

    #[test]
    fn test_structured_output_schema_validation() {
        let provider = AnthropicProvider::new("key".to_string());

        // Valid schema should pass
        let valid_schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"],
            "additionalProperties": false
        });
        assert!(provider.validate_anthropic_schema(&valid_schema).is_ok());

        // Schema with unsupported numeric constraints should fail
        let invalid_schema = json!({
            "type": "object",
            "properties": {
                "age": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 100
                }
            },
            "required": ["age"],
            "additionalProperties": false
        });
        assert!(provider.validate_anthropic_schema(&invalid_schema).is_err());

        // Schema with unsupported string constraints should fail
        let invalid_string_schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50
                }
            },
            "required": ["name"],
            "additionalProperties": false
        });
        assert!(
            provider
                .validate_anthropic_schema(&invalid_string_schema)
                .is_err()
        );

        // Schema with minItems > 1 should fail
        let invalid_array_schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {"type": "string"},
                    "minItems": 5
                }
            },
            "required": ["items"],
            "additionalProperties": false
        });
        assert!(
            provider
                .validate_anthropic_schema(&invalid_array_schema)
                .is_err()
        );

        // Schema with additionalProperties: true should fail
        let invalid_additional_props_schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            },
            "required": ["name"],
            "additionalProperties": true
        });
        assert!(
            provider
                .validate_anthropic_schema(&invalid_additional_props_schema)
                .is_err()
        );
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
            usage: response.usage.map(convert_usage_to_llm_types),
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

// Helper impl block for error parsing and schema validation methods (not part of LLMProvider trait)
impl AnthropicProvider {
    /// Validates a JSON schema against Anthropic's structured output limitations
    /// Based on Anthropic documentation: https://docs.anthropic.com/claude/reference/structured-outputs
    fn validate_anthropic_schema(&self, schema: &Value) -> Result<(), LLMError> {
        match schema {
            Value::Object(obj) => {
                // For Anthropic's output_format, the schema should be the JSON schema itself, not wrapped
                self.validate_schema_object(obj, "root")?;
            }
            Value::String(_)
            | Value::Number(_)
            | Value::Bool(_)
            | Value::Array(_)
            | Value::Null => {
                let formatted_error = error_display::format_llm_error(
                    "Anthropic",
                    "Structured output schema must be a JSON object",
                );
                return Err(LLMError::InvalidRequest {
                    message: formatted_error,
                    metadata: None,
                });
            }
        }
        Ok(())
    }

    /// Recursively validate an object in the JSON schema according to Anthropic limitations
    fn validate_schema_object(
        &self,
        obj: &serde_json::Map<String, Value>,
        path: &str,
    ) -> Result<(), LLMError> {
        for (key, value) in obj {
            match key.as_str() {
                // Validate type-specific limitations
                "type" => {
                    if let Some(type_str) = value.as_str() {
                        match type_str {
                            "object" | "array" | "string" | "number" | "integer" | "boolean"
                            | "null" => {}
                            _ => {
                                let formatted_error = error_display::format_llm_error(
                                    "Anthropic",
                                    &format!(
                                        "Unsupported schema type '{}', path: {}",
                                        type_str, path
                                    ),
                                );
                                return Err(LLMError::InvalidRequest {
                                    message: formatted_error,
                                    metadata: None,
                                });
                            }
                        }
                    }
                }
                // Check for unsupported numeric constraints
                "minimum" | "maximum" | "multipleOf" => {
                    let formatted_error = error_display::format_llm_error(
                        "Anthropic",
                        &format!(
                            "Numeric constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                            key, path
                        ),
                    );
                    return Err(LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    });
                }
                // Check for unsupported string constraints
                "minLength" | "maxLength" => {
                    let formatted_error = error_display::format_llm_error(
                        "Anthropic",
                        &format!(
                            "String constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                            key, path
                        ),
                    );
                    return Err(LLMError::InvalidRequest {
                        message: formatted_error,
                        metadata: None,
                    });
                }
                // Check for unsupported array constraints beyond minItems with values 0 or 1
                "minItems" | "maxItems" | "uniqueItems" => {
                    if key == "minItems" {
                        if let Some(min_items) = value.as_u64()
                            && min_items > 1
                        {
                            let formatted_error = error_display::format_llm_error(
                                "Anthropic",
                                &format!(
                                    "Array minItems only supports values 0 or 1, got {}, path: {}",
                                    min_items, path
                                ),
                            );
                            return Err(LLMError::InvalidRequest {
                                message: formatted_error,
                                metadata: None,
                            });
                        }
                    } else {
                        let formatted_error = error_display::format_llm_error(
                            "Anthropic",
                            &format!(
                                "Array constraints like '{}' are not supported by Anthropic structured output. Path: {}",
                                key, path
                            ),
                        );
                        return Err(LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        });
                    }
                }
                // Check for additionalProperties - must be false for objects
                "additionalProperties" => {
                    if let Some(additional_props) = value.as_bool()
                        && additional_props
                    {
                        let formatted_error = error_display::format_llm_error(
                            "Anthropic",
                            &format!(
                                "additionalProperties must be set to false, got {}, path: {}",
                                additional_props, path
                            ),
                        );
                        return Err(LLMError::InvalidRequest {
                            message: formatted_error,
                            metadata: None,
                        });
                    }
                }
                // Recursively validate nested objects and arrays in properties
                "properties" => {
                    if let Value::Object(props) = value {
                        for (prop_name, prop_value) in props {
                            let prop_path = format!("{}.properties.{}", path, prop_name);
                            self.validate_schema_property(prop_value, &prop_path)?;
                        }
                    }
                }
                "items" => {
                    let items_path = format!("{}.items", path);
                    self.validate_schema_property(value, &items_path)?;
                }
                "enum" => {
                    // Enums are supported but with limitations (no complex types)
                    if let Value::Array(items) = value {
                        for (i, item) in items.iter().enumerate() {
                            if !self.is_valid_enum_value(item) {
                                let formatted_error = error_display::format_llm_error(
                                    "Anthropic",
                                    &format!(
                                        "Invalid enum value at index {}, path: {}. Enums in Anthropic structured output only support strings, numbers, booleans, and null.",
                                        i, path
                                    ),
                                );
                                return Err(LLMError::InvalidRequest {
                                    message: formatted_error,
                                    metadata: None,
                                });
                            }
                        }
                    }
                }
                // For other keys, check if it's a nested schema component
                _ => {
                    // If the value is an object that could be a schema, validate it recursively
                    if let Value::Object(nested_obj) = value {
                        let nested_path = format!("{}.{}", path, key);
                        self.validate_schema_object(nested_obj, &nested_path)?;
                    }
                    // If it's an array of objects that could be schemas
                    else if let Value::Array(arr) = value {
                        for (i, item) in arr.iter().enumerate() {
                            if let Value::Object(nested_obj) = item {
                                let nested_path = format!("{}.{}[{}]", path, key, i);
                                self.validate_schema_object(nested_obj, &nested_path)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate an individual schema property
    fn validate_schema_property(&self, value: &Value, path: &str) -> Result<(), LLMError> {
        match value {
            Value::Object(obj) => self.validate_schema_object(obj, path),
            Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    if let Value::Object(obj) = item {
                        let item_path = format!("{}[{}]", path, i);
                        self.validate_schema_object(obj, &item_path)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Check if an enum value is valid (string, number, boolean, or null)
    fn is_valid_enum_value(&self, value: &Value) -> bool {
        matches!(
            value,
            Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
        )
    }
}
