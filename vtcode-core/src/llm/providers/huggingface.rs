#![allow(
    clippy::result_large_err,
    clippy::bind_instead_of_map,
    clippy::collapsible_if
)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display::format_llm_error;
use crate::llm::provider::{
    LLMError, LLMErrorMetadata, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    MessageRole, ToolDefinition,
};
use crate::llm::providers::shared::{NoopStreamTelemetry, StreamTelemetry};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client as HttpClient, Response, StatusCode};
use serde_json::{Value, json};

use super::common::{
    execute_token_count_request, map_finish_reason_common, override_base_url,
    parse_prompt_tokens_from_count_response, parse_response_openai_format, resolve_model,
    strip_generation_controls_for_token_count,
};
use super::error_handling::{format_network_error, format_parse_error};

const PROVIDER_NAME: &str = "HuggingFace";
const PROVIDER_KEY: &str = "huggingface";
const JSON_INSTRUCTION: &str = "Return JSON that matches the provided schema.";

pub struct HuggingFaceProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    _timeouts: TimeoutsConfig,
    model_behavior: Option<ModelConfig>,
}

impl HuggingFaceProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::huggingface::DEFAULT_MODEL.to_string(),
            None,
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, None)
    }

    pub fn with_timeouts(api_key: String, timeouts: TimeoutsConfig) -> Self {
        Self::with_model_internal(
            api_key,
            models::huggingface::DEFAULT_MODEL.to_string(),
            None,
            Some(timeouts),
            None,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::HUGGINGFACE_API_BASE,
                base_url,
                Some(env_vars::HUGGINGFACE_BASE_URL),
            ),
            model,
            _timeouts: timeouts,
            model_behavior,
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
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::huggingface::DEFAULT_MODEL);
        Self::with_model_internal(
            api_key_value,
            model_value,
            base_url,
            timeouts,
            model_behavior,
        )
    }

    fn normalize_model_id(&self, model: &str) -> Result<String, LLMError> {
        let lower = model.to_ascii_lowercase();

        if lower.contains("minimax-m2") && !model.contains(':') {
            return Err(LLMError::Provider {
                message: format_llm_error(
                    PROVIDER_NAME,
                    "MiniMax models require explicit provider selection (:novita suffix). \n                    Use 'MiniMaxAI/MiniMax-M2.5:novita'.",
                ),
                metadata: None,
            });
        }

        if lower.contains("glm-5") && !model.contains(':') {
            return Err(LLMError::Provider {
                message: format_llm_error(
                    PROVIDER_NAME,
                    "GLM models require explicit provider selection on HuggingFace.",
                ),
                metadata: None,
            });
        }

        Ok(model.to_string())
    }

    fn serialize_tools_huggingface(&self, tools: &[ToolDefinition]) -> Option<Vec<Value>> {
        crate::llm::providers::common::serialize_tools_openai_format(tools)
    }

    fn serialize_messages_huggingface_chat(
        &self,
        request: &LLMRequest,
    ) -> Result<Vec<Value>, LLMError> {
        use serde_json::{Map, json};

        let mut messages = Vec::with_capacity(request.messages.len());

        for message in &request.messages {
            message
                .validate_for_provider(PROVIDER_KEY)
                .map_err(|e| LLMError::InvalidRequest {
                    message: e,
                    metadata: None,
                })?;

            let mut message_map = Map::with_capacity(4);
            message_map.insert(
                "role".to_owned(),
                Value::String(message.role.as_generic_str().to_owned()),
            );

            match &message.content {
                crate::llm::provider::MessageContent::Text(text) => {
                    message_map.insert("content".to_owned(), Value::String(text.clone()));
                }
                crate::llm::provider::MessageContent::Parts(parts) => {
                    let has_images = parts
                        .iter()
                        .any(crate::llm::provider::ContentPart::is_image);
                    if has_images {
                        let parts_json: Vec<Value> = parts
                            .iter()
                            .map(|part| match part {
                                crate::llm::provider::ContentPart::Text { text } => {
                                    json!({ "type": "text", "text": text })
                                }
                                crate::llm::provider::ContentPart::Image {
                                    data,
                                    mime_type,
                                    ..
                                } => {
                                    json!({
                                        "type": "image_url",
                                        "image_url": {
                                            "url": format!("data:{};base64,{}", mime_type, data)
                                        }
                                    })
                                }
                                crate::llm::provider::ContentPart::File {
                                    filename,
                                    file_id,
                                    file_url,
                                    ..
                                } => {
                                    let fallback = filename
                                        .clone()
                                        .or_else(|| file_id.clone())
                                        .or_else(|| file_url.clone())
                                        .unwrap_or_else(|| "attached file".to_string());
                                    json!({ "type": "text", "text": format!("[File input not directly supported: {}]", fallback) })
                                }
                            })
                            .collect();
                        message_map.insert("content".to_owned(), Value::Array(parts_json));
                    } else {
                        let text = message.content.as_text().into_owned();
                        message_map.insert("content".to_owned(), Value::String(text));
                    }
                }
            }

            if let Some(tool_calls) = &message.tool_calls {
                let serialized_calls = tool_calls
                    .iter()
                    .filter_map(|call| {
                        call.function.as_ref().map(|func| {
                            json!({
                                "id": &call.id,
                                "type": "function",
                                "function": {
                                    "name": &func.name,
                                    "arguments": &func.arguments
                                }
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                message_map.insert("tool_calls".to_owned(), Value::Array(serialized_calls));
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                message_map.insert(
                    "tool_call_id".to_owned(),
                    Value::String(tool_call_id.clone()),
                );
            }

            messages.push(Value::Object(message_map));
        }

        Ok(messages)
    }

    fn format_for_chat_completions(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut messages = self.serialize_messages_huggingface_chat(request)?;
        let is_glm = self.is_glm_model(&request.model);

        if let Some(system) = &request.system_prompt {
            let has_system = messages
                .first()
                .and_then(|m| m.get("role"))
                .and_then(|r| r.as_str())
                == Some("system");
            if !has_system {
                messages.insert(
                    0,
                    json!({
                        "role": "system",
                        "content": system
                    }),
                );
            }
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": request.stream,
        });

        if request.stream && request.tools.is_some() && is_glm {
            payload["tool_stream"] = json!(true);
        }

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized) = self.serialize_tools_huggingface(tools) {
                payload["tools"] = json!(serialized);

                if let Some(choice) = &request.tool_choice {
                    payload["tool_choice"] = choice.to_provider_format("openai");
                }
            }
        }

        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }

        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(top_p);
        }

        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        if let Some(effort) = request.reasoning_effort {
            use crate::config::models::Provider;
            use crate::llm::rig_adapter::reasoning_parameters_for;
            if let Some(reasoning_params) = reasoning_parameters_for(Provider::HuggingFace, effort)
            {
                if let Some(params_obj) = reasoning_params.as_object() {
                    for (k, v) in params_obj {
                        payload[k] = v.clone();
                    }
                }
            }
        }

        if request.output_format.is_some() && !is_glm {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        Ok(payload)
    }

    fn is_glm_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("glm")
    }

    fn is_deepseek_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("deepseek")
    }

    fn is_minimax_model(&self, model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        lower.contains("minimax")
    }

    fn apply_model_defaults(&self, request: &mut LLMRequest) {
        if self.is_minimax_model(&request.model) {
            if request.temperature.is_none() {
                request.temperature = Some(1.0);
            }
            if request.top_p.is_none() {
                request.top_p = Some(0.95);
            }
            if request.top_k.is_none() {
                request.top_k = Some(40);
            }
        }
    }

    fn add_json_instruction(&self, payload: &mut Value) -> Result<(), LLMError> {
        if let Some(instructions) = payload.get_mut("instructions") {
            if let Some(text) = instructions.as_str() {
                if !text.contains("Return JSON") {
                    *instructions = json!(format!("{}\n\n{}", text, JSON_INSTRUCTION));
                }
            }
        } else {
            payload["instructions"] = json!(JSON_INSTRUCTION);
        }

        Ok(())
    }

    fn format_for_responses_api(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut input = Vec::new();

        for msg in &request.messages {
            let convert_parts = |parts: &[crate::llm::provider::ContentPart]| -> Value {
                let parts_json: Vec<Value> = parts
                    .iter()
                    .map(|part| match part {
                        crate::llm::provider::ContentPart::Text { text } => {
                            json!({ "type": "input_text", "text": text })
                        }
                        crate::llm::provider::ContentPart::Image {
                            data, mime_type, ..
                        } => {
                            json!({
                                "type": "input_image",
                                "image_url": format!("data:{};base64,{}", mime_type, data)
                            })
                        }
                        crate::llm::provider::ContentPart::File {
                            filename,
                            file_id,
                            file_url,
                            ..
                        } => {
                            let fallback = filename
                                .clone()
                                .or_else(|| file_id.clone())
                                .or_else(|| file_url.clone())
                                .unwrap_or_else(|| "attached file".to_string());
                            json!({
                                "type": "input_text",
                                "text": format!("[File input not directly supported: {}]", fallback)
                            })
                        }
                    })
                    .collect();
                json!(parts_json)
            };

            match msg.role {
                MessageRole::System | MessageRole::User => {
                    if msg.role == MessageRole::System && request.system_prompt.is_some() {
                        if let crate::llm::provider::MessageContent::Text(text) = &msg.content {
                            if request.system_prompt.as_ref().map(|s| s.as_str())
                                == Some(text.as_str())
                            {
                                continue;
                            }
                        }
                    }

                    let role = if msg.role == MessageRole::System {
                        "system"
                    } else {
                        "user"
                    };

                    let mut message_obj = json!({
                        "type": "message",
                        "role": role,
                    });

                    match &msg.content {
                        crate::llm::provider::MessageContent::Text(text) => {
                            message_obj["content"] = json!(text);
                        }
                        crate::llm::provider::MessageContent::Parts(parts) => {
                            message_obj["content"] = convert_parts(parts);
                        }
                    }

                    input.push(message_obj);
                }
                MessageRole::Assistant => {
                    let has_content = match &msg.content {
                        crate::llm::provider::MessageContent::Text(text) => !text.is_empty(),
                        crate::llm::provider::MessageContent::Parts(parts) => !parts.is_empty(),
                    };

                    if has_content {
                        let mut message_obj = json!({
                            "type": "message",
                            "role": "assistant",
                        });

                        match &msg.content {
                            crate::llm::provider::MessageContent::Text(text) => {
                                message_obj["content"] = json!(text);
                            }
                            crate::llm::provider::MessageContent::Parts(parts) => {
                                message_obj["content"] = convert_parts(parts);
                            }
                        }

                        input.push(message_obj);
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            if let Some(func) = &tc.function {
                                input.push(json!({
                                    "type": "function_call",
                                    "call_id": tc.id,
                                    "name": func.name,
                                    "arguments": func.arguments
                                }));
                            }
                        }
                    }
                }
                MessageRole::Tool => {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": msg.tool_call_id.clone().unwrap_or_default(),
                        "output": match &msg.content {
                            crate::llm::provider::MessageContent::Text(text) => text.clone(),
                            crate::llm::provider::MessageContent::Parts(parts) => {
                                parts.iter().filter_map(|p| match p {
                                    crate::llm::provider::ContentPart::Text { text } => Some(text.as_str()),
                                    _ => None
                                }).collect::<Vec<_>>().join("")
                            }
                        }
                    }));
                }
            }
        }

        let mut payload = json!({
            "model": request.model,
            "input": input,
            "stream": request.stream,
        });

        if let Some(system_prompt) = &request.system_prompt {
            payload["instructions"] = json!(system_prompt);
        }

        if let Some(effort) = request.reasoning_effort {
            use crate::config::types::ReasoningEffortLevel;
            if effort != ReasoningEffortLevel::None {
                payload["reasoning"] = json!({ "effort": effort.as_str() });
            }
        }

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(top_p);
        }
        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        if let Some(tools) = &request.tools {
            if let Some(serialized) = self.serialize_tools_huggingface(tools) {
                payload["tools"] = json!(serialized);

                if let Some(choice) = &request.tool_choice {
                    payload["tool_choice"] = choice.to_provider_format("openai");
                }
            }
        }

        if request.output_format.is_some() || request.tools.is_some() {
            self.add_json_instruction(&mut payload)?;
        }

        if request.output_format.is_some() && !self.is_glm_model(&request.model) {
            payload["response_format"] = json!({ "type": "json_object" });
        }

        Ok(payload)
    }

    fn should_use_responses_api(&self, _request: &LLMRequest) -> bool {
        false
    }

    fn format_error(&self, status: StatusCode, body: &str) -> LLMError {
        let message = format!("HuggingFace API error ({}): {}", status, body);

        LLMError::Provider {
            message: format_llm_error(PROVIDER_NAME, &message),
            metadata: Some(LLMErrorMetadata::new(
                PROVIDER_NAME,
                Some(status.as_u16()),
                None,
                None,
                None,
                None,
                Some(body.to_string()),
            )),
        }
    }

    fn parse_responses_api_format(json: &Value, model: String) -> Result<LLMResponse, LLMError> {
        let convenience_text = json.get("output_text").and_then(|t| t.as_str());

        let json_obj = if json.get("response").is_some() {
            json.get("response").unwrap()
        } else {
            json
        };

        let output = json_obj.get("output").and_then(|v| v.as_array());

        let output_arr = match output {
            Some(arr) => arr,
            None => {
                if let Some(text) = convenience_text {
                    return Ok(LLMResponse {
                        content: Some(text.to_string()),
                        tool_calls: None,
                        model,
                        usage: None,
                        finish_reason: crate::llm::provider::FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        tool_references: Vec::new(),
                        request_id: None,
                        organization_id: None,
                    });
                }

                return Err(LLMError::Provider {
                    message: format_llm_error(PROVIDER_NAME, "Not a Responses API format"),
                    metadata: None,
                });
            }
        };

        let mut content_fragments: Vec<String> = Vec::new();
        let mut reasoning_fragments: Vec<String> = Vec::new();
        let mut tool_calls: Vec<crate::llm::provider::ToolCall> = Vec::new();

        for item in output_arr {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                        for entry in content_arr {
                            let entry_type =
                                entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match entry_type {
                                "text" | "output_text" => {
                                    if let Some(text) = entry.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            content_fragments.push(text.to_string());
                                        }
                                    }
                                }
                                "reasoning" => {
                                    if let Some(text) = entry.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            reasoning_fragments.push(text.to_string());
                                        }
                                    }
                                }
                                "function_call" | "tool_call" => {
                                    if let Some(call) = Self::parse_responses_tool_call(entry) {
                                        tool_calls.push(call);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "function_call" | "tool_call" => {
                    if let Some(call) = Self::parse_responses_tool_call(item) {
                        tool_calls.push(call);
                    }
                }
                "reasoning" => {
                    if let Some(summary_arr) = item.get("summary").and_then(|s| s.as_array()) {
                        for summary in summary_arr {
                            if let Some(text) = summary.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    reasoning_fragments.push(text.to_string());
                                }
                            }
                        }
                    } else if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        reasoning_fragments.push(text.to_string());
                    }
                }
                _ => {}
            }
        }

        let content = if content_fragments.is_empty() {
            convenience_text.map(|t| t.to_string())
        } else {
            Some(content_fragments.join(""))
        };

        let reasoning = if reasoning_fragments.is_empty() {
            None
        } else {
            Some(reasoning_fragments.join("\n\n"))
        };

        let finish_reason = if !tool_calls.is_empty() {
            crate::llm::provider::FinishReason::ToolCalls
        } else {
            crate::llm::provider::FinishReason::Stop
        };

        let usage_value = json.get("usage").or_else(|| json_obj.get("usage"));
        let usage = usage_value.map(|usage_value| crate::llm::provider::Usage {
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
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        Ok(LLMResponse {
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            model,
            usage,
            finish_reason,
            reasoning,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }

    fn parse_responses_tool_call(item: &Value) -> Option<crate::llm::provider::ToolCall> {
        let call_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let function_obj = item.get("function").and_then(|v| v.as_object());
        let name = function_obj.and_then(|f| f.get("name").and_then(|n| n.as_str()))?;
        let arguments = function_obj.and_then(|f| f.get("arguments"));

        let serialized = arguments.map_or("{}".to_owned(), |args| {
            if args.is_string() {
                args.as_str().unwrap_or("{}").to_string()
            } else {
                args.to_string()
            }
        });

        Some(crate::llm::provider::ToolCall::function(
            call_id.to_string(),
            name.to_string(),
            serialized,
        ))
    }

    async fn parse_response(
        &self,
        response: Response,
        model: String,
        use_responses_api: bool,
    ) -> Result<LLMResponse, LLMError> {
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(self.format_error(status, &body));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|err| format_parse_error(PROVIDER_NAME, &err))?;

        if use_responses_api {
            if json.get("output").is_some() {
                return Self::parse_responses_api_format(&json, model);
            }
        }

        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            json,
            PROVIDER_NAME,
            model,
            false,
            None,
        )
    }

    pub fn available_models() -> Vec<String> {
        models::huggingface::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn get_endpoint(&self, use_responses_api: bool) -> String {
        let base = self.base_url.trim_end_matches('/');
        if use_responses_api {
            format!("{}/responses", base)
        } else {
            format!("{}/chat/completions", base)
        }
    }
}

#[async_trait]
impl LLMProvider for HuggingFaceProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        models::huggingface::REASONING_MODELS.contains(&model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Same robustness logic for reasoning effort
        self.is_glm_model(model)
            || self.is_deepseek_model(model)
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        false
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_context_caching(&self, _model: &str) -> bool {
        false
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        128_000
    }

    async fn count_prompt_tokens_exact(
        &self,
        request: &LLMRequest,
    ) -> Result<Option<u32>, LLMError> {
        let mut request = request.clone();
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.apply_model_defaults(&mut request);
        self.validate_request(&request)?;
        request.model = self.normalize_model_id(&request.model)?;

        let mut payload = self.format_for_responses_api(&request)?;
        strip_generation_controls_for_token_count(&mut payload);

        let endpoint = format!(
            "{}/responses/input_tokens",
            self.base_url.trim_end_matches('/')
        );

        let value = execute_token_count_request(
            self.http_client
                .post(&endpoint)
                .header("Authorization", format!("Bearer {}", self.api_key)),
            &payload,
            PROVIDER_NAME,
        )
        .await?;

        let Some(value) = value else {
            return Ok(None);
        };

        Ok(parse_prompt_tokens_from_count_response(&value))
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        self.apply_model_defaults(&mut request);
        self.validate_request(&request)?;

        let model_id = self.normalize_model_id(&request.model)?;
        request.model = model_id;

        let use_responses_api = self.should_use_responses_api(&request);
        let payload = if use_responses_api {
            self.format_for_responses_api(&request)?
        } else {
            self.format_for_chat_completions(&request)?
        };

        let endpoint = self.get_endpoint(use_responses_api);

        let response = self
            .http_client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|err| format_network_error(PROVIDER_NAME, &err))?;

        self.parse_response(response, model, use_responses_api)
            .await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        self.apply_model_defaults(&mut request);
        self.validate_request(&request)?;
        request.stream = true;

        let model_id = self.normalize_model_id(&request.model)?;
        request.model = model_id;

        let use_responses_api = self.should_use_responses_api(&request);
        let payload = if use_responses_api {
            self.format_for_responses_api(&request)?
        } else {
            self.format_for_chat_completions(&request)?
        };

        let endpoint = self.get_endpoint(use_responses_api);

        let response = self
            .http_client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|err| format_network_error(PROVIDER_NAME, &err))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(self.format_error(status, &body));
        }

        self.create_stream(response, model, use_responses_api).await
    }

    fn supported_models(&self) -> Vec<String> {
        Self::available_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            return Err(LLMError::InvalidRequest {
                message: format_llm_error(PROVIDER_NAME, "Messages cannot be empty"),
                metadata: None,
            });
        }

        if request.model.trim().is_empty() {
            return Err(LLMError::InvalidRequest {
                message: format_llm_error(PROVIDER_NAME, "Model identifier cannot be empty"),
                metadata: None,
            });
        }

        Ok(())
    }
}

