//! OpenResponses provider implementation.
//!
//! This module provides a provider that can communicate with any server
//! implementing the OpenResponses specification.

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Value, json};
use tracing::debug;

use crate::config::TimeoutsConfig;
use crate::config::core::PromptCachingConfig;
use crate::llm::error_display;
#[cfg(test)]
use crate::llm::provider::Message;
use crate::llm::provider::{
    FinishReason, LLMError, LLMErrorMetadata, LLMProvider, LLMRequest, LLMResponse, LLMStream,
    LLMStreamEvent, MessageRole, ToolCall, Usage,
};

use super::streaming::{StreamAccumulator, parse_sse_event};

/// Default base URL for OpenResponses-compatible APIs.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Default OpenResponses version header value.
const DEFAULT_OPENRESPONSES_VERSION: &str = "latest";

/// Default model to use if none specified.
const DEFAULT_MODEL: &str = "gpt-4o";

/// OpenResponses provider for multi-provider LLM interactions.
///
/// This provider implements the OpenResponses specification and can
/// communicate with any server that implements the spec, including
/// OpenAI, Anthropic adapters, and other compatible backends.
pub struct OpenResponsesProvider {
    api_key: String,
    model: String,
    base_url: String,
    http_client: reqwest::Client,
    prompt_cache: Option<PromptCachingConfig>,
    version: String,
}

