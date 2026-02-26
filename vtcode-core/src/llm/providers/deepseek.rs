#![allow(clippy::collapsible_if, clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{
    AnthropicConfig, DeepSeekPromptCacheSettings, ModelConfig, PromptCachingConfig,
};
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

use super::{
    common::{
        execute_token_count_request, extract_prompt_cache_settings, map_finish_reason_common,
        override_base_url, parse_prompt_tokens_from_count_response, parse_response_openai_format,
        resolve_model, serialize_messages_openai_format, serialize_tools_openai_format,
        strip_generation_controls_for_token_count, validate_request_common,
    },
    error_handling::handle_openai_http_error,
    extract_reasoning_trace,
};

const PROVIDER_NAME: &str = "DeepSeek";
const PROVIDER_KEY: &str = "deepseek";

pub struct DeepSeekProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: DeepSeekPromptCacheSettings,
    model_behavior: Option<ModelConfig>,
}

impl DeepSeekProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::deepseek::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, TimeoutsConfig::default(), None)
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
            prompt_cache_enabled: false,
            prompt_cache_settings: DeepSeekPromptCacheSettings::default(),
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::deepseek::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
            model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.deepseek,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::DEEPSEEK_API_BASE,
                base_url,
                Some(env_vars::DEEPSEEK_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
            model_behavior,
        }
    }

    fn convert_to_deepseek_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        payload.insert("model".to_owned(), Value::String(request.model.clone()));
        payload.insert(
            "messages".to_owned(),
            Value::Array(self.serialize_messages(request)?),
        );

        if let Some(system_prompt) = &request.system_prompt {
            payload.insert(
                "system".to_owned(),
                Value::String(system_prompt.trim().to_owned()),
            );
        }

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

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_string(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        if let Some(effort) = request.reasoning_effort {
            payload.insert(
                "reasoning_effort".to_string(),
                Value::String(effort.to_string()),
            );
        }

        Ok(Value::Object(payload))
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        serialize_messages_openai_format(request, PROVIDER_KEY)
    }

    fn parse_response(&self, response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let include_cache = self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;

        // Custom reasoning extractor for DeepSeek
        let reasoning_extractor = |message: &Value, choice: &Value| {
            message
                .get("reasoning_content")
                .and_then(extract_reasoning_trace)
                .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
                .or_else(|| {
                    choice
                        .get("reasoning_content")
                        .and_then(extract_reasoning_trace)
                })
        };

        parse_response_openai_format(
            response_json,
            PROVIDER_NAME,
            model,
            include_cache,
            Some(reasoning_extractor),
        )
    }
}

#[async_trait]
impl LLMProvider for DeepSeekProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        // Codex-inspired robustness: Setting model_supports_reasoning to false
        // does NOT disable it for known reasoning models.
        requested == models::deepseek::DEEPSEEK_REASONER
            || self
                .model_behavior
                .as_ref()
                .and_then(|b| b.model_supports_reasoning)
                .unwrap_or(false)
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        // Same robustness logic for reasoning effort
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    async fn count_prompt_tokens_exact(
        &self,
        request: &LLMRequest,
    ) -> Result<Option<u32>, LLMError> {
        let mut request = request.clone();
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        let mut payload = self.convert_to_deepseek_format(&request)?;
        strip_generation_controls_for_token_count(&mut payload);

        let url = format!(
            "{}/responses/input_tokens",
            self.base_url.trim_end_matches('/')
        );
        let response_json = execute_token_count_request(
            self.http_client.post(&url).bearer_auth(&self.api_key),
            &payload,
            PROVIDER_NAME,
        )
        .await?;

        let Some(response_json) = response_json else {
            return Ok(None);
        };

        Ok(parse_prompt_tokens_from_count_response(&response_json))
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut request = request;
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.convert_to_deepseek_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
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

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "DEEPSEEK_API_KEY").await?;

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

        self.parse_response(response_json, model)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.convert_to_deepseek_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
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

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "DEEPSEEK_API_KEY").await?;

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
                            if let Some(reasoning) =
                                delta.get("reasoning_content").and_then(|r| r.as_str())
                            {
                                if let Some(delta) = aggregator.handle_reasoning(reasoning) {
                                    let _ = tx.send(Ok(LLMStreamEvent::Reasoning { delta }));
                                }
                            }

                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                for event in aggregator.handle_content(content) {
                                    let _ = tx.send(Ok(event));
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
                            crate::llm::providers::common::parse_usage_openai_format(&value, true)
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
        models::deepseek::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let supported_models = models::deepseek::SUPPORTED_MODELS
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

#[async_trait]
impl LLMClient for DeepSeekProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = super::common::make_default_request(prompt, &self.model);
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::DeepSeek
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::DeepSeekProvider;
    use crate::config::TimeoutsConfig;
    use crate::config::constants::models;
    use crate::llm::provider::{LLMProvider, LLMRequest, Message};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_request(model: &str) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn exact_count_uses_deepseek_input_tokens_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "input_tokens": 201
            })))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new_with_client(
            "test-key".to_string(),
            models::deepseek::DEFAULT_MODEL.to_string(),
            reqwest::Client::new(),
            format!("{}/v1", server.uri()),
            TimeoutsConfig::default(),
        );

        let count = <DeepSeekProvider as LLMProvider>::count_prompt_tokens_exact(
            &provider,
            &sample_request(models::deepseek::DEFAULT_MODEL),
        )
        .await
        .expect("count should succeed");
        assert_eq!(count, Some(201));
    }

    #[tokio::test]
    async fn exact_count_accepts_prompt_tokens_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "prompt_tokens": 66
            })))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new_with_client(
            "test-key".to_string(),
            models::deepseek::DEFAULT_MODEL.to_string(),
            reqwest::Client::new(),
            format!("{}/v1", server.uri()),
            TimeoutsConfig::default(),
        );

        let count = <DeepSeekProvider as LLMProvider>::count_prompt_tokens_exact(
            &provider,
            &sample_request(models::deepseek::DEFAULT_MODEL),
        )
        .await
        .expect("count should succeed");
        assert_eq!(count, Some(66));
    }

    #[tokio::test]
    async fn exact_count_returns_none_when_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/input_tokens"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new_with_client(
            "test-key".to_string(),
            models::deepseek::DEFAULT_MODEL.to_string(),
            reqwest::Client::new(),
            format!("{}/v1", server.uri()),
            TimeoutsConfig::default(),
        );

        let count = <DeepSeekProvider as LLMProvider>::count_prompt_tokens_exact(
            &provider,
            &sample_request(models::deepseek::DEFAULT_MODEL),
        )
        .await
        .expect("count should succeed");
        assert_eq!(count, None);
    }
}
