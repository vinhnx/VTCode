#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, ToolCall,
};
use crate::llm::providers::common::serialize_message_content_openai;
use crate::llm::providers::shared::parse_compacted_output_messages;
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
    fn output_item_to_value(item: crate::open_responses::OutputItem) -> Result<Value, LLMError> {
        serde_json::to_value(item).map_err(|e| LLMError::Provider {
            message: format!("Failed to serialize Open Responses input item: {e}"),
            metadata: None,
        })
    }

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

    fn responses_compact_url(&self) -> String {
        format!("{}/responses/compact", self.base_url.trim_end_matches('/'))
    }

    fn supports_compaction_endpoint(&self) -> bool {
        self.base_url.contains("api.openai.com") || self.base_url.contains("api.openresponses.com")
    }

    async fn compact_history_request(
        &self,
        model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        let resolved_model = if model.trim().is_empty() {
            self.model.clone()
        } else {
            model.trim().to_string()
        };
        let request = LLMRequest {
            model: resolved_model.clone(),
            messages: history.to_vec(),
            ..Default::default()
        };
        let native_payload = self.build_native_payload(&request, false)?;
        let input = native_payload
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let compact_payload = json!({
            "model": resolved_model,
            "input": input,
        });

        let response = self
            .http_client
            .post(self.responses_compact_url())
            .bearer_auth(&self.api_key)
            .json(&compact_payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("Compaction endpoint error (HTTP {}): {}", status, body),
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
        let output = json
            .get("output")
            .and_then(|value| value.as_array())
            .ok_or_else(|| LLMError::Provider {
                message:
                    "Invalid response from OpenResponses compact endpoint: missing output array"
                        .to_string(),
                metadata: None,
            })?;

        let compacted = parse_compacted_output_messages(output);
        if compacted.is_empty() {
            return Err(LLMError::Provider {
                message: "Compaction response contained no reusable messages".to_string(),
                metadata: None,
            });
        }

        Ok(compacted)
    }

    fn build_native_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        use crate::open_responses::{
            ContentPart, ImageDetail, InputFileContent, InputImageContent, MessageRole, OutputItem,
            Request,
        };

        let mut input: Vec<Value> = Vec::new();

        if let Some(system) = &request.system_prompt {
            input.push(Self::output_item_to_value(OutputItem::completed_message(
                "msg_system",
                MessageRole::System,
                vec![ContentPart::input_text(system.as_str())],
            ))?);
        }

        for (i, message) in request.messages.iter().enumerate() {
            if let Some(reasoning_details) = &message.reasoning_details {
                for item in reasoning_details {
                    input.push(item.clone());
                }
            }

            let role = match message.role.as_generic_str() {
                "user" => Some(MessageRole::User),
                "assistant" => Some(MessageRole::Assistant),
                "system" => Some(MessageRole::System),
                // Tool responses are represented by function_call_output items below.
                "tool" => None,
                _ => Some(MessageRole::User),
            };

            if let Some(role) = role {
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
                                    data,
                                    mime_type,
                                    ..
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
                    let content_text = message.content.as_text();
                    if !content_text.trim().is_empty() {
                        content.push(ContentPart::input_text(content_text.to_string()));
                    }
                }
                if !content.is_empty() {
                    input.push(Self::output_item_to_value(OutputItem::completed_message(
                        id, role, content,
                    ))?);
                }
            }

            // Handle tool calls and outputs if present in message history
            if let Some(tool_calls) = &message.tool_calls {
                for (j, tc) in tool_calls.iter().enumerate() {
                    if let Some(f) = &tc.function {
                        input.push(Self::output_item_to_value(OutputItem::function_call(
                            format!("fc_{i}_{j}"),
                            &f.name,
                            serde_json::from_str(&f.arguments).unwrap_or(Value::Null),
                        ))?);
                    }
                }
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                // If this message is a tool output, add it as FunctionCallOutput
                input.push(Self::output_item_to_value(
                    OutputItem::completed_function_call_output(
                        format!("fco_{i}"),
                        Some(tool_call_id.clone()),
                        message.content.as_text(),
                    ),
                )?);
            }
        }

        let mut req = Request::new(&request.model, Vec::new());
        req.stream = stream;
        req.temperature = request.temperature.map(|t| t as f64);
        req.max_output_tokens = request.max_tokens.map(|t| t as u64);
        req.previous_response_id = request
            .previous_response_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        req.store = request.response_store;
        req.include = request.responses_include.as_ref().and_then(|fields| {
            let values: Vec<String> = fields
                .iter()
                .map(|field| field.trim())
                .filter(|field| !field.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(values)
            }
        });

        if let Some(tools) = &request.tools {
            req.tools = Some((**tools).clone());
        }

        let mut payload = serde_json::to_value(req).map_err(|e| LLMError::Provider {
            message: format!("Failed to serialize Open Responses request: {e}"),
            metadata: None,
        })?;
        if let Some(map) = payload.as_object_mut() {
            map.insert("input".to_string(), Value::Array(input));
        }

        if let Some(context_management) = &request.context_management
            && let Some(map) = payload.as_object_mut()
        {
            map.insert("context_management".to_string(), context_management.clone());
        }

        Ok(payload)
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

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        self.supports_compaction_endpoint()
    }

    async fn compact_history(
        &self,
        model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        if !self.supports_compaction_endpoint() {
            return Err(LLMError::Provider {
                message:
                    "OpenResponses compact endpoint is not supported for this configured base URL"
                        .to_string(),
                metadata: None,
            });
        }

        self.compact_history_request(model, history).await
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

        // Fallback: Extract reasoning from content tags if not provided natively
        // Handles <think></think>, <thought></thought>, <reasoning></reasoning>, <analysis></analysis>
        let (final_reasoning, final_content) = if reasoning.is_none() && !content.is_empty() {
            let (reasoning_parts, cleaned_content) =
                crate::llm::utils::extract_reasoning_content(&content);
            if reasoning_parts.is_empty() {
                (None, Some(content))
            } else {
                (
                    Some(reasoning_parts.join("\n\n")),
                    cleaned_content.or(Some(content)),
                )
            }
        } else {
            (reasoning, Some(content))
        };

        let finish_reason = match json.get("status").and_then(|s| s.as_str()) {
            Some("completed") => FinishReason::Stop,
            Some("incomplete") => FinishReason::Length,
            _ => FinishReason::Stop,
        };

        Ok(LLMResponse {
            content: final_content.filter(|c| !c.is_empty()),
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            model,
            usage: None,
            finish_reason,
            reasoning: final_reasoning,
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
                                        // Use aggregator's sanitizer to extract reasoning tags from content
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
                                    // Legacy/simple reasoning event
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                "response.reasoning_content.delta" => {
                                    // Raw reasoning traces (preferred)
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                        yield LLMStreamEvent::Reasoning { delta: delta.to_string() };
                                    }
                                }
                                "response.reasoning_summary_text.delta" => {
                                    // Summary reasoning (fallback when raw not available)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider(base_url: &str) -> OpenResponsesProvider {
        let http_client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client should build");
        OpenResponsesProvider::new_with_client(
            String::new(),
            "gpt-5".to_string(),
            http_client,
            base_url.to_string(),
            TimeoutsConfig::default(),
        )
    }

    #[test]
    fn native_payload_includes_responses_continuity_fields() {
        let provider = test_provider("https://api.openresponses.com/v1");
        let mut request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };
        request.previous_response_id = Some("resp_prev_1".to_string());
        request.response_store = Some(false);
        request.responses_include = Some(vec![
            "reasoning.encrypted_content".to_string(),
            "output_text.annotations".to_string(),
        ]);

        let payload = provider
            .build_native_payload(&request, false)
            .expect("native payload should serialize");

        assert_eq!(
            payload.get("previous_response_id").and_then(Value::as_str),
            Some("resp_prev_1")
        );
        assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
        let include = payload
            .get("include")
            .and_then(Value::as_array)
            .expect("include must exist");
        assert_eq!(include.len(), 2);
    }

    #[test]
    fn native_payload_includes_context_management() {
        let provider = test_provider("https://api.openresponses.com/v1");
        let mut request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        };
        request.context_management = Some(serde_json::json!([{
            "type": "compaction",
            "compact_threshold": 200000
        }]));

        let payload = provider
            .build_native_payload(&request, false)
            .expect("native payload should serialize");
        let management = payload
            .get("context_management")
            .and_then(Value::as_array)
            .expect("context management should exist");
        assert_eq!(management.len(), 1);
    }

    #[test]
    fn openresponses_provider_reports_compaction_support() {
        let provider = test_provider("https://api.openresponses.com/v1");
        assert!(provider.supports_responses_compaction("gpt-5"));
    }

    #[test]
    fn openresponses_provider_disables_compaction_for_unknown_endpoint() {
        let provider = test_provider("https://api.example.com/v1");
        assert!(!provider.supports_responses_compaction("gpt-5"));
    }

    #[test]
    fn native_payload_preserves_opaque_reasoning_details_items() {
        let provider = test_provider("https://api.openresponses.com/v1");
        let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
            "type": "compaction",
            "id": "cmp_1",
            "status": "completed",
            "encrypted_content": "opaque_state"
        })]));
        let request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![message],
            ..Default::default()
        };

        let payload = provider
            .build_native_payload(&request, false)
            .expect("native payload should serialize");
        let input = payload
            .get("input")
            .and_then(Value::as_array)
            .expect("input should be an array");

        assert_eq!(input.len(), 1);
        assert_eq!(
            input[0].get("type").and_then(Value::as_str),
            Some("compaction")
        );
        assert_eq!(
            input[0].get("encrypted_content").and_then(Value::as_str),
            Some("opaque_state")
        );
    }

    #[test]
    fn native_payload_emits_tool_response_only_as_function_call_output() {
        let provider = test_provider("https://api.openresponses.com/v1");
        let request = LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                Message::assistant_with_tools(
                    String::new(),
                    vec![ToolCall::function(
                        "call_1".to_string(),
                        "shell".to_string(),
                        "{\"command\":\"pwd\"}".to_string(),
                    )],
                ),
                Message::tool_response("call_1".to_string(), "/tmp/work".to_string()),
            ],
            ..Default::default()
        };

        let payload = provider
            .build_native_payload(&request, false)
            .expect("native payload should serialize");
        let input = payload
            .get("input")
            .and_then(Value::as_array)
            .expect("input should be an array");

        assert!(input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        }));
        assert!(!input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("message")
                && item.get("role").and_then(Value::as_str) == Some("user")
                && item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|part| part.get("text").and_then(Value::as_str) == Some("/tmp/work"))
        }));
    }
}