impl std::fmt::Debug for OpenResponsesProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenResponsesProvider")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl OpenResponsesProvider {
    /// Create a new OpenResponses provider with the given API key.
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    /// Create a new provider with a specific model.
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            base_url: DEFAULT_BASE_URL.to_string(),
            http_client: reqwest::Client::new(),
            prompt_cache: None,
            version: DEFAULT_OPENRESPONSES_VERSION.to_string(),
        }
    }

    /// Create a provider from configuration options.
    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<crate::config::core::AnthropicConfig>,
    ) -> Self {
        let api_key = api_key.unwrap_or_default();
        let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let base_url = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Self {
            api_key,
            model,
            base_url,
            http_client: reqwest::Client::new(),
            prompt_cache,
            version: DEFAULT_OPENRESPONSES_VERSION.to_string(),
        }
    }

    /// Set the base URL for the API.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Set the OpenResponses version header.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Get the responses endpoint URL.
    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url.trim_end_matches('/'))
    }

    /// Convert an LLMRequest to OpenResponses format.
    fn build_request_payload(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut input: Vec<Value> = Vec::new();
        let mut instructions: Option<String> = None;

        // Handle system prompt as instructions
        if let Some(ref system_prompt) = request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                instructions = Some(trimmed.to_string());
            }
        }

        // Convert messages to items
        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    // Append to instructions
                    let text = msg.content.as_text();
                    if !text.trim().is_empty() {
                        if let Some(ref mut inst) = instructions {
                            inst.push_str("\n\n");
                            inst.push_str(text.trim());
                        } else {
                            instructions = Some(text.trim().to_string());
                        }
                    }
                }
                MessageRole::User => {
                    // Handle multimodal content
                    let mut content_parts: Vec<Value> = Vec::new();

                    // For now, just handle text content - in a full implementation we'd handle images too
                    content_parts.push(json!({
                        "type": "input_text",
                        "text": msg.content.as_text()
                    }));

                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content_parts
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts: Vec<Value> = Vec::new();

                    // Add text content
                    if !msg.content.is_empty() {
                        content_parts.push(json!({
                            "type": "output_text",
                            "text": msg.content.as_text()
                        }));
                    }

                    // Add tool calls
                    if let Some(ref tool_calls) = msg.tool_calls {
                        for call in tool_calls {
                            if let Some(ref func) = call.function {
                                // Add as a separate function_call item
                                input.push(json!({
                                    "type": "function_call",
                                    "id": &call.id,
                                    "call_id": &call.id,
                                    "name": &func.name,
                                    "arguments": &func.arguments
                                }));
                            }
                        }
                    }

                    // Add reasoning items if present
                    if let Some(ref reasoning_details) = msg.reasoning_details {
                        for item in reasoning_details {
                            input.push(item.clone());
                        }
                    }

                    if !content_parts.is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": content_parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let call_id =
                        msg.tool_call_id
                            .as_ref()
                            .ok_or_else(|| LLMError::InvalidRequest {
                                message: "Tool messages must include tool_call_id".to_string(),
                                metadata: None,
                            })?;

                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": msg.content.as_text()
                    }));
                }
            }
        }

        // Build the request payload
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };

        let mut payload = json!({
            "model": model,
        });

        // Add input array if we have messages
        if !input.is_empty() {
            payload["input"] = json!(input);
        }

        // Add instructions if present
        if let Some(inst) = instructions {
            payload["instructions"] = json!(inst);
        }

        // Add tools if present
        if let Some(ref tools) = request.tools {
            let tool_params: Vec<Value> = tools
                .iter()
                .filter_map(|t| {
                    t.function.as_ref().map(|f| {
                        json!({
                            "type": "function",
                            "name": &f.name,
                            "description": &f.description,
                            "parameters": &f.parameters
                        })
                    })
                })
                .collect();

            if !tool_params.is_empty() {
                payload["tools"] = json!(tool_params);
            }
        }

        // Add optional parameters
        if let Some(max_tokens) = request.max_tokens {
            payload["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            payload["temperature"] = json!(temperature);
        }

        if let Some(top_p) = request.top_p {
            payload["top_p"] = json!(top_p);
        }

        // Add reasoning effort if specified
        if let Some(ref effort) = request.reasoning_effort {
            let effort_str = match effort {
                crate::config::types::ReasoningEffortLevel::Low => Some("low"),
                crate::config::types::ReasoningEffortLevel::Medium => Some("medium"),
                crate::config::types::ReasoningEffortLevel::High => Some("high"),
                crate::config::types::ReasoningEffortLevel::Minimal => Some("minimal"),
                crate::config::types::ReasoningEffortLevel::XHigh => Some("high"),
                crate::config::types::ReasoningEffortLevel::None => None,
            };
            if let Some(s) = effort_str {
                payload["reasoning"] = json!({ "effort": s });
            }
        }

        // Add top_k parameter if specified
        if let Some(top_k) = request.top_k {
            payload["top_k"] = json!(top_k);
        }

        // Add presence_penalty parameter if specified
        if let Some(presence_penalty) = request.presence_penalty {
            payload["presence_penalty"] = json!(presence_penalty);
        }

        // Add frequency_penalty parameter if specified
        if let Some(frequency_penalty) = request.frequency_penalty {
            payload["frequency_penalty"] = json!(frequency_penalty);
        }

        // Add tool_choice parameter if specified
        if let Some(ref tool_choice) = request.tool_choice {
            payload["tool_choice"] = json!(tool_choice);
        }

        // Add provider-specific options if present
        if let Some(provider_options) = request.betas.as_ref() {
            payload["provider_options"] = json!(provider_options);
        }

        // Add routing parameters if present
        // This allows specifying which provider to route to in multi-provider setups
        let mut routing_needed = false;
        let mut provider_part = String::new();
        let mut model_name = String::new();
        let mut variant_part = None;

        if let Some(model) = payload.get("model").and_then(|v| v.as_str()) {
            // Check if the model contains provider routing information
            if model.contains("/") {
                // Split provider/model format like "moonshotai/Kimi-K2-Thinking:nebius"
                let parts: Vec<&str> = model.split('/').collect();
                if parts.len() >= 2 {
                    provider_part = parts[0].to_string();
                    let model_part = parts[1];

                    // Extract provider and model name
                    let model_parts: Vec<&str> = model_part.split(':').collect();
                    model_name = model_parts[0].to_string();
                    if model_parts.len() > 1 {
                        variant_part = Some(model_parts[1].to_string());
                    }

                    routing_needed = true;
                }
            }
        }

        if routing_needed {
            // Update the model field to just the model name
            payload["model"] = json!(model_name);

            // Add provider routing information
            let mut routing_info = json!({
                "provider": provider_part
            });

            if let Some(variant) = variant_part {
                routing_info["variant"] = json!(variant);
            }

            payload["routing"] = routing_info;
        }

        // Add prompt cache retention if configured
        if let Some(ref pc) = self.prompt_cache {
            if let Some(ref retention) = pc.providers.openai.prompt_cache_retention {
                payload["prompt_cache_retention"] = json!(retention);
            }
        }

        Ok(payload)
    }

    /// Parse an OpenResponses response into an LLMResponse.
    fn parse_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        let output = response_json
            .get("output")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenResponses",
                    "Invalid response format: missing output array",
                );
                LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let mut content_fragments: Vec<String> = Vec::new();
        let mut reasoning_fragments: Vec<String> = Vec::new();
        let mut reasoning_items: Vec<Value> = Vec::new();
        let mut tool_calls_vec: Vec<ToolCall> = Vec::new();
        let mut tool_references: Vec<String> = Vec::new();

        for item in output {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match item_type {
                "message" => {
                    if let Some(content_array) = item.get("content").and_then(|v| v.as_array()) {
                        for entry in content_array {
                            let entry_type =
                                entry.get("type").and_then(|v| v.as_str()).unwrap_or("");

                            match entry_type {
                                "text" | "output_text" => {
                                    if let Some(text) = entry.get("text").and_then(|v| v.as_str()) {
                                        if !text.is_empty() {
                                            content_fragments.push(text.to_string());
                                        }
                                    }
                                }
                                "refusal" => {
                                    if let Some(refusal) =
                                        entry.get("refusal").and_then(|v| v.as_str())
                                    {
                                        if !refusal.is_empty() {
                                            content_fragments
                                                .push(format!("[Refusal: {}]", refusal));
                                        }
                                    }
                                }
                                "input_text" | "input_image" | "input_file" => {
                                    // These are input content types, typically not part of assistant responses
                                    // but we might encounter them in certain contexts
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "function_call" => {
                    if let Some(call) = self.parse_function_call(item) {
                        tool_calls_vec.push(call);
                    }
                }
                "function_call_output" => {
                    // Handle function call outputs if needed
                    if let Some(output) = item.get("output").and_then(|v| v.as_str()) {
                        content_fragments.push(format!("[Function Output: {}]", output));
                    }
                }
                "reasoning" => {
                    reasoning_items.push(item.clone());

                    // Extract summary content
                    if let Some(summary_array) = item.get("summary").and_then(|v| v.as_array()) {
                        for summary_part in summary_array {
                            if let Some(text) = summary_part.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    reasoning_fragments.push(text.to_string());
                                }
                            }
                        }
                    }

                    // Extract content if available
                    if let Some(content) = item.get("content") {
                        if content.is_string() {
                            if let Some(text) = content.as_str() {
                                if !text.is_empty() {
                                    reasoning_fragments.push(text.to_string());
                                }
                            }
                        } else if content.is_object() || content.is_array() {
                            // For complex content, serialize to string
                            let content_str = content.to_string();
                            if !content_str.is_empty() && content_str != "{}" && content_str != "[]"
                            {
                                reasoning_fragments.push(content_str);
                            }
                        }
                    }
                }
                "item_reference" => {
                    // Handle item references
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        tool_references.push(id.to_string());
                    }
                }
                _ => {}
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
            Some(reasoning_fragments.join("\n\n"))
        };

        let reasoning_details = if reasoning_items.is_empty() {
            None
        } else {
            Some(reasoning_items)
        };

        let finish_reason = if !tool_calls_vec.is_empty() {
            FinishReason::ToolCalls
        } else {
            // Determine finish reason from response status if available
            let status = response_json
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("completed");

            match status {
                "completed" => FinishReason::Stop,
                "failed" | "cancelled" => FinishReason::Error("response_failed".to_string()),
                "incomplete" => FinishReason::Length,
                _ => FinishReason::Stop,
            }
        };

        let tool_calls = if tool_calls_vec.is_empty() {
            None
        } else {
            Some(tool_calls_vec)
        };

        let usage = response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_value
                .get("input_tokens")
                .or_else(|| usage_value.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            completion_tokens: usage_value
                .get("output_tokens")
                .or_else(|| usage_value.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            cached_prompt_tokens: usage_value
                .get("input_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok()),
            cache_creation_tokens: usage_value
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok()),
            cache_read_tokens: usage_value
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok()),
        });

        // Extract request ID and organization ID if available
        let request_id = response_json
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let organization_id = response_json
            .get("organization_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(LLMResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
            reasoning,
            reasoning_details,
            tool_references,
            request_id,
            organization_id,
        })
    }

    /// Parse a function call item into a ToolCall.
    fn parse_function_call(&self, item: &Value) -> Option<ToolCall> {
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let name = item.get("name").and_then(|v| v.as_str())?;

        let arguments = item
            .get("arguments")
            .map(|args| {
                if args.is_string() {
                    args.as_str().unwrap_or("{}").to_string()
                } else {
                    args.to_string()
                }
            })
            .unwrap_or_else(|| "{}".to_string());

        Some(ToolCall::function(
            call_id.to_string(),
            name.to_string(),
            arguments,
        ))
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

    fn supported_models(&self) -> Vec<String> {
        // OpenResponses is model-agnostic; return the configured model
        vec![self.model.clone()]
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        // Validate that the request conforms to OpenResponses specification
        if request.model.is_empty() && self.model.is_empty() {
            return Err(LLMError::InvalidRequest {
                message: "Model must be specified".to_string(),
                metadata: None,
            });
        }

        // Validate message roles are supported by OpenResponses
        for msg in &request.messages {
            match msg.role {
                MessageRole::User | MessageRole::Assistant | MessageRole::System => {
                    // These are all valid OpenResponses roles
                }
                _ => {
                    return Err(LLMError::InvalidRequest {
                        message: format!(
                            "Unsupported message role for OpenResponses: {:?}",
                            msg.role
                        ),
                        metadata: None,
                    });
                }
            }
        }

        // Validate max_tokens range if specified
        if let Some(max_tokens) = request.max_tokens {
            if max_tokens == 0 {
                return Err(LLMError::InvalidRequest {
                    message: "max_tokens must be greater than 0".to_string(),
                    metadata: None,
                });
            }
        }

        // Validate temperature range if specified
        if let Some(temp) = request.temperature {
            if temp < 0.0 || temp > 2.0 {
                return Err(LLMError::InvalidRequest {
                    message: "temperature must be between 0.0 and 2.0".to_string(),
                    metadata: None,
                });
            }
        }

        // Validate top_p range if specified
        if let Some(top_p) = request.top_p {
            if top_p <= 0.0 || top_p > 1.0 {
                return Err(LLMError::InvalidRequest {
                    message: "top_p must be between 0.0 and 1.0".to_string(),
                    metadata: None,
                });
            }
        }

        // Validate top_k range if specified
        if let Some(top_k) = request.top_k {
            if top_k <= 0 {
                return Err(LLMError::InvalidRequest {
                    message: "top_k must be greater than 0".to_string(),
                    metadata: None,
                });
            }
        }

        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        // Validate the request before sending
        self.validate_request(&request)?;

        let payload = self.build_request_payload(&request)?;
        let url = self.responses_url();

        debug!(
            provider = "openresponses",
            model = %request.model,
            url = %url,
            "Sending request"
        );

        let response = self
            .http_client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .header("OpenAI-Beta", "responses-v1") // Indicate OpenResponses API usage
            .header("OpenResponses-Version", &self.version) // Specify OpenResponses version
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenResponses",
                    &format!("Network error: {}", e),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("API error ({}): {}", status.as_u16(), error_text),
            );

            // Try to parse error details from the response
            let error_details: Option<Value> = serde_json::from_str(&error_text).ok();
            let error_code = error_details
                .as_ref()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("code"))
                        .and_then(|c| c.as_str())
                })
                .map(|s| s.to_string());

            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: Some(LLMErrorMetadata::new(
                    "openresponses",
                    Some(status.as_u16()),
                    error_code,       // code
                    None,             // request_id (will be extracted from headers if available)
                    None,             // organization_id
                    None,             // retry_after
                    Some(error_text), // message
                )),
            });
        }

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("Failed to parse response JSON: {}", e),
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        self.parse_response(response_json)
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        // Validate the request before sending
        self.validate_request(&request)?;

        let mut payload = self.build_request_payload(&request)?;
        payload["stream"] = json!(true);

        let url = self.responses_url();

        debug!(
            provider = "openresponses",
            model = %request.model,
            url = %url,
            "Sending streaming request"
        );

        let response = self
            .http_client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .header("OpenAI-Beta", "responses-v1") // Indicate OpenResponses API usage
            .header("OpenResponses-Version", &self.version) // Specify OpenResponses version
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    "OpenResponses",
                    &format!("Streaming request failed: {}", e),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("Streaming API error ({}): {}", status.as_u16(), error_text),
            );

            // Try to parse error details from the response
            let error_details: Option<Value> = serde_json::from_str(&error_text).ok();
            let error_code = error_details
                .as_ref()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("code"))
                        .and_then(|c| c.as_str())
                })
                .map(|s| s.to_string());

            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: Some(LLMErrorMetadata::new(
                    "openresponses",
                    Some(status.as_u16()),
                    error_code,       // code
                    None,             // request_id
                    None,             // organization_id
                    None,             // retry_after
                    Some(error_text), // message
                )),
            });
        }

        // Create a stream from the response bytes
        let byte_stream = response.bytes_stream();

        let event_stream = futures::stream::unfold(
            (byte_stream, String::new(), StreamAccumulator::new(), false),
            |(mut byte_stream, mut buffer, mut accumulator, mut done)| async move {
                use futures::StreamExt;

                if done {
                    return None;
                }

                loop {
                    // Check if we have complete lines in buffer
                    if let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if let Some(event) = parse_sse_event(&line) {
                            accumulator.process_event(&event);

                            // Emit text deltas
                            match event.event_type.as_str() {
                                "response.output_text.delta" => {
                                    if let super::streaming::StreamEventData::TextDelta(data) =
                                        &event.data
                                    {
                                        return Some((
                                            Ok(LLMStreamEvent::Token {
                                                delta: data.delta.clone(),
                                            }),
                                            (byte_stream, buffer, accumulator, done),
                                        ));
                                    }
                                }
                                "response.reasoning_summary_text.delta" => {
                                    if let super::streaming::StreamEventData::TextDelta(_data) =
                                        &event.data
                                    {
                                        // For reasoning deltas, we could emit them separately if needed
                                        // For now, we accumulate them and include in final response
                                    }
                                }
                                "response.reasoning_content.delta" => {
                                    if let super::streaming::StreamEventData::ReasoningContentDelta(_data) = &event.data {
                                        // For reasoning content deltas, we accumulate them
                                        // Could emit them as separate events if needed
                                    }
                                }
                                "response.function_call_arguments.delta" => {
                                    if let super::streaming::StreamEventData::FunctionCallDelta(
                                        _data,
                                    ) = &event.data
                                    {
                                        // For function call argument deltas, we accumulate them
                                        // and emit the completed call at the end
                                    }
                                }
                                "response.completed" => {
                                    done = true;

                                    let tool_calls = if accumulator.function_calls.is_empty() {
                                        None
                                    } else {
                                        Some(
                                            accumulator
                                                .function_calls
                                                .iter()
                                                .map(|fc| {
                                                    ToolCall::function(
                                                        fc.call_id.clone(),
                                                        fc.name.clone(),
                                                        fc.arguments.clone(),
                                                    )
                                                })
                                                .collect(),
                                        )
                                    };

                                    let finish_reason = if tool_calls.is_some() {
                                        FinishReason::ToolCalls
                                    } else {
                                        FinishReason::Stop
                                    };

                                    let response = LLMResponse {
                                        content: if accumulator.text_content.is_empty() {
                                            None
                                        } else {
                                            Some(accumulator.text_content.clone())
                                        },
                                        tool_calls,
                                        usage: None, // Usage info might be available in accumulator
                                        finish_reason,
                                        reasoning: if accumulator.reasoning_content.is_empty() {
                                            None
                                        } else {
                                            Some(accumulator.reasoning_content.clone())
                                        },
                                        reasoning_details: None,
                                        tool_references: Vec::new(),
                                        request_id: accumulator.response_id.clone(),
                                        organization_id: None,
                                    };

                                    return Some((
                                        Ok(LLMStreamEvent::Completed { response }),
                                        (byte_stream, buffer, accumulator, done),
                                    ));
                                }
                                "response.failed" | "error" => {
                                    done = true;

                                    // Return an error response
                                    let error_msg = if let Some(ref err) = accumulator.error {
                                        format!(
                                            "OpenResponses stream error: {} - {}",
                                            err.code, err.message
                                        )
                                    } else {
                                        "OpenResponses stream failed".to_string()
                                    };

                                    return Some((
                                        Err(LLMError::Provider {
                                            message: error_display::format_llm_error(
                                                "OpenResponses",
                                                &error_msg,
                                            ),
                                            metadata: None,
                                        }),
                                        (byte_stream, buffer, accumulator, done),
                                    ));
                                }
                                _ => {
                                    // Ignore other events for now
                                }
                            }
                        }
                        continue;
                    }

                    // Need more data
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(LLMError::Network {
                                    message: format!("Stream error: {}", e),
                                    metadata: None,
                                }),
                                (byte_stream, buffer, accumulator, true),
                            ));
                        }
                        None => {
                            // Stream ended, emit final response if not already done
                            if !done {
                                done = true;

                                let tool_calls = if accumulator.function_calls.is_empty() {
                                    None
                                } else {
                                    Some(
                                        accumulator
                                            .function_calls
                                            .iter()
                                            .map(|fc| {
                                                ToolCall::function(
                                                    fc.call_id.clone(),
                                                    fc.name.clone(),
                                                    fc.arguments.clone(),
                                                )
                                            })
                                            .collect(),
                                    )
                                };

                                let finish_reason = if tool_calls.is_some() {
                                    FinishReason::ToolCalls
                                } else {
                                    FinishReason::Stop
                                };

                                let response = LLMResponse {
                                    content: if accumulator.text_content.is_empty() {
                                        None
                                    } else {
                                        Some(accumulator.text_content.clone())
                                    },
                                    tool_calls,
                                    usage: accumulator.usage.as_ref().map(|usage_value| Usage {
                                        prompt_tokens: usage_value
                                            .get("input_tokens")
                                            .or_else(|| usage_value.get("prompt_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok())
                                            .unwrap_or(0),
                                        completion_tokens: usage_value
                                            .get("output_tokens")
                                            .or_else(|| usage_value.get("completion_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok())
                                            .unwrap_or(0),
                                        total_tokens: usage_value
                                            .get("total_tokens")
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok())
                                            .unwrap_or(0),
                                        cached_prompt_tokens: usage_value
                                            .get("input_tokens_details")
                                            .and_then(|d| d.get("cached_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok()),
                                        cache_creation_tokens: usage_value
                                            .get("cache_creation_input_tokens")
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok()),
                                        cache_read_tokens: usage_value
                                            .get("cache_read_input_tokens")
                                            .and_then(|v| v.as_u64())
                                            .and_then(|v| u32::try_from(v).ok()),
                                    }),
                                    finish_reason,
                                    reasoning: if accumulator.reasoning_content.is_empty() {
                                        None
                                    } else {
                                        Some(accumulator.reasoning_content.clone())
                                    },
                                    reasoning_details: None,
                                    tool_references: Vec::new(),
                                    request_id: accumulator.response_id.clone(),
                                    organization_id: None,
                                };

                                return Some((
                                    Ok(LLMStreamEvent::Completed { response }),
                                    (byte_stream, buffer, accumulator, done),
                                ));
                            }
                            return None;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(event_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        assert_eq!(provider.name(), "openresponses");
    }

    #[test]
    fn test_build_simple_request() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            model: "test-model".to_string(),
            ..Default::default()
        };

        let payload = provider.build_request_payload(&request).unwrap();
        assert_eq!(payload["model"], "test-model");
        assert!(payload["input"].is_array());
    }

    #[test]
    fn test_parse_simple_response() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let response_json = json!({
            "id": "resp_123",
            "object": "response",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "Hello, world!"
                }]
            }],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15
            }
        });

        let response = provider.parse_response(response_json).unwrap();
        assert_eq!(response.content, Some("Hello, world!".to_string()));
        assert!(response.usage.is_some());
    }

    #[test]
    fn test_parse_function_call_response() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let response_json = json!({
            "id": "resp_123",
            "object": "response",
            "output": [{
                "type": "function_call",
                "call_id": "call_abc",
                "name": "get_weather",
                "arguments": "{\"location\":\"NYC\"}"
            }]
        });

        let response = provider.parse_response(response_json).unwrap();
        assert!(response.tool_calls.is_some());
        let calls = response.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    #[test]
    fn test_parse_reasoning_response() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let response_json = json!({
            "id": "resp_123",
            "object": "response",
            "output": [{
                "type": "reasoning",
                "summary": [{"text": "Let me think through this step by step."}],
                "content": {"raw_thoughts": "Initial thoughts..."}
            }, {
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "The answer is 42."
                }]
            }],
            "usage": {
                "input_tokens": 20,
                "output_tokens": 10,
                "total_tokens": 30
            }
        });

        let response = provider.parse_response(response_json).unwrap();
        assert!(response.reasoning.is_some());
        let reasoning = response.reasoning.unwrap();
        // The reasoning should contain the summary text
        assert!(reasoning.contains("Let me think through this step by step."));
        assert_eq!(response.content, Some("The answer is 42.".to_string()));
        assert!(response.usage.is_some());
    }

    #[test]
    fn test_validate_request_with_temperature_range() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let mut request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            model: "test-model".to_string(),
            temperature: Some(2.5), // Invalid range
            ..Default::default()
        };

        let result = provider.validate_request(&request);
        assert!(result.is_err());

        // Test valid temperature
        request.temperature = Some(1.0);
        let result = provider.validate_request(&request);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_request_with_additional_params() {
        let provider = OpenResponsesProvider::new("test-key".to_string());
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())],
            model: "test-model".to_string(),
            top_k: Some(40),
            presence_penalty: Some(0.5),
            frequency_penalty: Some(0.3),
            ..Default::default()
        };

        let payload = provider.build_request_payload(&request).unwrap();
        assert_eq!(payload["top_k"], 40);
        assert_eq!(payload["presence_penalty"], 0.5);
        // Use approximate equality for floating point comparison
        let freq_penalty = payload["frequency_penalty"].as_f64().unwrap();
        assert!((freq_penalty - 0.3).abs() < 1e-6);
    }
}
