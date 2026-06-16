use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use super::common::{
    chat_completions_url, ensure_model, impl_llm_client, override_base_url, parse_json_response,
    parse_response_openai_format, resolve_model, serialize_messages_openai_format,
    spawn_openai_compatible_stream,
};
use super::error_handling::{format_network_error, handle_openai_http_error};

const PROVIDER_NAME: &str = "Moonshot";
const PROVIDER_KEY: &str = "moonshot";

pub struct MoonshotProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
}

impl MoonshotProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::moonshot::DEFAULT_MODEL.to_string(),
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
            model: model.trim().to_string(),
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
        let model_value = resolve_model(model, models::moonshot::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            base_url,
            timeouts,
            _model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        base_url: Option<String>,
        timeouts: Option<TimeoutsConfig>,
        _model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let timeouts = timeouts.unwrap_or_default();

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::MOONSHOT_API_BASE,
                base_url,
                Some(env_vars::MOONSHOT_BASE_URL),
            ),
            model: model.trim().to_string(),
        }
    }

    fn convert_to_moonshot_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
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
                        message: "Invalid temperature value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }

        // Add reasoning_effort for Kimi K2 Thinking model
        if let Some(effort) = request.reasoning_effort
            && self.supports_reasoning_effort(&request.model)
        {
            payload.insert(
                "reasoning_effort".to_string(),
                Value::String(effort.as_str().to_string()),
            );
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
        }

        // Add tools if present (Moonshot supports function calling)
        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = super::common::serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_string(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        Ok(Value::Object(payload))
    }
}

#[async_trait]
impl LLMProvider for MoonshotProvider {
    fn name(&self) -> &str {
        "moonshot"
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        model.contains("k2-thinking") || model.contains("kimi-k2-thinking")
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        model.contains("k2-thinking") || model.contains("kimi-k2-thinking")
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        ensure_model(&mut request, &self.model);
        request.model = request.model.trim().to_string();
        let model = request.model.clone();

        let payload = self.convert_to_moonshot_format(&request)?;
        let url = chat_completions_url(&self.base_url);

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
        let response_json = parse_json_response(response, PROVIDER_NAME).await?;

        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            PROVIDER_NAME,
            model,
            false,
            None,
        )
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        ensure_model(&mut request, &self.model);
        request.model = request.model.trim().to_string();
        let model = request.model.clone();

        self.validate_request(&request)?;
        request.stream = true;

        let payload = self.convert_to_moonshot_format(&request)?;
        let url = chat_completions_url(&self.base_url);

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

        Ok(spawn_openai_compatible_stream(
            response,
            PROVIDER_NAME,
            model,
            &["reasoning_content"],
            super::shared::OpenAiDeltaOrder::ReasoningFirst,
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        models::moonshot::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        // Moonshot publishes new official aliases and preview slugs faster than VT Code's
        // curated picker list is refreshed, so let the upstream API be the source of truth
        // for model identifiers and keep local validation focused on request shape.
        super::common::validate_request_common(request, PROVIDER_NAME, PROVIDER_KEY, None)
    }
}

impl_llm_client!(MoonshotProvider);
