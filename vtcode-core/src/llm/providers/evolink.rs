use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};

use super::common::{
    map_finish_reason_common, override_base_url, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format, serialize_tools_openai_format, validate_request_common,
};
use super::error_handling::handle_openai_http_error;
use super::extract_reasoning_trace;

const PROVIDER_NAME: &str = "Evolink";
const PROVIDER_KEY: &str = "evolink";
const PRIMARY_API_KEY_ENV: &str = "EVOLINK_API_KEY";

pub struct EvolinkProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    model_behavior: Option<ModelConfig>,
}

impl EvolinkProvider {
    /// Evolink's gateway expects bare upstream model names (e.g. `gpt-5.2`).
    /// The curated `ModelId` catalog namespaces entries as `evolink/<model>`, so
    /// strip that prefix before sending the request upstream.
    fn normalize_model(model: &str) -> &str {
        model
            .trim()
            .strip_prefix("evolink/")
            .unwrap_or(model.trim())
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::evolink::DEFAULT_MODEL.to_string(),
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
            model: Self::normalize_model(&model).to_string(),
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
        let api_key_value = api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var(PRIMARY_API_KEY_ENV).ok())
            .unwrap_or_default();

        Self::with_model_internal(
            api_key_value,
            resolve_model(model, models::evolink::DEFAULT_MODEL),
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
            base_url: override_base_url(
                urls::EVOLINK_API_BASE,
                base_url,
                Some(env_vars::EVOLINK_BASE_URL),
            ),
            model: Self::normalize_model(&model).to_string(),
            model_behavior,
        }
    }

    fn float_to_json_number(value: f32) -> Result<serde_json::Number, LLMError> {
        serde_json::Number::from_f64(value as f64).ok_or_else(|| LLMError::InvalidRequest {
            message: "invalid numeric parameter value (NaN or infinity)".to_string(),
            metadata: None,
        })
    }

    fn reasoning_effort_value(effort: ReasoningEffortLevel) -> Option<&'static str> {
        match effort {
            ReasoningEffortLevel::None => None,
            ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => Some("low"),
            ReasoningEffortLevel::Medium => Some("medium"),
            ReasoningEffortLevel::High
            | ReasoningEffortLevel::XHigh
            | ReasoningEffortLevel::Max => Some("high"),
        }
    }

    fn is_reasoning_enabled(request: &LLMRequest) -> bool {
        request
            .reasoning_effort
            .is_some_and(|effort| effort != ReasoningEffortLevel::None)
    }

    fn convert_to_evolink_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::with_capacity(10);
        payload.insert(
            "model".to_owned(),
            Value::String(Self::normalize_model(&request.model).to_string()),
        );

        let mut messages = serialize_messages_openai_format(request, PROVIDER_KEY)?;
        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                messages.insert(
                    0,
                    serde_json::json!({ "role": "system", "content": trimmed }),
                );
            }
        }
        payload.insert("messages".to_owned(), Value::Array(messages));

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        if !Self::is_reasoning_enabled(request) {
            if let Some(temperature) = request.temperature {
                payload.insert(
                    "temperature".to_owned(),
                    Value::Number(Self::float_to_json_number(temperature)?),
                );
            }

            if let Some(top_p) = request.top_p {
                payload.insert(
                    "top_p".to_owned(),
                    Value::Number(Self::float_to_json_number(top_p)?),
                );
            }
        }

        if request.stream {
            payload.insert("stream".to_owned(), Value::Bool(true));
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_owned(), Value::Array(serialized_tools));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_owned(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        if let Some(effort) = request.reasoning_effort
            && let Some(mapped) = Self::reasoning_effort_value(effort)
        {
            payload.insert(
                "reasoning_effort".to_owned(),
                Value::String(mapped.to_string()),
            );
        }

        Ok(Value::Object(payload))
    }

    fn is_anthropic_model(model: &str) -> bool {
        models::evolink::is_anthropic_format(model)
    }

    fn convert_to_anthropic_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::with_capacity(8);
        let model = Self::normalize_model(&request.model).to_string();
        payload.insert("model".to_owned(), Value::String(model));

        // Anthropic uses top-level `system` field, not a system message
        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                payload.insert("system".to_owned(), Value::String(trimmed.to_string()));
            }
        }

        // Convert messages to Anthropic format (user/assistant only, no system)
        let anthropic_messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|msg| msg.role != crate::llm::provider::MessageRole::System)
            .map(|msg| {
                let role = match msg.role {
                    crate::llm::provider::MessageRole::User => "user",
                    crate::llm::provider::MessageRole::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({
                    "role": role,
                    "content": msg.content.as_text()
                })
            })
            .collect();
        payload.insert("messages".to_owned(), Value::Array(anthropic_messages));

        let max_tokens = request.max_tokens.unwrap_or(8192);
        payload.insert(
            "max_tokens".to_owned(),
            Value::Number(serde_json::Number::from(max_tokens as u64)),
        );

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_owned(),
                Value::Number(Self::float_to_json_number(temperature)?),
            );
        }

        if request.stream {
            payload.insert("stream".to_owned(), Value::Bool(true));
        }

        Ok(Value::Object(payload))
    }

    fn parse_anthropic_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let content = response_json
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            block.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            });

        let usage = response_json.get("usage").map(|u| {
            let prompt_tokens = u.get("input_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32;
            let completion_tokens = u.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as u32;
            crate::llm::provider::Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                cached_prompt_tokens: u
                    .get("cache_read_input_tokens")
                    .and_then(|t| t.as_u64())
                    .map(|v| v as u32),
                cache_creation_tokens: u
                    .get("cache_creation_input_tokens")
                    .and_then(|t| t.as_u64())
                    .map(|v| v as u32),
                cache_read_tokens: None,
            }
        });

        let finish_reason = match response_json
            .get("stop_reason")
            .and_then(|r| r.as_str())
        {
            Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            Some("tool_use") => FinishReason::ToolCalls,
            _ => FinishReason::Stop,
        };

        Ok(LLMResponse {
            content,
            tool_calls: None,
            model,
            usage,
            finish_reason,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: response_json
                .get("id")
                .and_then(|id| id.as_str())
                .map(String::from),
            organization_id: None,
            compaction: None,
        })
    }

    async fn generate_anthropic(
        &self,
        mut request: LLMRequest,
        model: String,
    ) -> Result<LLMResponse, LLMError> {
        request.stream = false;
        let payload = self.convert_to_anthropic_format(&request)?;
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await
            .map_err(|error| LLMError::Network {
                message: error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("network error: {error}"),
                ),
                metadata: None,
            })?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, PRIMARY_API_KEY_ENV).await?;

        let response_json: Value = response.json().await.map_err(|error| LLMError::Provider {
            message: error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("failed to parse Anthropic response: {error}"),
            ),
            metadata: None,
        })?;

        Self::parse_anthropic_response(response_json, model)
    }
}

