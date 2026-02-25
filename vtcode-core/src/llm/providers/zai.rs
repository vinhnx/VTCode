#![allow(clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;

use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use super::common::{
    map_finish_reason_common, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format, serialize_tools_openai_format, validate_request_common,
};
use super::error_handling::handle_openai_http_error;

const PROVIDER_NAME: &str = "Z.AI";
const PROVIDER_KEY: &str = "zai";

pub struct ZAIProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    model_behavior: Option<ModelConfig>,
}

impl ZAIProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::zai::DEFAULT_MODEL.to_string(),
            None,
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model,
            model_behavior: None,
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
        let model_value = resolve_model(model, models::zai::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            base_url,
            timeouts,
            model_behavior,
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
            base_url: resolve_zai_base_url(base_url),
            model,
            model_behavior,
        }
    }

    fn convert_to_zai_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();
        let normalized_model = normalize_model_id(&request.model);
        let has_preserved_reasoning = request.messages.iter().any(|message| {
            message.role == crate::llm::provider::MessageRole::Assistant
                && message
                    .reasoning
                    .as_ref()
                    .is_some_and(|reasoning| !reasoning.is_empty())
        });

        payload.insert("model".to_owned(), Value::String(normalized_model));
        payload.insert(
            "messages".to_owned(),
            Value::Array(serialize_messages_openai_format(request, PROVIDER_KEY)?),
        );

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_owned(),
                Value::Number(serde_json::Number::from_f64(temperature as f64).ok_or_else(
                    || LLMError::InvalidRequest {
                        message: "Invalid temperature value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }
        if let Some(top_p) = request.top_p {
            payload.insert(
                "top_p".to_owned(),
                Value::Number(serde_json::Number::from_f64(top_p as f64).ok_or_else(|| {
                    LLMError::InvalidRequest {
                        message: "Invalid top_p value".to_string(),
                        metadata: None,
                    }
                })?),
            );
        }
        if let Some(do_sample) = request.do_sample {
            payload.insert("do_sample".to_owned(), Value::Bool(do_sample));
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
            if request
                .tools
                .as_ref()
                .is_some_and(|tools| !tools.is_empty())
            {
                payload.insert("tool_stream".to_string(), Value::Bool(true));
            }
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));
        }

        if request.output_format.is_some() {
            payload.insert(
                "response_format".to_owned(),
                serde_json::json!({ "type": "json_object" }),
            );
        }

        if let Some(choice) = &request.tool_choice {
            let tool_choice_value = match choice {
                crate::llm::provider::ToolChoice::Auto => choice.to_provider_format(PROVIDER_KEY),
                _ => serde_json::Value::String("auto".to_string()),
            };
            payload.insert("tool_choice".to_string(), tool_choice_value);
        } else if request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            payload.insert(
                "tool_choice".to_string(),
                serde_json::Value::String("auto".to_string()),
            );
        }

        if let Some(effort) = request.reasoning_effort {
            if effort == crate::config::types::ReasoningEffortLevel::None {
                payload.insert(
                    "thinking".to_owned(),
                    serde_json::json!({"type": "disabled"}),
                );
                return Ok(Value::Object(payload));
            }

            use crate::config::models::Provider;
            use crate::llm::rig_adapter::reasoning_parameters_for;
            if let Some(reasoning_params) = reasoning_parameters_for(Provider::ZAI, effort) {
                if let Some(params_obj) = reasoning_params.as_object() {
                    for (k, v) in params_obj {
                        payload.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        if has_preserved_reasoning {
            if let Some(thinking) = payload.get_mut("thinking").and_then(Value::as_object_mut) {
                thinking.insert("clear_thinking".to_owned(), Value::Bool(false));
            } else {
                payload.insert(
                    "thinking".to_owned(),
                    serde_json::json!({
                        "type": "enabled",
                        "clear_thinking": false
                    }),
                );
            }
        }

        Ok(Value::Object(payload))
    }
}

fn normalize_model_id(model: &str) -> String {
    if model == models::zai::GLM_5_LEGACY {
        models::zai::GLM_5.to_string()
    } else {
        model.to_string()
    }
}

#[async_trait]
impl LLMProvider for ZAIProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        model.contains("glm")
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        // Same robustness logic for reasoning effort
        model.contains("glm")
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning_effort)
                .unwrap_or(false)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.convert_to_zai_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Accept-Language", "en-US,en")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("Network error: {}", e),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let response = handle_openai_http_error(response, PROVIDER_NAME, "ZAI_API_KEY").await?;

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            PROVIDER_NAME,
            model,
            false,
            None,
        )
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        self.validate_request(&request)?;
        request.stream = true;

        let payload = self.convert_to_zai_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Accept-Language", "en-US,en")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error = error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("Network error: {}", e),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let response = handle_openai_http_error(response, PROVIDER_NAME, "ZAI_API_KEY").await?;

        let bytes_stream = response.bytes_stream();
        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx = event_tx.clone();

        let model_clone = model.clone();
        tokio::spawn(async move {
            let mut aggregator =
                crate::llm::providers::shared::StreamAggregator::new(model_clone.clone());

            let result = crate::llm::providers::shared::process_openai_stream(
                bytes_stream,
                PROVIDER_NAME,
                model_clone,
                |value| {
                    if let Some(choices) = value.get("choices").and_then(|c| c.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                for event in aggregator.handle_content(content) {
                                    let _ = tx.send(Ok(event));
                                }
                            }

                            if let Some(reasoning) =
                                delta.get("reasoning_content").and_then(|c| c.as_str())
                            {
                                if let Some(d) = aggregator.handle_reasoning(reasoning) {
                                    let _ = tx.send(Ok(LLMStreamEvent::Reasoning { delta: d }));
                                }
                            }

                            if let Some(tool_calls) =
                                delta.get("tool_calls").and_then(|tc| tc.as_array())
                            {
                                aggregator.handle_tool_calls(tool_calls);
                            }
                        }

                        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                            aggregator.set_finish_reason(map_finish_reason_common(reason));
                        }
                    }

                    if let Some(_usage_value) = value.get("usage") {
                        if let Some(usage) =
                            crate::llm::providers::common::parse_usage_openai_format(&value, false)
                        {
                            aggregator.set_usage(usage);
                        }
                    }
                    Ok(())
                },
            )
            .await;

            match result {
                Ok(_) => {
                    let response = aggregator.finalize();
                    let _ = tx.send(Ok(LLMStreamEvent::Completed {
                        response: Box::new(response),
                    }));
                }
                Err(err) => {
                    let _ = tx.send(Err(err));
                }
            }
        });

        let stream = try_stream! {
            let mut receiver = event_rx;
            while let Some(event) = receiver.recv().await {
                yield event?;
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::zai::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let supported_models = models::zai::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect::<Vec<_>>();

        validate_request_common(
            request,
            PROVIDER_NAME,
            PROVIDER_KEY,
            Some(&supported_models),
        )
    }
}

