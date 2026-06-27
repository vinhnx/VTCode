use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};

use super::common::{
    chat_completions_url, ensure_model, impl_llm_client, override_base_url, parse_json_response,
    parse_response_openai_format, resolve_model, send_chat_completions,
    serialize_messages_openai_format, serialize_tools_openai_format,
    spawn_openai_compatible_stream, validate_supported_models,
};
use super::error_handling::handle_openai_http_error;
use super::extract_reasoning_trace;

const PROVIDER_NAME: &str = "StepFun";
const PROVIDER_KEY: &str = "stepfun";
const PRIMARY_API_KEY_ENV: &str = "STEPFUN_API_KEY";
const LEGACY_API_KEY_ENV: &str = "STEP_API_KEY";

pub struct StepFunProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    model_behavior: Option<ModelConfig>,
}

impl StepFunProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::stepfun::DEFAULT_MODEL.to_string(),
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
        let api_key_value = api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var(PRIMARY_API_KEY_ENV).ok())
            .or_else(|| std::env::var(LEGACY_API_KEY_ENV).ok())
            .unwrap_or_default();

        Self::with_model_internal(
            api_key_value,
            resolve_model(model, models::stepfun::DEFAULT_MODEL),
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
                urls::STEPFUN_API_BASE,
                base_url,
                Some(env_vars::STEPFUN_BASE_URL),
            ),
            model,
            model_behavior,
        }
    }

    fn reasoning_effort_value(effort: ReasoningEffortLevel) -> Option<&'static str> {
        match effort {
            ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
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

    fn convert_to_stepfun_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::with_capacity(10);
        payload.insert("model".to_owned(), Value::String(request.model.clone()));

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
}

#[async_trait]
impl LLMProvider for StepFunProvider {
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
            &self.model
        } else {
            model
        };

        self.model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning)
            .unwrap_or(false)
            || models::stepfun::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        self.model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning_effort)
            .unwrap_or(false)
            || models::stepfun::REASONING_MODELS.contains(&requested)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        match requested {
            models::stepfun::STEP_3_7_FLASH => 262_144,
            _ => 262_144,
        }
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = ensure_model(&mut request, &self.model);

        let payload = self.convert_to_stepfun_format(&request)?;
        let url = chat_completions_url(&self.base_url);

        let response = send_chat_completions(
            self.http_client.post(&url).bearer_auth(&self.api_key),
            &payload,
            PROVIDER_NAME,
        )
        .await?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, PRIMARY_API_KEY_ENV).await?;
        let response_json = parse_json_response(response, PROVIDER_NAME).await?;

        let reasoning_extractor = |message: &Value, choice: &Value| {
            message
                .get("reasoning")
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
        ensure_model(&mut request, &self.model);
        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.convert_to_stepfun_format(&request)?;
        let url = chat_completions_url(&self.base_url);

        let response = send_chat_completions(
            self.http_client.post(&url).bearer_auth(&self.api_key),
            &payload,
            PROVIDER_NAME,
        )
        .await?;

        let response =
            handle_openai_http_error(response, PROVIDER_NAME, PRIMARY_API_KEY_ENV).await?;

        Ok(spawn_openai_compatible_stream(
            response,
            PROVIDER_NAME,
            model,
            &["reasoning"],
            super::shared::OpenAiDeltaOrder::ReasoningFirst,
            false,
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        models::stepfun::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_supported_models(
            request,
            PROVIDER_NAME,
            PROVIDER_KEY,
            models::stepfun::SUPPORTED_MODELS,
        )
    }
}

impl_llm_client!(StepFunProvider);

#[cfg(test)]
mod tests {
    use super::StepFunProvider;
    use crate::config::constants::models;
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::{LLMRequest, Message};

    #[test]
    fn payload_maps_reasoning_effort() {
        let provider = StepFunProvider::new("test-key".to_string());
        let payload = provider
            .convert_to_stepfun_format(&LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())],
                reasoning_effort: Some(ReasoningEffortLevel::XHigh),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert_eq!(
            payload
                .get("reasoning_effort")
                .and_then(|value| value.as_str()),
            Some("high")
        );
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());
    }
}
