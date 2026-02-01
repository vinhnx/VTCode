#![allow(clippy::collapsible_if, clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, PromptCachingConfig};
use crate::config::models::Provider as ModelProvider;
use crate::llm::client::LLMClient;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use crate::llm::providers::common::{
    convert_usage_to_llm_types, forward_prompt_cache_with_state, make_default_request,
    map_finish_reason_common, override_base_url, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format, serialize_tools_openai_format, validate_request_common,
};
use crate::llm::providers::error_handling::{
    format_network_error, format_parse_error, handle_openai_http_error,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Map, Value};

const PROVIDER_NAME: &str = "Moonshot";
const PROVIDER_KEY: &str = "moonshot";

/// Moonshot.ai provider with native reasoning support.
pub struct MoonshotProvider {
    api_key: String,
    base_url: String,
    model: String,
    http_client: Client,
    prompt_cache_enabled: bool,
}

impl MoonshotProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            "kimi-k2-0905".to_string(), // Deprecated: use OpenRouter models instead
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
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            http_client,
            prompt_cache_enabled: false,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, "kimi-k2-0905"); // Deprecated: use OpenRouter models instead
        let resolved_base_url = override_base_url(
            urls::MOONSHOT_API_BASE,
            base_url,
            Some(env_vars::MOONSHOT_BASE_URL),
        );
        let (prompt_cache_enabled, _) = forward_prompt_cache_with_state(
            prompt_cache,
            |cfg| cfg.enabled && cfg.providers.moonshot.enabled,
            false,
        );

        let http_client = Client::builder()
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key: api_key.unwrap_or_default(),
            base_url: resolved_base_url,
            model: resolved_model,
            http_client,
            prompt_cache_enabled,
        }
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        Self::from_config(
            Some(api_key),
            Some(model),
            None,
            prompt_cache,
            timeouts,
            None,
        )
    }

    fn convert_to_moonshot_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        // Basic parameters
        payload.insert("model".to_owned(), Value::String(request.model.clone()));

        payload.insert(
            "messages".to_string(),
            Value::Array(self.serialize_messages(request)?),
        );

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_string(),
                Value::Number(serde_json::Number::from(max_tokens)),
            );
        }

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_string(),
                Value::Number(serde_json::Number::from_f64(temperature as f64).unwrap()),
            );
        }

        payload.insert("stream".to_string(), Value::Bool(request.stream));

        // Add tools if present
        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));

            // Add tool choice if specified
            if let Some(choice) = &request.tool_choice {
                payload.insert(
                    "tool_choice".to_string(),
                    choice.to_provider_format(PROVIDER_KEY),
                );
            }
        }

        // Handle reasoning effort for Kimi-K2-Thinking model
        if let Some(effort) = request.reasoning_effort
            && self.supports_reasoning_effort(&request.model)
        {
            // Use the configured reasoning parameters
            if let Some(reasoning_payload) =
                reasoning_parameters_for(ModelProvider::Moonshot, effort)
            {
                // Add the reasoning parameters to the payload
                if let Some(obj) = reasoning_payload.as_object() {
                    for (key, value) in obj {
                        payload.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        Ok(Value::Object(payload))
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        // Use "openai" as provider key since Moonshot is OpenAI-compatible
        serialize_messages_openai_format(request, "openai")
    }

    fn parse_response(&self, response_json: Value) -> Result<LLMResponse, LLMError> {
        parse_response_openai_format(
            response_json,
            PROVIDER_NAME,
            self.prompt_cache_enabled,
            None::<fn(&Value, &Value) -> Option<String>>,
        )
    }
}

#[async_trait]
impl LLMProvider for MoonshotProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        model.to_lowercase().contains("thinking") || model.to_lowercase().contains("k2")
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.supports_reasoning(model)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.validate_request(&request)?;

        let payload = self.convert_to_moonshot_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error(PROVIDER_NAME, &e))?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "MOONSHOT_API_KEY").await?;

        let response_json: Value = response
            .json()
            .await
            .map_err(|e| format_parse_error(PROVIDER_NAME, &e))?;

        self.parse_response(response_json)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.validate_request(&request)?;
        request.stream = true;

        let payload = self.convert_to_moonshot_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error(PROVIDER_NAME, &e))?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "MOONSHOT_API_KEY").await?;

        let bytes_stream = response.bytes_stream();
        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx = event_tx.clone();

        tokio::spawn(async move {
            let mut aggregator = crate::llm::providers::shared::StreamAggregator::new();

            let result = crate::llm::providers::shared::process_openai_stream(
                bytes_stream,
                PROVIDER_NAME,
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
        models::moonshot::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_request_common(
            request,
            PROVIDER_NAME,
            "openai",
            Some(&self.supported_models()),
        )
    }
}

#[async_trait]
impl LLMClient for MoonshotProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = make_default_request(prompt, &self.model);
        let response = <MoonshotProvider as LLMProvider>::generate(self, request).await?;
        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: self.model.clone(),
            usage: response.usage.map(convert_usage_to_llm_types),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Moonshot
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