fn resolve_zai_base_url(base_url: Option<String>) -> String {
    if let Some(url) = base_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Ok(value) = std::env::var(env_vars::ZAI_BASE_URL) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Ok(legacy) = std::env::var(env_vars::Z_AI_BASE_URL) {
        let trimmed = legacy.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    urls::ZAI_API_BASE.to_string()
}

#[async_trait]
impl LLMClient for ZAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::ZAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::{ZAIProvider, normalize_model_id, resolve_zai_base_url};
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::{LLMRequest, Message, ToolChoice, ToolDefinition};
    use std::sync::Arc;

    #[test]
    fn normalizes_legacy_glm5_model_id() {
        assert_eq!(
            normalize_model_id(models::zai::GLM_5_LEGACY),
            models::zai::GLM_5
        );
    }

    #[test]
    fn keeps_canonical_glm5_model_id() {
        assert_eq!(normalize_model_id(models::zai::GLM_5), models::zai::GLM_5);
    }

    #[test]
    fn keeps_glm47_model_id() {
        assert_eq!(normalize_model_id(models::zai::GLM_47), models::zai::GLM_47);
    }

    #[test]
    fn payload_includes_top_p() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            top_p: Some(0.95),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        let top_p = payload
            .get("top_p")
            .and_then(|v| v.as_f64())
            .expect("top_p should be present");
        assert!((top_p - 0.95).abs() < 1e-6);
    }

    #[test]
    fn payload_enables_tool_stream_when_streaming_with_tools() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            stream: true,
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }),
            )])),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(payload.get("stream").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            payload.get("tool_stream").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn payload_streaming_without_tools_does_not_set_tool_stream() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            stream: true,
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(payload.get("stream").and_then(|v| v.as_bool()), Some(true));
        assert!(payload.get("tool_stream").is_none());
    }

    #[test]
    fn zai_base_url_uses_explicit_override() {
        let resolved =
            resolve_zai_base_url(Some("https://api.z.ai/api/coding/paas/v4".to_string()));
        assert_eq!(resolved, "https://api.z.ai/api/coding/paas/v4");
    }

    #[test]
    fn payload_includes_do_sample() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            do_sample: Some(false),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload.get("do_sample").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn payload_disables_thinking_for_none_effort() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            reasoning_effort: Some(ReasoningEffortLevel::None),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload
                .get("thinking")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("disabled")
        );
    }

    #[test]
    fn payload_enables_thinking_for_low_effort() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            reasoning_effort: Some(ReasoningEffortLevel::Low),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload
                .get("thinking")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("enabled")
        );
        assert_eq!(
            payload.get("thinking_effort").and_then(|v| v.as_str()),
            Some("low")
        );
    }

    #[test]
    fn payload_enables_preserved_thinking_when_reasoning_history_present() {
        let provider = ZAIProvider::new("test-key".to_string());
        let mut assistant = Message::assistant("tool planning".to_string());
        assistant.reasoning = Some("reason step 1".to_string());

        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![assistant],
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload
                .get("thinking")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("enabled")
        );
        assert_eq!(
            payload
                .get("thinking")
                .and_then(|v| v.get("clear_thinking"))
                .and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn payload_serializes_assistant_reasoning_content() {
        let provider = ZAIProvider::new("test-key".to_string());
        let mut assistant = Message::assistant("answer".to_string());
        assistant.reasoning = Some("chain".to_string());

        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![assistant],
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        let messages = payload
            .get("messages")
            .and_then(|v| v.as_array())
            .expect("messages should be serialized");
        let first = messages.first().expect("at least one message");
        assert_eq!(
            first.get("reasoning_content").and_then(|v| v.as_str()),
            Some("chain")
        );
    }

    #[test]
    fn payload_serializes_web_search_tool() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("latest economic events".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition::web_search(
                serde_json::json!({
                    "enable": true,
                    "search_engine": "search-prime",
                    "count": 5
                }),
            )])),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        let tools = payload
            .get("tools")
            .and_then(|v| v.as_array())
            .expect("tools should be serialized");
        let first = tools.first().expect("at least one tool");
        assert_eq!(
            first.get("type").and_then(|v| v.as_str()),
            Some("web_search")
        );
        assert_eq!(
            first
                .get("web_search")
                .and_then(|v| v.get("search_engine"))
                .and_then(|v| v.as_str()),
            Some("search-prime")
        );
    }

    #[test]
    fn payload_tool_choice_auto_when_requested() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            tool_choice: Some(ToolChoice::auto()),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload.get("tool_choice").and_then(|v| v.as_str()),
            Some("auto")
        );
    }

    #[test]
    fn payload_forces_tool_choice_to_auto_for_non_auto_modes() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            tool_choice: Some(ToolChoice::none()),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload.get("tool_choice").and_then(|v| v.as_str()),
            Some("auto")
        );
    }

    #[test]
    fn payload_defaults_tool_choice_to_auto_when_tools_provided() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("hello".to_string())],
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "get_weather".to_string(),
                "Get weather".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }),
            )])),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload.get("tool_choice").and_then(|v| v.as_str()),
            Some("auto")
        );
    }

    #[test]
    fn payload_enables_json_mode_when_output_format_requested() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("return json".to_string())],
            output_format: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "sentiment": {"type": "string"}
                }
            })),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload
                .get("response_format")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("json_object")
        );
    }

    #[test]
    fn payload_keeps_json_mode_when_thinking_disabled() {
        let provider = ZAIProvider::new("test-key".to_string());
        let request = LLMRequest {
            model: models::zai::GLM_5.to_string(),
            messages: vec![Message::user("return json".to_string())],
            output_format: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "sentiment": {"type": "string"}
                }
            })),
            reasoning_effort: Some(ReasoningEffortLevel::None),
            ..Default::default()
        };

        let payload = provider
            .convert_to_zai_format(&request)
            .expect("payload should be valid");
        assert_eq!(
            payload
                .get("response_format")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("json_object")
        );
        assert_eq!(
            payload
                .get("thinking")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("disabled")
        );
    }
}