impl HuggingFaceProvider {
    async fn create_stream(
        &self,
        response: Response,
        model: String,
        use_responses_api: bool,
    ) -> Result<LLMStream, LLMError> {
        let mut bytes_stream = response.bytes_stream();
        let mut buffer = String::with_capacity(4096);
        let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model.clone());
        let telemetry = NoopStreamTelemetry;

        let stream = try_stream! {
            'outer: while let Some(chunk_result) = bytes_stream.next().await {
                let chunk = chunk_result.map_err(|err| format_network_error(PROVIDER_NAME, &err))?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                if buffer.len() > 128_000 {
                    Err(LLMError::Provider {
                        message: format_llm_error(PROVIDER_NAME, "Stream buffer exceeded maximum size (128KB)"),
                        metadata: None,
                    })?;
                }

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer.drain(..=newline_pos);

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    let data = if let Some(stripped) = line.strip_prefix("data: ") {
                        stripped
                    } else {
                        continue;
                    };

                    if data == "[DONE]" {
                        break 'outer;
                    }

                    let event: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if use_responses_api {
                        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                        match event_type {
                            "response.output_text.delta" | "output_text.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                                    telemetry.on_content_delta(delta);
                                    for ev in aggregator.handle_content(delta) {
                                        yield ev;
                                    }
                                }
                                continue;
                            }
                            "response.reasoning.delta" | "reasoning.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                                    if let Some(d) = aggregator.handle_reasoning(delta) {
                                        telemetry.on_reasoning_delta(&d);
                                        yield LLMStreamEvent::Reasoning { delta: d };
                                    }
                                }
                                continue;
                            }
                            "response.function_call_arguments.delta" | "tool_call.delta" => {
                                telemetry.on_tool_call_delta();
                                continue;
                            }
                            "response.completed" => {
                                if let Some(response_obj) = event.get("response") {
                                    if let Ok(response) = Self::parse_responses_api_format(response_obj, model.clone()) {
                                        let final_agg_response = aggregator.finalize();
                                        let mut merged_response = response;
                                        if merged_response.content.is_none() {
                                            merged_response.content = final_agg_response.content;
                                        }
                                        if merged_response.reasoning.is_none() {
                                            merged_response.reasoning = final_agg_response.reasoning;
                                        }
                                        if merged_response.tool_calls.is_none() {
                                            merged_response.tool_calls = final_agg_response.tool_calls;
                                        }
                                        if merged_response.usage.is_none() {
                                            merged_response.usage = final_agg_response.usage;
                                        }
                                        yield LLMStreamEvent::Completed { response: Box::new(merged_response) };
                                        return;
                                    }
                                }
                                break 'outer;
                            }
                            "response.done" => {
                                break 'outer;
                            }
                            _ => {}
                        }
                    }

                    if let Some(choices_arr) = event.get("choices").and_then(|c| c.as_array()) {
                        if let Some(choice) = choices_arr.first() {
                            if let Some(delta_obj) = choice.get("delta") {
                                if let Some(content) = delta_obj.get("content").and_then(|c| c.as_str()) {
                                    telemetry.on_content_delta(content);
                                    for ev in aggregator.handle_content(content) {
                                        yield ev;
                                    }
                                }

                                if let Some(reason) = delta_obj.get("reasoning_content").and_then(|r| r.as_str()) {
                                    if let Some(d) = aggregator.handle_reasoning(reason) {
                                        telemetry.on_reasoning_delta(&d);
                                        yield LLMStreamEvent::Reasoning { delta: d };
                                    }
                                }

                                if let Some(tool_calls_arr) = delta_obj.get("tool_calls").and_then(|tc| tc.as_array()) {
                                    aggregator.handle_tool_calls(tool_calls_arr);
                                    telemetry.on_tool_call_delta();
                                }
                            }

                            if let Some(finish_reason_str) = choice.get("finish_reason").and_then(|fr| fr.as_str()) {
                                aggregator.set_finish_reason(map_finish_reason_common(finish_reason_str));
                                if let Some(usage_value) = event.get("usage") {
                                    aggregator.set_usage(crate::llm::provider::Usage {
                                        prompt_tokens: usage_value.get("prompt_tokens").and_then(|pt| pt.as_u64()).unwrap_or(0) as u32,
                                        completion_tokens: usage_value.get("completion_tokens").and_then(|ct| ct.as_u64()).unwrap_or(0) as u32,
                                        total_tokens: usage_value.get("total_tokens").and_then(|tt| tt.as_u64()).unwrap_or(0) as u32,
                                        cached_prompt_tokens: None,
                                        cache_creation_tokens: None,
                                        cache_read_tokens: None,
                                    });
                                }

                                break 'outer;
                            }
                        }
                    }
                }
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl LLMClient for HuggingFaceProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::HuggingFace
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_request(model: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![crate::llm::provider::Message::user("hello".to_string())],
            model: model.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn exact_count_uses_huggingface_input_tokens_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "input_tokens": 123
            })))
            .mount(&server)
            .await;

        let provider = HuggingFaceProvider::from_config(
            Some("key".to_string()),
            Some(models::huggingface::DEFAULT_MODEL.to_string()),
            Some(format!("{}/v1", server.uri())),
            None,
            None,
            None,
            None,
        );

        let count = provider
            .count_prompt_tokens_exact(&sample_request(models::huggingface::DEFAULT_MODEL))
            .await
            .expect("count should succeed");
        assert_eq!(count, Some(123));
    }

    #[tokio::test]
    async fn exact_count_accepts_usage_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "usage": { "input_tokens": 88 }
            })))
            .mount(&server)
            .await;

        let provider = HuggingFaceProvider::from_config(
            Some("key".to_string()),
            Some(models::huggingface::DEFAULT_MODEL.to_string()),
            Some(format!("{}/v1", server.uri())),
            None,
            None,
            None,
            None,
        );

        let count = provider
            .count_prompt_tokens_exact(&sample_request(models::huggingface::DEFAULT_MODEL))
            .await
            .expect("count should succeed");
        assert_eq!(count, Some(88));
    }

    #[tokio::test]
    async fn exact_count_returns_none_when_endpoint_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let provider = HuggingFaceProvider::from_config(
            Some("key".to_string()),
            Some(models::huggingface::DEFAULT_MODEL.to_string()),
            Some(format!("{}/v1", server.uri())),
            None,
            None,
            None,
            None,
        );

        let count = provider
            .count_prompt_tokens_exact(&sample_request(models::huggingface::DEFAULT_MODEL))
            .await
            .expect("count should succeed");
        assert_eq!(count, None);
    }
}
