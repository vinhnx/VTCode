#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    MessageRole,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::Value;

use super::common::{
    is_minimax_m2_model, normalize_reasoning_detail_objects, override_base_url,
    parse_response_openai_format, resolve_model, serialize_message_content_openai_for_role,
};
use super::error_handling::{format_network_error, format_parse_error};

pub struct MinimaxProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: String,
    model_behavior: Option<ModelConfig>,
}

impl MinimaxProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::minimax::DEFAULT_MODEL.to_string())
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
        let resolved_model = resolve_model(model, models::minimax::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key_value, model_behavior)
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: String,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let resolved_base = override_base_url(
            urls::MINIMAX_API_BASE,
            base_url,
            Some(env_vars::MINIMAX_BASE_URL),
        );

        Self {
            http_client: HttpClient::new(),
            base_url: normalize_openai_base_url(&resolved_base),
            model,
            api_key,
            model_behavior,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut messages = Vec::new();

        // MiniMax's Anthropic-compatible API doesn't support system role in messages
        // System prompt is handled separately via the system_prompt field in the request

        for message in &request.messages {
            // Skip system messages as MiniMax doesn't support them in the messages array
            if message.role == MessageRole::System {
                continue;
            }

            let mut message_obj = serde_json::json!({
                "role": message.role.as_generic_str(),
                "content": serialize_message_content_openai_for_role(&message.role, &message.content)
            });

            if message.role == MessageRole::Assistant
                && let Some(tool_calls) = &message.tool_calls
                && !tool_calls.is_empty()
            {
                let tool_calls_json: Vec<Value> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tc.function.as_ref().map(|func| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": func.name,
                                    "arguments": func.arguments
                                }
                            })
                        })
                    })
                    .collect();

                if !tool_calls_json.is_empty() {
                    message_obj["tool_calls"] = Value::Array(tool_calls_json);
                }
                tracing::debug!(
                    target: "minimax",
                    assistant_message_with_tool_calls = true,
                    tool_call_count = tool_calls.len(),
                    tool_call_ids = ?tool_calls.iter().map(|tc| &tc.id).collect::<Vec<_>>(),
                    "Building assistant message with tool calls"
                );
            }

            if message.role == MessageRole::Assistant
                && let Some(reasoning_details) = &message.reasoning_details
                && !reasoning_details.is_empty()
            {
                let normalized_details = normalize_reasoning_detail_objects(reasoning_details);
                if !normalized_details.is_empty() {
                    message_obj["reasoning_details"] = Value::Array(normalized_details);
                }
            }

            if message.role == MessageRole::Tool {
                let tool_call_id =
                    message
                        .tool_call_id
                        .as_ref()
                        .ok_or_else(|| LLMError::InvalidRequest {
                            message: "Tool response message missing required tool_call_id"
                                .to_string(),
                            metadata: None,
                        })?;
                message_obj["tool_call_id"] = Value::String(tool_call_id.clone());
                tracing::debug!(
                    target: "minimax",
                    tool_result_message = true,
                    tool_call_id = %tool_call_id,
                    "Building tool result message"
                );
            }

            messages.push(message_obj);
        }

        let mut payload = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": stream
        });

        // Add system prompt separately (MiniMax supports this in OpenAI-compatible format)
        if let Some(system) = &request.system_prompt {
            payload["system_prompt"] = serde_json::json!(system);
        }

        if should_enable_reasoning_split(&request.model) {
            payload["reasoning_split"] = Value::Bool(true);
        }

        if let Some(tools) = &request.tools {
            let tools_json: Vec<Value> = tools
                .iter()
                .filter_map(|tool| {
                    tool.function.as_ref().map(|func| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "description": func.description,
                                "parameters": func.parameters
                            }
                        })
                    })
                })
                .collect();

            if !tools_json.is_empty() {
                payload["tools"] = Value::Array(tools_json);
            }
        }

        if let Some(tool_choice) = &request.tool_choice {
            payload["tool_choice"] = tool_choice.to_provider_format("openai");
        }

        if let Some(parallel) = request.parallel_tool_calls
            && payload.get("parallel_tool_calls").is_none()
        {
            payload["parallel_tool_calls"] = Value::Bool(parallel);
        }

        Ok(payload)
    }
}

fn normalize_openai_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.minimax.io/v1".to_string();
    }

    if let Some(prefix) = trimmed.strip_suffix("/v1/messages") {
        let prefix = prefix.trim_end_matches('/');
        if let Some(no_anthropic) = prefix.strip_suffix("/anthropic") {
            return format!("{}/v1", no_anthropic.trim_end_matches('/'));
        }
        return format!("{}/v1", prefix);
    }
    if let Some(prefix) = trimmed.strip_suffix("/messages") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }

    if let Some(prefix) = trimmed.strip_suffix("/anthropic/v1") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }
    if let Some(prefix) = trimmed.strip_suffix("/anthropic") {
        return format!("{}/v1", prefix.trim_end_matches('/'));
    }

    if trimmed.ends_with("/v1") {
        return trimmed.to_string();
    }

    format!("{trimmed}/v1")
}