#[async_trait]
impl LLMProvider for EvolinkProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            Self::normalize_model(model)
        };

        self.model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning)
            .unwrap_or(false)
            || models::evolink::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            Self::normalize_model(model)
        };

        self.model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning_effort)
            .unwrap_or(false)
            || models::evolink::REASONING_MODELS.contains(&requested)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = Self::normalize_model(&request.model).to_string();

        if Self::is_anthropic_model(&model) {
            return self.generate_anthropic(request, model).await;
        }

        let payload = self.convert_to_evolink_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| LLMError::Network {
                message: error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("network error: {error}"),
                ),
                metadata: None,
            })?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, PRIMARY_API_KEY_ENV).await?;

        let response_json: Value = response.json().await.map_err(|error| LLMError::Provider {
            message: error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("failed to parse response: {error}"),
            ),
            metadata: None,
        })?;

        let reasoning_extractor = |message: &Value, choice: &Value| {
            message
                .get("reasoning")
                .or_else(|| message.get("reasoning_content"))
                .and_then(extract_reasoning_trace)
                .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
        };

        parse_response_openai_format(
            response_json,
            PROVIDER_NAME,
            model,
            false,
            Some(reasoning_extractor),
        )
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.validate_request(&request)?;
        let model = Self::normalize_model(&request.model).to_string();

        // Anthropic models: fall back to non-streaming via generate_anthropic
        if Self::is_anthropic_model(&model) {
            request.stream = false;
            let response = self.generate_anthropic(request, model).await?;
            let (tx, rx) =
                tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
            let _ = tx.send(Ok(LLMStreamEvent::Completed {
                response: Box::new(response),
            }));
            let stream = async_stream::try_stream! {
                let mut receiver = rx;
                while let Some(event) = receiver.recv().await {
                    yield event?;
                }
            };
            return Ok(Box::pin(stream));
        }

        request.stream = true;

        let payload = self.convert_to_evolink_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| LLMError::Network {
                message: error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("network error: {error}"),
                ),
                metadata: None,
            })?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, PRIMARY_API_KEY_ENV).await?;

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
                    if let Some(choices) =
                        value.get("choices").and_then(|choices| choices.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(reasoning) = delta
                                .get("reasoning")
                                .or_else(|| delta.get("reasoning_content"))
                                .and_then(|v| v.as_str())
                                && let Some(delta) = aggregator.handle_reasoning(reasoning)
                            {
                                let _ = tx.send(Ok(LLMStreamEvent::Reasoning { delta }));
                            }

                            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                for event in aggregator.handle_content(content) {
                                    let _ = tx.send(Ok(event));
                                }
                            }

                            if let Some(tool_calls) =
                                delta.get("tool_calls").and_then(|calls| calls.as_array())
                            {
                                aggregator.handle_tool_calls(tool_calls);
                            }
                        }

                        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                            aggregator.set_finish_reason(map_finish_reason_common(reason));
                        }
                    }

                    if let Some(_usage_value) = value.get("usage")
                        && let Some(usage) =
                            crate::llm::providers::common::parse_usage_openai_format(&value, false)
                    {
                        aggregator.set_usage(usage);
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
                Err(error) => {
                    let _ = tx.send(Err(error));
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
        models::evolink::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        // Evolink is a gateway whose upstream catalog changes over time, so do
        // not constrain requests to the curated `SUPPORTED_MODELS` list.
        validate_request_common(request, PROVIDER_NAME, PROVIDER_KEY, None)
    }
}

