#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    ToolCall,
};
use crate::llm::providers::common::serialize_message_content_openai;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};

use super::super::common::{override_base_url, resolve_model};
use super::super::error_handling::{format_network_error, format_parse_error};

pub struct OpenResponsesProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: String,
    model_behavior: Option<ModelConfig>,
}

impl OpenResponsesProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::openresponses::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(model, None, api_key, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
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
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let resolved_model = resolve_model(model, models::openresponses::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key_value, model_behavior)
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: String,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        Self {
            http_client: HttpClient::new(),
            base_url: override_base_url(
                urls::OPENRESPONSES_API_BASE,
                base_url,
                Some(env_vars::OPENRESPONSES_BASE_URL),
            ),
            model,
            api_key,
            model_behavior,
        }
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url.trim_end_matches('/'))
    }

    fn build_native_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        use crate::open_responses::{
            ContentPart, ImageDetail, InputFileContent, InputImageContent, MessageRole, OutputItem,
            Request,
        };

        let mut input = Vec::new();

        if let Some(system) = &request.system_prompt {
            input.push(OutputItem::completed_message(
                "msg_system",
                MessageRole::System,
                vec![ContentPart::input_text(system.as_str())],
            ));
        }

        for (i, message) in request.messages.iter().enumerate() {
            let role = match message.role.as_generic_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                _ => MessageRole::User,
            };

            let id = format!("msg_{i}");
            let mut content = Vec::new();
            match &message.content {
                crate::llm::provider::MessageContent::Text(text) => {
                    if !text.trim().is_empty() {
                        content.push(ContentPart::input_text(text.as_str()));
                    }
                }
                crate::llm::provider::MessageContent::Parts(parts) => {
                    for part in parts {
                        match part {
                            crate::llm::provider::ContentPart::Text { text } => {
                                if !text.trim().is_empty() {
                                    content.push(ContentPart::input_text(text.as_str()));
                                }
                            }
                            crate::llm::provider::ContentPart::Image {
                                data, mime_type, ..
                            } => {
                                content.push(ContentPart::InputImage(InputImageContent {
                                    image_url: format!("data:{};base64,{}", mime_type, data),
                                    detail: ImageDetail::Auto,
                                }));
                            }
                            crate::llm::provider::ContentPart::File {
                                filename,
                                file_id,
                                file_data,
                                file_url,
                                ..
                            } => {
                                content.push(ContentPart::InputFile(InputFileContent {
                                    filename: filename.clone(),
                                    file_id: file_id.clone(),
                                    file_data: file_data.clone(),
                                    file_url: file_url.clone(),
                                }));
                            }
                        }
                    }
                }
            }
            if content.is_empty() {
                content.push(ContentPart::input_text(message.content.as_text()));
            }

            input.push(OutputItem::completed_message(id, role, content));

            // Handle tool calls and outputs if present in message history
            if let Some(tool_calls) = &message.tool_calls {
                for (j, tc) in tool_calls.iter().enumerate() {
                    if let Some(f) = &tc.function {
                        input.push(OutputItem::function_call(
                            format!("fc_{i}_{j}"),
                            &f.name,
                            serde_json::from_str(&f.arguments).unwrap_or(Value::Null),
                        ));
                    }
                }
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                // If this message is a tool output, add it as FunctionCallOutput
                input.push(OutputItem::completed_function_call_output(
                    format!("fco_{i}"),
                    Some(tool_call_id.clone()),
                    message.content.as_text(),
                ));
            }
        }

        let mut req = Request::new(&request.model, input);
        req.stream = stream;
        req.temperature = request.temperature.map(|t| t as f64);
        req.max_output_tokens = request.max_tokens.map(|t| t as u64);

        if let Some(tools) = &request.tools {
            req.tools = Some((**tools).clone());
        }

        serde_json::to_value(req).map_err(|e| LLMError::Provider {
            message: format!("Failed to serialize Open Responses request: {e}"),
            metadata: None,
        })
    }

    fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for message in &request.messages {
            let role = message.role.as_generic_str();
            let mut message_obj = json!({
                "role": role,
                "content": serialize_message_content_openai(&message.content)
            });

            if let Some(tool_calls) = &message.tool_calls {
                let tool_calls_json: Vec<Value> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tc.function.as_ref().map(|f| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": f.name,
                                    "arguments": f.arguments
                                }
                            })
                        })
                    })
                    .collect();
                message_obj["tool_calls"] = json!(tool_calls_json);
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                message_obj["tool_call_id"] = json!(tool_call_id);
            }

            messages.push(message_obj);
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": stream
        });

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            payload["temperature"] = json!(temp);
        }

        if let Some(tools) = &request.tools {
            let tools_json: Vec<Value> = tools
                .iter()
                .filter_map(|t| {
                    t.function.as_ref().map(|f| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": f.name,
                                "description": f.description,
                                "parameters": f.parameters
                            }
                        })
                    })
                })
                .collect();
            payload["tools"] = json!(tools_json);
        }

        Ok(payload)
    }

    async fn generate_fallback(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("HTTP {}: {}", status, body),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("OpenResponses", &e))?;

        let choice = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from OpenResponses: missing choices".to_string(),
                metadata: None,
            })?;

        let message = choice.get("message").ok_or_else(|| LLMError::Provider {
            message: "Invalid response from OpenResponses: missing message".to_string(),
            metadata: None,
        })?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

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
                        let arguments = function.get("arguments").and_then(|v| v.as_str())?;
                        Some(ToolCall::function(
                            id.to_string(),
                            name.to_string(),
                            arguments.to_string(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|calls| !calls.is_empty());

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|fr| fr.as_str())
            .map(|fr| match fr {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolCalls,
                other => FinishReason::Error(other.to_string()),
            })
            .unwrap_or(FinishReason::Stop);

        Ok(LLMResponse {
            content,
            tool_calls,
            model,
            usage: None,
            finish_reason,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: json
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            organization_id: None,
        })
    }

    async fn stream_fallback(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("HTTP {}: {}", status, body),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model);

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("OpenResponses", &e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = crate::llm::providers::shared::find_sse_boundary(&buffer) {
                    let event = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = crate::llm::providers::shared::extract_data_payload(&event) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(payload) = serde_json::from_str::<Value>(trimmed)
                            && let Some(choices) = payload.get("choices").and_then(|v| v.as_array())
                                && let Some(choice) = choices.first()
                                    && let Some(delta) = choice.get("delta") {
                                        if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                            for ev in aggregator.handle_content(content) {
                                                yield ev;
                                            }
                                        }

                                        if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
                                            aggregator.handle_tool_calls(tool_calls);
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
impl LLMProvider for OpenResponsesProvider {
    fn name(&self) -> &str {
        "openresponses"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(true) // Open Responses usually implies reasoning support
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(true)
    }

    fn supported_models(&self) -> Vec<String> {
        use crate::config::constants::models::openresponses::SUPPORTED_MODELS;
        SUPPORTED_MODELS.iter().map(|s| s.to_string()).collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.model.is_empty() {
            return Err(LLMError::Provider {
                message: "Model is required for OpenResponses provider".to_string(),
                metadata: None,
            });
        }

        let supported = self.supported_models();
        if !supported.contains(&request.model) {
            return Err(LLMError::Provider {
                message: format!(
                    "Model '{}' is not supported by OpenResponses provider. Supported models: {}",
                    request.model,
                    supported.join(", ")
                ),
                metadata: None,
            });
        }

        Ok(())
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        // Try native Open Responses endpoint first
        let payload = self.build_native_payload(&request, false)?;
        let url = self.responses_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        // If native endpoint fails with 404, fallback to chat/completions
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return self.generate_fallback(request).await;
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("HTTP {}: {}", status, body),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("OpenResponses", &e))?;

        // Handle native Open Responses response structure
        let output = json
            .get("output")
            .and_then(|o| o.as_array())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from OpenResponses: missing output".to_string(),
                metadata: None,
            })?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut reasoning = None;

        for item_val in output {
            let item_type = item_val.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(content_parts) = item_val.get("content").and_then(|c| c.as_array())
                    {
                        for part in content_parts {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                content.push_str(text);
                            }
                        }
                    }
                }
                "reasoning" => {
                    if let Some(text) = item_val.get("content").and_then(|t| t.as_str()) {
                        reasoning = Some(text.to_string());
                    }
                }
                "function_call" => {
                    let id = item_val
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = item_val
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = item_val
                        .get("arguments")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    tool_calls.push(ToolCall::function(id, name, arguments));
                }
                _ => {}
            }
        }

        let finish_reason = match json.get("status").and_then(|s| s.as_str()) {
            Some("completed") => FinishReason::Stop,
            Some("incomplete") => FinishReason::Length,
            _ => FinishReason::Stop,
        };

        Ok(LLMResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            model,
            usage: None,
            finish_reason,
            reasoning,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: json
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            organization_id: None,
        })
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.build_native_payload(&request, true)?;
        let url = self.responses_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return self.stream_fallback(request).await;
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("HTTP {}: {}", status, body),
            );
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model);

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("OpenResponses", &e))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some((split_idx, delimiter_len)) = crate::llm::providers::shared::find_sse_boundary(&buffer) {
                    let event_text = buffer[..split_idx].to_string();
                    buffer.drain(..split_idx + delimiter_len);

                    if let Some(data_payload) = crate::llm::providers::shared::extract_data_payload(&event_text) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(event) = serde_json::from_str::<Value>(trimmed) {
                            let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

                            match event_type {
                                "response.output_text.delta" => {
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        for ev in aggregator.handle_content(delta) {
                                            yield ev;
                                        }
                                    }
                                }
                                "response.function_call_arguments.delta" => {
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        let tc_json = json!([{
                                            "index": 0,
                                            "id": event.get("item_id"),
                                            "function": { "arguments": delta }
                                        }]);
                                        aggregator.handle_tool_calls(tc_json.as_array().unwrap());
                                    }
                                }
                                "response.reasoning.delta" => {
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                _ => {}
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
