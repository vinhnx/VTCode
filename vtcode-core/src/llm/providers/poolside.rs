use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};

use super::{
    common::{
        chat_completions_url, ensure_model, extract_prompt_cache_settings_default, impl_llm_client,
        override_base_url, parse_json_response, parse_response_openai_format, resolve_model,
        send_chat_completions, serialize_messages_openai_format, serialize_tools_openai_format,
        spawn_openai_compatible_stream, validate_supported_models,
    },
    error_handling::handle_openai_http_error,
};

const PROVIDER_NAME: &str = "Poolside";
const PROVIDER_KEY: &str = "poolside";

pub struct PoolsideProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    #[allow(dead_code)]
    model_behavior: Option<ModelConfig>,
}

impl PoolsideProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::poolside::DEFAULT_MODEL.to_string(),
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
        let api_key_value = api_key
            .filter(|k| !k.trim().is_empty())
            .or_else(|| {
                std::env::var("POOLSIDE_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
            })
            .unwrap_or_default();

        Self::with_model_internal(
            api_key_value,
            resolve_model(model, models::poolside::DEFAULT_MODEL),
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

        let (prompt_cache_enabled, _) =
            extract_prompt_cache_settings_default(prompt_cache, PROVIDER_KEY);

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::POOLSIDE_API_BASE,
                base_url,
                Some(env_vars::POOLSIDE_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            model_behavior,
        }
    }

    fn convert_to_poolside_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::with_capacity(12);

        payload.insert("model".to_owned(), Value::String(request.model.clone()));

        let mut messages = self.serialize_messages(request)?;

        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                messages.insert(0, serde_json::json!({"role": "system", "content": trimmed}));
            }
        }

        payload.insert("messages".to_owned(), Value::Array(messages));

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_owned(),
                Value::Number(serde_json::Number::from_f64(temperature as f64).ok_or(
                    LLMError::InvalidRequest {
                        message: "invalid temperature value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }

        if let Some(top_p) = request.top_p {
            payload.insert(
                "top_p".to_owned(),
                Value::Number(serde_json::Number::from_f64(top_p as f64).ok_or(
                    LLMError::InvalidRequest {
                        message: "invalid top_p value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
            payload.insert(
                "stream_options".to_string(),
                serde_json::json!({"include_usage": true}),
            );
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

        if let Some(meta) = &request.metadata
            && let Some(user_id) = meta.get("user_id").and_then(|v| v.as_str())
        {
            payload.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
        }

        Ok(Value::Object(payload))
    }

    async fn send_request(&self, payload: &Value) -> Result<reqwest::Response, LLMError> {
        let url = chat_completions_url(&self.base_url);
        send_chat_completions(
            self.http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key)),
            payload,
            PROVIDER_NAME,
        )
        .await
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        serialize_messages_openai_format(request, PROVIDER_KEY)
    }

    fn parse_response(&self, response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let reasoning_extractor = |_message: &Value, _choice: &Value| -> Option<String> { None };

        parse_response_openai_format(
            response_json,
            PROVIDER_NAME,
            model,
            self.prompt_cache_enabled,
            Some(reasoning_extractor),
        )
    }
}

#[async_trait]
impl LLMProvider for PoolsideProvider {
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
        false
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        131_072
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = ensure_model(&mut request, &self.model);

        let payload = self.convert_to_poolside_format(&request)?;
        let response = self.send_request(&payload).await?;
        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "POOLSIDE_API_KEY").await?;

        let response_json = parse_json_response(response, PROVIDER_NAME).await?;
        self.parse_response(response_json, model)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.convert_to_poolside_format(&request)?;
        let response = self.send_request(&payload).await?;
        let response =
            handle_openai_http_error(response, PROVIDER_NAME, "POOLSIDE_API_KEY").await?;

        Ok(spawn_openai_compatible_stream(
            response,
            PROVIDER_NAME,
            model,
            &[],
            super::shared::OpenAiDeltaOrder::ContentFirst,
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        models::poolside::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_supported_models(
            request,
            PROVIDER_NAME,
            PROVIDER_KEY,
            models::poolside::SUPPORTED_MODELS,
        )
    }
}

impl_llm_client!(PoolsideProvider);
