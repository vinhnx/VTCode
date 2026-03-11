//! LiteLLM provider implementation.
//!
//! LiteLLM is an OpenAI-compatible proxy that routes requests to 100+ LLM
//! providers. VT Code connects to it via the standard `/chat/completions`
//! endpoint, defaulting to `http://localhost:4000`.
//!
//! Configuration precedence for the base URL:
//! 1. Explicit `base_url` in [`LiteLLMProvider::from_config`]
//! 2. `LITELLM_BASE_URL` environment variable
//! 3. Built-in default ([`urls::LITELLM_API_BASE`])

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
    map_finish_reason_common, override_base_url, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format, serialize_tools_openai_format,
};
use super::error_handling::handle_openai_http_error;

const PROVIDER_NAME: &str = "LiteLLM";
const PROVIDER_KEY: &str = "litellm";
const API_KEY_ENV: &str = "LITELLM_API_KEY";

pub struct LiteLLMProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
}

impl LiteLLMProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::litellm::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: HttpClient,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        _prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        _model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::litellm::DEFAULT_MODEL);

        Self::with_model_internal(api_key_value, model_value, base_url, timeouts)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::LITELLM_API_BASE,
                base_url,
                Some(env_vars::LITELLM_BASE_URL),
            ),
            model,
        }
    }

    /// Build the chat completions endpoint URL.
    #[inline]
    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Send an HTTP POST to the LiteLLM proxy and handle transport errors.
    async fn send_request(&self, payload: &Value) -> Result<reqwest::Response, LLMError> {
        let url = self.completions_url();

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| {
                let formatted =
                    error_display::format_llm_error(PROVIDER_NAME, &format!("network error: {e}"));
                LLMError::Network {
                    message: formatted,
                    metadata: None,
                }
            })?;

        handle_openai_http_error(response, PROVIDER_NAME, API_KEY_ENV).await
    }

    fn build_payload(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        payload.insert("model".to_owned(), Value::String(request.model.clone()));
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
                        message: "invalid temperature value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }

        if request.stream {
            payload.insert("stream".to_owned(), Value::Bool(true));
        }

        if let Some(tools) = &request.tools
            && let Some(serialized) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_owned(), Value::Array(serialized));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_owned(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        Ok(Value::Object(payload))
    }

    /// Resolve the effective model, falling back to the configured default.
    fn resolve_request_model(&self, request: &mut LLMRequest) {
        if request.model.trim().is_empty() {
            request.model.clone_from(&self.model);
        }
    }
}

#[async_trait]
impl LLMProvider for LiteLLMProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.resolve_request_model(&mut request);
        let model = request.model.clone();

        let payload = self.build_payload(&request)?;
        let response = self.send_request(&payload).await?;

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("failed to parse response: {e}"),
            );
            LLMError::Provider {
                message: formatted,
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
        self.resolve_request_model(&mut request);
        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.build_payload(&request)?;
        let response = self.send_request(&payload).await?;

        let bytes_stream = response.bytes_stream();
        let (tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx_spawn = tx.clone();

        tokio::spawn(async move {
            let mut aggregator =
                crate::llm::providers::shared::StreamAggregator::new(model.clone());

            let result = crate::llm::providers::shared::process_openai_stream(
                bytes_stream,
                PROVIDER_NAME,
                model,
                |value| {
                    if let Some(choices) = value.get("choices").and_then(|c| c.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                for event in aggregator.handle_content(content) {
                                    let _ = tx_spawn.send(Ok(event));
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

                    if value.get("usage").is_some() {
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
                    let _ = tx_spawn.send(Ok(LLMStreamEvent::Completed {
                        response: Box::new(response),
                    }));
                }
                Err(err) => {
                    let _ = tx_spawn.send(Err(err));
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
        models::litellm::SUPPORTED_MODELS
            .iter()
            .map(|m| (*m).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        // LiteLLM proxies any model — skip model validation, only validate basics
        super::common::validate_request_common(request, PROVIDER_NAME, PROVIDER_KEY, None)
    }
}

#[async_trait]
impl LLMClient for LiteLLMProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        LLMProvider::generate(self, request).await
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::LiteLLM
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