#[async_trait]
impl LLMClient for EvolinkProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = super::common::make_default_request(prompt, &self.model);
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::EvolinkProvider;
    use crate::config::constants::{models, urls};
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::{LLMRequest, Message};

    #[test]
    fn normalizes_namespaced_model_for_wire() {
        let provider =
            EvolinkProvider::with_model("test-key".to_string(), "evolink/gpt-5.2".to_string());
        assert_eq!(provider.model_id_for_test(), models::evolink::GPT_5_2);
    }

    #[test]
    fn defaults_to_direct_base_url() {
        let provider = EvolinkProvider::new("test-key".to_string());
        assert_eq!(provider.base_url_for_test(), urls::EVOLINK_API_BASE);
    }

    #[test]
    fn payload_strips_prefix_and_maps_reasoning_effort() {
        let provider = EvolinkProvider::new("test-key".to_string());
        let payload = provider
            .convert_to_evolink_format(&LLMRequest {
                model: "evolink/deepseek-v4-pro".to_string(),
                messages: vec![Message::user("hello".to_string())],
                reasoning_effort: Some(ReasoningEffortLevel::High),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert_eq!(
            payload.get("model").and_then(|value| value.as_str()),
            Some(models::evolink::DEEPSEEK_V4_PRO)
        );
        assert_eq!(
            payload
                .get("reasoning_effort")
                .and_then(|value| value.as_str()),
            Some("high")
        );
        assert!(payload.get("temperature").is_none());
    }

    impl EvolinkProvider {
        fn model_id_for_test(&self) -> &str {
            &self.model
        }

        fn base_url_for_test(&self) -> &str {
            &self.base_url
        }
    }
}
