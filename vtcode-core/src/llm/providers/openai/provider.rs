#![allow(
    clippy::collapsible_if,
    clippy::manual_contains,
    clippy::nonminimal_bool,
    clippy::single_match,
    clippy::result_large_err,
    unused_imports
)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{
    AnthropicConfig, ModelConfig, OpenAIConfig, OpenAIPromptCacheSettings, PromptCachingConfig,
};
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::provider::LLMProvider;
use reqwest::Client as HttpClient;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::sync::Mutex as AsyncMutex;
use tracing::debug;

// Import from extracted modules
use super::harmony;
use super::request_builder;
use super::response_parser;
use super::responses_api::parse_responses_payload;
use super::types::{MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload, ResponsesApiState};

mod generation;
mod streaming;
mod websocket;

use self::websocket::OpenAIResponsesWebSocketSession;
use super::super::{
    common::{
        extract_prompt_cache_settings, override_base_url, parse_client_prompt_common, resolve_model,
    },
    extract_reasoning_trace,
};
use crate::prompts::system::default_system_prompt;

pub struct OpenAIProvider {
    api_key: Arc<str>,
    http_client: HttpClient,
    base_url: Arc<str>,
    model: Arc<str>,
    responses_api_modes: Mutex<HashMap<String, ResponsesApiState>>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenAIPromptCacheSettings,
    model_behavior: Option<ModelConfig>,
    websocket_mode: bool,
    websocket_session: AsyncMutex<Option<OpenAIResponsesWebSocketSession>>,
}

impl OpenAIProvider {
    fn is_responses_api_model(model: &str) -> bool {
        models::openai::RESPONSES_API_MODELS.contains(&model)
    }

    fn uses_harmony(model: &str) -> bool {
        harmony::uses_harmony(model)
    }

    fn requires_responses_api(model: &str) -> bool {
        model == models::openai::GPT_5
    }

    fn default_responses_state(model: &str) -> ResponsesApiState {
        if Self::requires_responses_api(model) {
            ResponsesApiState::Required
        } else if Self::is_responses_api_model(model) {
            ResponsesApiState::Allowed
        } else {
            ResponsesApiState::Disabled
        }
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openai::DEFAULT_MODEL.to_string(),
            None,
            None,
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, None, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::Mutex;

        Self {
            api_key: Arc::from(api_key.as_str()),
            http_client,
            base_url: Arc::from(base_url.as_str()),
            model: Arc::from(model.as_str()),
            prompt_cache_enabled: false,
            prompt_cache_settings: Default::default(),
            responses_api_modes: Mutex::new(HashMap::new()),
            model_behavior: None,
            websocket_mode: false,
            websocket_session: AsyncMutex::new(None),
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        openai: Option<OpenAIConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openai::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            openai,
            model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        openai: Option<OpenAIConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openai,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let resolved_base_url = override_base_url(
            urls::OPENAI_API_BASE,
            base_url,
            Some(env_vars::OPENAI_BASE_URL),
        );

        let mut responses_api_modes = HashMap::new();
        let default_state = Self::default_responses_state(&model);
        let is_native_openai = resolved_base_url.contains("api.openai.com");
        let is_xai = resolved_base_url.contains("api.x.ai");
        let websocket_mode = openai.map(|cfg| cfg.websocket_mode).unwrap_or(false);

        let initial_state = if is_xai || !is_native_openai {
            ResponsesApiState::Disabled
        } else {
            default_state
        };
        responses_api_modes.insert(model.clone(), initial_state);

        use crate::llm::http_client::HttpClientFactory;
        let http_client =
            HttpClientFactory::with_timeouts(Duration::from_secs(120), Duration::from_secs(30));

        Self {
            api_key: Arc::from(api_key.as_str()),
            http_client,
            base_url: Arc::from(resolved_base_url.as_str()),
            model: Arc::from(model.as_str()),
            responses_api_modes: Mutex::new(responses_api_modes),
            prompt_cache_enabled,
            prompt_cache_settings,
            model_behavior,
            websocket_mode,
            websocket_session: AsyncMutex::new(None),
        }
    }

    fn websocket_mode_enabled(&self, model: &str) -> bool {
        self.websocket_mode
            && self.base_url.contains("api.openai.com")
            && !matches!(self.responses_api_state(model), ResponsesApiState::Disabled)
    }

    fn authorize(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.trim().is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.api_key)
        }
    }

    fn supports_temperature_parameter(model: &str) -> bool {
        if model == models::openai::GPT_5
            || model == models::openai::GPT_5_MINI
            || model == models::openai::GPT_5_NANO
        {
            return false;
        }
        true
    }

    fn responses_api_state(&self, model: &str) -> ResponsesApiState {
        let mut modes = match self.responses_api_modes.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("OpenAI responses_api_modes mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        *modes
            .entry(model.to_string())
            .or_insert_with(|| Self::default_responses_state(model))
    }

    fn set_responses_api_state(&self, model: &str, state: ResponsesApiState) {
        let mut modes = match self.responses_api_modes.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("OpenAI responses_api_modes mutex poisoned, recovering");
                poisoned.into_inner()
            }
        };
        modes.insert(model.to_string(), state);
    }

    fn convert_to_openai_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let is_native_openai = self.base_url.contains("api.openai.com");
        let ctx = request_builder::ChatRequestContext {
            model: &self.model,
            base_url: &self.base_url,
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
            supports_prompt_cache_key: is_native_openai,
            prompt_cache_key: request.prompt_cache_key.as_deref(),
        };

        request_builder::build_chat_request(request, &ctx)
    }

    fn convert_to_openai_responses_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let is_native_openai = self.base_url.contains("api.openai.com");
        let ctx = request_builder::ResponsesRequestContext {
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
            supports_reasoning_effort: self.supports_reasoning_effort(&request.model),
            supports_reasoning: self.supports_reasoning(&request.model),
            is_responses_api_model: Self::is_responses_api_model(&request.model),
            supports_prompt_cache_key: is_native_openai,
            prompt_cache_key: request.prompt_cache_key.as_deref(),
            prompt_cache_retention: self.prompt_cache_settings.prompt_cache_retention.as_deref(),
        };

        request_builder::build_responses_request(request, &ctx)
    }

    fn parse_openai_response(
        &self,
        response_json: Value,
        model: String,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let include_cached_prompt_tokens =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        response_parser::parse_chat_response(response_json, model, include_cached_prompt_tokens)
    }

    fn parse_openai_responses_response(
        &self,
        response_json: Value,
        model: String,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        parse_responses_payload(response_json, model, include_metrics)
    }
}

#[cfg(test)]
mod tests;

mod harmony_client;
mod provider_impl;