fn should_enable_reasoning_split(model: &str) -> bool {
    is_minimax_m2_model(model)
}

#[async_trait]
impl LLMProvider for MinimaxProvider {
    fn name(&self) -> &str {
        "minimax"
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = self.chat_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("MiniMax", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                provider = "minimax",
                model = %request.model,
                status = %status,
                has_tools = request.tools.as_ref().is_some_and(|t| !t.is_empty()),
                message_count = request.messages.len(),
                body = %body,
                "MiniMax request failed"
            );
            let formatted_error =
                error_display::format_llm_error("MiniMax", &format!("HTTP {}: {}", status, body));
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error("MiniMax", &e))?;

        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            "MiniMax",
            model,
            false,
            None,
        )
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let url = self.chat_url();

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("MiniMax", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                provider = "minimax",
                model = %request.model,
                status = %status,
                has_tools = request.tools.as_ref().is_some_and(|t| !t.is_empty()),
                message_count = request.messages.len(),
                body = %body,
                "MiniMax stream request failed"
            );
            let formatted_error =
                error_display::format_llm_error("MiniMax", &format!("HTTP {}: {}", status, body));
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
                let chunk = chunk_result.map_err(|e| format_network_error("MiniMax", &e))?;
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
                            && let Some(delta) = choice.get("delta")
                        {
                            // Handle content through the aggregator's sanitizer to extract reasoning
                            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                for ev in aggregator.handle_content(content) {
                                    yield ev;
                                }
                            }

                            if let Some(reasoning_details) =
                                delta.get("reasoning_details").and_then(|v| v.as_array())
                            {
                                aggregator.set_reasoning_details(reasoning_details);
                            }

                            // Handle tool calls
                            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                aggregator.handle_tool_calls(tool_calls);
                            }

                            // Handle finish reason
                            if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str())
                                && finish_reason == "tool_calls"
                            {
                                aggregator.set_finish_reason(FinishReason::ToolCalls);
                            }
                        }
                    }
                }
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::minimax::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            return Err(LLMError::InvalidRequest {
                message: "Messages cannot be empty".to_string(),
                metadata: None,
            });
        }
        Ok(())
    }
}

#[async_trait]
impl LLMClient for MinimaxProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Minimax
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::{MinimaxProvider, normalize_openai_base_url, should_enable_reasoning_split};
    use crate::llm::provider::{LLMRequest, Message};
    use crate::llm::providers::common::normalize_reasoning_detail_object;
    use serde_json::json;

    #[test]
    fn normalize_minimax_anthropic_base_to_openai_v1() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/anthropic"),
            "https://api.minimax.io/v1"
        );
    }

    #[test]
    fn normalize_minimax_anthropic_messages_base_to_openai_v1() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/anthropic/v1/messages"),
            "https://api.minimax.io/v1"
        );
    }

    #[test]
    fn preserve_openai_v1_base() {
        assert_eq!(
            normalize_openai_base_url("https://api.minimax.io/v1"),
            "https://api.minimax.io/v1"
        );
    }

    #[test]
    fn reasoning_split_enabled_for_m2_models() {
        assert!(should_enable_reasoning_split("MiniMax-M2.5"));
        assert!(should_enable_reasoning_split("minimax-m2"));
        assert!(!should_enable_reasoning_split("MiniMax-Text-01"));
    }

    #[test]
    fn payload_preserves_assistant_reasoning_details() {
        let provider =
            MinimaxProvider::with_model("test-key".to_string(), "MiniMax-M2.5".to_string());
        let assistant =
            Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![json!({
                "type": "reasoning_content",
                "text": "chain"
            })]));
        let request = LLMRequest {
            model: "MiniMax-M2.5".to_string(),
            messages: vec![assistant],
            ..Default::default()
        };

        let payload = provider
            .build_payload(&request, false)
            .expect("payload should serialize");

        assert_eq!(payload["reasoning_split"], json!(true));
        let messages = payload["messages"]
            .as_array()
            .expect("messages should be an array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], json!("assistant"));
        assert!(messages[0]["reasoning_details"].is_array());
        assert!(messages[0]["reasoning_details"][0].is_object());
    }

    #[test]
    fn payload_normalizes_stringified_reasoning_details() {
        let provider =
            MinimaxProvider::with_model("test-key".to_string(), "MiniMax-M2.5".to_string());
        let assistant =
            Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![json!(
                r#"{"type":"reasoning.text","id":"r1","text":"chain"}"#
            )]));
        let request = LLMRequest {
            model: "MiniMax-M2.5".to_string(),
            messages: vec![assistant],
            ..Default::default()
        };

        let payload = provider
            .build_payload(&request, false)
            .expect("payload should serialize");
        assert!(payload["messages"][0]["reasoning_details"][0].is_object());
        assert_eq!(
            payload["messages"][0]["reasoning_details"][0]["type"],
            "reasoning.text"
        );
    }

    #[test]
    fn normalize_reasoning_detail_rejects_non_object_strings() {
        assert!(normalize_reasoning_detail_object(&json!("plain text")).is_none());
    }
}
