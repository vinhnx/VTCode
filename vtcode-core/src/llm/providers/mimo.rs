use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::config::models::{MiMoAuthMethod, detect_mimo_auth_method};
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};

use super::{
    common::{
        chat_completions_url, ensure_model, extract_prompt_cache_settings_default, impl_llm_client,
        override_base_url, parse_json_response, parse_response_openai_format, resolve_model,
        send_chat_completions, serialize_messages_openai_format, serialize_tools_openai_format,
        spawn_openai_compatible_stream, validate_supported_models,
    },
    error_handling::handle_openai_http_error,
    extract_reasoning_trace,
};

const PROVIDER_NAME: &str = "Xiaomi MiMo";
const PROVIDER_KEY: &str = "mimo";

pub struct MiMoProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    model_behavior: Option<ModelConfig>,
    auth_method: MiMoAuthMethod,
}

impl MiMoProvider {
    pub fn new(api_key: String) -> Self {
        let auth_method = detect_mimo_auth_method(&api_key, None);
        Self::with_model_internal(
            api_key,
            models::mimo::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
            None,
            auth_method,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        let auth_method = detect_mimo_auth_method(&api_key, None);
        Self::with_model_internal(
            api_key,
            model,
            None,
            None,
            TimeoutsConfig::default(),
            None,
            auth_method,
        )
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        let auth_method = detect_mimo_auth_method(&api_key, Some(&base_url));
        Self {
            api_key,
            http_client,
            base_url,
            model,
            prompt_cache_enabled: false,
            model_behavior: None,
            auth_method,
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
        let auth_method = detect_mimo_auth_method(&api_key_value, base_url.as_deref());

        Self::with_model_internal(
            api_key_value,
            resolve_model(model, models::mimo::DEFAULT_MODEL),
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
            model_behavior,
            auth_method,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
        auth_method: MiMoAuthMethod,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let (prompt_cache_enabled, _) =
            extract_prompt_cache_settings_default(prompt_cache, PROVIDER_KEY);

        let default_base = auth_method.api_base();
        let env_var = match auth_method {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => env_vars::MIMO_BASE_URL,
            MiMoAuthMethod::TokenPlan => env_vars::MIMO_TOKEN_PLAN_BASE_URL,
        };

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(default_base, base_url, Some(env_var)),
            model,
            prompt_cache_enabled,
            model_behavior,
            auth_method,
        }
    }

    #[must_use]
    #[inline]
    fn is_thinking_enabled(request: &LLMRequest) -> bool {
        request
            .reasoning_effort
            .is_some_and(|e| e != crate::config::types::ReasoningEffortLevel::None)
    }

    fn convert_to_mimo_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
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
                "max_completion_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        let thinking_enabled = Self::is_thinking_enabled(request);

        if !thinking_enabled {
            if let Some(temperature) = request.temperature {
                payload.insert(
                    "temperature".to_owned(),
                    Value::Number(super::common::float_to_json_number(temperature)?),
                );
            }

            if let Some(top_p) = request.top_p {
                payload.insert(
                    "top_p".to_owned(),
                    Value::Number(super::common::float_to_json_number(top_p)?),
                );
            }
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

        if let Some(effort) = request.reasoning_effort {
            if effort == crate::config::types::ReasoningEffortLevel::None {
                payload.insert(
                    "thinking".to_owned(),
                    serde_json::json!({"type": "disabled"}),
                );
            } else {
                payload.insert(
                    "thinking".to_owned(),
                    serde_json::json!({"type": "enabled"}),
                );
            }
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
        let request = self.http_client.post(&url);

        // Use different header format based on auth method
        let request = match self.auth_method {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => {
                request.header("api-key", &self.api_key)
            }
            MiMoAuthMethod::TokenPlan => {
                request.header("Authorization", format!("Bearer {}", &self.api_key))
            }
        };

        send_chat_completions(request, payload, PROVIDER_NAME).await
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        serialize_messages_openai_format(request, PROVIDER_KEY)
    }

    fn parse_response(&self, response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let reasoning_extractor = |message: &Value, choice: &Value| {
            message
                .get("reasoning_content")
                .and_then(extract_reasoning_trace)
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
            self.prompt_cache_enabled,
            Some(reasoning_extractor),
        )
    }
}

#[async_trait]
impl LLMProvider for MiMoProvider {
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

    fn supports_vision(&self, model: &str) -> bool {
        model == models::mimo::MIMO_V2_5
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(false)
            || requested == models::mimo::MIMO_V2_5_PRO
            || requested == models::mimo::MIMO_V2_5
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };
        match requested {
            models::mimo::MIMO_V2_5_PRO | models::mimo::MIMO_V2_5 => 1_048_576,
            _ => 128_000,
        }
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = ensure_model(&mut request, &self.model);

        let payload = self.convert_to_mimo_format(&request)?;
        let response = self.send_request(&payload).await?;
        let env_key = self.auth_method.env_key();
        let response = handle_openai_http_error(response, PROVIDER_NAME, env_key).await?;

        let response_json = parse_json_response(response, PROVIDER_NAME).await?;
        self.parse_response(response_json, model)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.convert_to_mimo_format(&request)?;
        let response = self.send_request(&payload).await?;
        let env_key = self.auth_method.env_key();
        let response = handle_openai_http_error(response, PROVIDER_NAME, env_key).await?;

        Ok(spawn_openai_compatible_stream(
            response,
            PROVIDER_NAME,
            model,
            &["reasoning_content"],
            super::shared::OpenAiDeltaOrder::ReasoningFirst,
            true,
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        let model_list = match self.auth_method {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => models::mimo::PAYG_MODELS,
            MiMoAuthMethod::TokenPlan => models::mimo::TOKEN_PLAN_MODELS,
        };
        model_list.iter().map(|model| model.to_string()).collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let model_list = match self.auth_method {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => models::mimo::PAYG_MODELS,
            MiMoAuthMethod::TokenPlan => models::mimo::TOKEN_PLAN_MODELS,
        };
        validate_supported_models(request, PROVIDER_NAME, PROVIDER_KEY, model_list)
    }
}

impl_llm_client!(MiMoProvider);
