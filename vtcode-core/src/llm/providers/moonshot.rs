use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::config::models::Provider as ModelProvider;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse};
use crate::llm::providers::common::{
    convert_usage_to_llm_types, forward_prompt_cache_with_state, handle_http_error,
    make_default_request, override_base_url, parse_response_openai_format, resolve_model,
    serialize_messages_openai_format, serialize_tools_openai_format, validate_request_common,
};
use crate::llm::rig_adapter::reasoning_parameters_for;
use crate::llm::types as llm_types;
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
            models::moonshot::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, models::moonshot::DEFAULT_MODEL);
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
        Self::from_config(Some(api_key), Some(model), None, prompt_cache, timeouts)
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
        if let Some(tools) = &request.tools {
            if let Some(serialized_tools) = serialize_tools_openai_format(tools) {
                payload.insert("tools".to_string(), Value::Array(serialized_tools));

                // Add tool choice if specified
                if let Some(choice) = &request.tool_choice {
                    payload.insert(
                        "tool_choice".to_string(),
                        choice.to_provider_format(PROVIDER_KEY),
                    );
                }
            }
        }

        // Handle reasoning effort for Kimi-K2-Thinking model
        if let Some(effort) = request.reasoning_effort {
            if self.supports_reasoning_effort(&request.model) {
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
        }

        // Apply Heavy Mode configuration specifically for the heavy model variant
        if request.model == models::moonshot::KIMI_K2_THINKING_TURBO {
            // Override or add Heavy Mode specific parameters
            payload.insert("heavy_thinking".to_string(), Value::Bool(true));
            payload.insert(
                "parallel_trajectories".to_string(),
                Value::Number(serde_json::Number::from(8)),
            );
            payload.insert(
                "trajectory_aggregation".to_string(),
                Value::String("reflective".to_string()),
            );
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

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        requested == models::moonshot::KIMI_K2_THINKING
            || requested == models::moonshot::KIMI_K2_THINKING_TURBO
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        requested == models::moonshot::KIMI_K2_THINKING
            || requested == models::moonshot::KIMI_K2_THINKING_TURBO
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        // Convert request to Moonshot-specific format
        let payload = self.convert_to_moonshot_format(&request)?;
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
                LLMError::Network(formatted_error)
            })?;

        let response = handle_http_error(response, PROVIDER_NAME, "MOONSHOT_API_KEY").await?;

        let response_json: Value = response.json().await.map_err(|e| {
            let formatted_error = error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("Failed to parse response: {}", e),
            );
            LLMError::Provider(formatted_error)
        })?;

        self.parse_response(response_json)
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
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Moonshot
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
