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
    AnthropicConfig, ModelConfig, OpenAIConfig, OpenAIHostedShellConfig, OpenAIPromptCacheSettings,
    OpenAIServiceTier, PromptCachingConfig,
};
use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::provider::LLMProvider;
use crate::models_manager::model_family::find_family_for_model;
use crate::utils::file_input::{MAX_INPUT_FILE_BYTES, decoded_base64_size};
use hashbrown::{HashMap, HashSet};
use reqwest::Client as HttpClient;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::env;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::sync::Mutex as AsyncMutex;
use tracing::debug;
use uuid::Uuid;
use vtcode_config::auth::{OpenAIChatGptAuthHandle, OpenAIChatGptSession};

// Import from extracted modules
use super::harmony;
use super::request_builder;
use super::response_parser;
use super::responses_api::parse_responses_payload;
use super::types::{MAX_COMPLETION_TOKENS_FIELD, OpenAIResponsesPayload, ResponsesApiState};

mod generation;
mod streaming;
mod websocket;

use self::websocket::{OpenAIResponsesWebSocketContinuationCache, OpenAIResponsesWebSocketSession};
use super::super::{
    common::{
        extract_prompt_cache_settings, override_base_url, parse_client_prompt_common, resolve_model,
    },
    extract_reasoning_trace,
};
use crate::prompts::system::default_system_prompt;

const CHATGPT_CODEX_BASE: &str = "https://chatgpt.com/backend-api/codex";
const CHATGPT_ACCOUNT_HEADER: &str = "ChatGPT-Account-Id";
const CHATGPT_ORIGINATOR_HEADER: &str = "originator";
const CHATGPT_ORIGINATOR_VALUE: &str = "codex_cli_rs";
const CHATGPT_SESSION_HEADER: &str = "session_id";
const CHATGPT_USER_AGENT: &str = "VT Code/1.0";
const INLINE_FILE_LIMIT_ERROR_PREFIX: &str =
    "Inline OpenAI input_file payload exceeds the 50 MB request limit";

#[derive(Clone, Debug)]
struct OpenAIRequestAuth {
    bearer_token: String,
    chatgpt_account_id: Option<String>,
}

pub struct OpenAIProvider {
    api_key: Arc<str>,
    /// Override provider key for custom providers (e.g., "mycorp").
    /// When `None`, defaults to `"openai"`.
    provider_key_override: Option<Arc<str>>,
    /// Override display name for custom providers.
    /// When `None`, defaults to `"OpenAI"`.
    provider_display_override: Option<Arc<str>>,
    openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    http_client: HttpClient,
    base_url: Arc<str>,
    model: Arc<str>,
    responses_api_modes: Mutex<HashMap<String, ResponsesApiState>>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenAIPromptCacheSettings,
    model_behavior: Option<ModelConfig>,
    websocket_mode: bool,
    responses_store: Option<bool>,
    responses_include: Vec<String>,
    service_tier: Option<OpenAIServiceTier>,
    hosted_shell: OpenAIHostedShellConfig,
    websocket_session: AsyncMutex<Option<OpenAIResponsesWebSocketSession>>,
    websocket_continuation_cache: Mutex<Option<OpenAIResponsesWebSocketContinuationCache>>,
}

impl OpenAIProvider {
    fn requires_streaming_responses(model: &str) -> bool {
        matches!(
            model,
            models::openai::GPT | models::openai::GPT_5_4 | models::openai::GPT_5_4_PRO
        )
    }

    fn model_supports_reasoning_summaries(model: &str) -> bool {
        find_family_for_model(model).supports_reasoning_summaries
    }

    fn normalize_reasoning_output(
        model: &str,
        mut response: provider::LLMResponse,
    ) -> provider::LLMResponse {
        if !Self::model_supports_reasoning_summaries(model) {
            response.reasoning = None;
            response.reasoning_details = None;
        }

        response
    }

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
            None,
            models::openai::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(
            api_key,
            None,
            model,
            None,
            None,
            TimeoutsConfig::default(),
            None,
            None,
        )
    }

    pub fn new_with_client(
        api_key: String,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        use hashbrown::HashMap;
        use std::sync::Arc;
        use std::sync::Mutex;

        Self {
            api_key: Arc::from(api_key.as_str()),
            provider_key_override: None,
            provider_display_override: None,
            openai_chatgpt_auth,
            http_client,
            base_url: Arc::from(base_url.as_str()),
            model: Arc::from(model.as_str()),
            prompt_cache_enabled: false,
            prompt_cache_settings: Default::default(),
            responses_api_modes: Mutex::new(HashMap::new()),
            model_behavior: None,
            websocket_mode: false,
            responses_store: None,
            responses_include: Vec::new(),
            service_tier: None,
            hosted_shell: OpenAIHostedShellConfig::default(),
            websocket_session: AsyncMutex::new(None),
            websocket_continuation_cache: Mutex::new(None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_config(
        api_key: Option<String>,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        openai: Option<OpenAIConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openai::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            openai_chatgpt_auth,
            model_value,
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
            openai,
            model_behavior,
        )
    }

    /// Create a custom OpenAI-compatible provider with overridden identity.
    #[allow(clippy::too_many_arguments)]
    pub fn from_custom_config(
        provider_key: String,
        display_name: String,
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        openai: Option<OpenAIConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let mut provider = Self::from_config(
            api_key,
            None, // no chatgpt auth for custom providers
            model,
            base_url,
            prompt_cache,
            timeouts,
            None, // no anthropic config
            openai,
            model_behavior,
        );
        provider.provider_key_override = Some(Arc::from(provider_key.as_str()));
        provider.provider_display_override = Some(Arc::from(display_name.as_str()));
        provider
    }

    fn with_model_internal(
        api_key: String,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
        openai: Option<OpenAIConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openai,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        let using_chatgpt_auth = openai_chatgpt_auth.is_some();
        let resolved_base_url = override_base_url(
            if using_chatgpt_auth {
                CHATGPT_CODEX_BASE
            } else {
                urls::OPENAI_API_BASE
            },
            base_url,
            Some(env_vars::OPENAI_BASE_URL),
        );

        let mut responses_api_modes = HashMap::new();
        let default_state = Self::default_responses_state(&model);
        let is_native_openai = resolved_base_url.contains("api.openai.com");
        let is_chatgpt_backend = using_chatgpt_auth && resolved_base_url.contains("chatgpt.com");
        let is_xai = resolved_base_url.contains("api.x.ai");
        let websocket_mode = openai
            .as_ref()
            .map(|cfg| cfg.websocket_mode)
            .unwrap_or(false);
        let responses_store = openai.as_ref().and_then(|cfg| cfg.responses_store);
        let responses_include = openai
            .as_ref()
            .map(|cfg| {
                cfg.responses_include
                    .iter()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let service_tier = openai.as_ref().and_then(|cfg| cfg.service_tier);
        let hosted_shell = openai
            .as_ref()
            .map(|cfg| cfg.hosted_shell.clone())
            .unwrap_or_default();

        let initial_state = if is_xai {
            ResponsesApiState::Disabled
        } else if is_chatgpt_backend {
            match default_state {
                ResponsesApiState::Disabled => ResponsesApiState::Allowed,
                state => state,
            }
        } else if !is_native_openai {
            ResponsesApiState::Disabled
        } else {
            default_state
        };
        responses_api_modes.insert(model.clone(), initial_state);

        use crate::llm::http_client::HttpClientFactory;
        let http_client = HttpClientFactory::for_llm(&timeouts);

        Self {
            api_key: Arc::from(api_key.as_str()),
            provider_key_override: None,
            provider_display_override: None,
            openai_chatgpt_auth,
            http_client,
            base_url: Arc::from(resolved_base_url.as_str()),
            model: Arc::from(model.as_str()),
            responses_api_modes: Mutex::new(responses_api_modes),
            prompt_cache_enabled,
            prompt_cache_settings,
            model_behavior,
            websocket_mode,
            responses_store,
            responses_include,
            service_tier,
            hosted_shell,
            websocket_session: AsyncMutex::new(None),
            websocket_continuation_cache: Mutex::new(None),
        }
    }

    fn websocket_mode_enabled(&self, model: &str) -> bool {
        self.websocket_mode
            && self.base_url.contains("api.openai.com")
            && !matches!(self.responses_api_state(model), ResponsesApiState::Disabled)
    }

    fn hosted_shell_for_model(&self, model: &str) -> Option<&OpenAIHostedShellConfig> {
        (self.base_url.contains("api.openai.com")
            && !matches!(self.responses_api_state(model), ResponsesApiState::Disabled)
            && self.hosted_shell.enabled
            && self.hosted_shell.is_valid_for_runtime())
        .then_some(&self.hosted_shell)
    }

    fn authorize_with_api_key(
        &self,
        builder: reqwest::RequestBuilder,
        auth: &OpenAIRequestAuth,
    ) -> reqwest::RequestBuilder {
        let mut builder = if auth.bearer_token.trim().is_empty() {
            builder
        } else {
            builder.bearer_auth(&auth.bearer_token)
        };

        if self.is_chatgpt_backend() {
            if let Some(account_id) = auth
                .chatgpt_account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                builder = builder.header(CHATGPT_ACCOUNT_HEADER, account_id);
            }
            builder = builder
                .header(CHATGPT_ORIGINATOR_HEADER, CHATGPT_ORIGINATOR_VALUE)
                .header("User-Agent", CHATGPT_USER_AGENT);
            if let Ok(session_id) = env::var("VT_SESSION_ID")
                && !session_id.trim().is_empty()
            {
                builder = builder.header(CHATGPT_SESSION_HEADER, session_id);
            }
        }

        builder
    }

    fn uses_chatgpt_auth(&self) -> bool {
        self.openai_chatgpt_auth.is_some()
    }

    fn is_chatgpt_backend(&self) -> bool {
        self.uses_chatgpt_auth() && self.base_url.contains("chatgpt.com")
    }

    fn allows_chat_completions_fallback(&self) -> bool {
        !self.is_chatgpt_backend()
    }

    fn auth_retryable_status(status: StatusCode) -> bool {
        matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    }

    fn new_client_request_id() -> String {
        format!("vtcode-{}", Uuid::new_v4())
    }

    fn format_network_error(&self, error: impl std::fmt::Display) -> provider::LLMError {
        let label = self
            .provider_display_override
            .as_deref()
            .unwrap_or("OpenAI");
        provider::LLMError::Network {
            message: error_display::format_llm_error(label, &format!("Network error: {error}")),
            metadata: None,
        }
    }

    fn format_auth_error(&self, error: impl std::fmt::Display) -> provider::LLMError {
        let label = self
            .provider_display_override
            .as_deref()
            .unwrap_or("OpenAI");
        provider::LLMError::Authentication {
            message: error_display::format_llm_error(
                label,
                &format!("Authentication error: {error}"),
            ),
            metadata: None,
        }
    }

    async fn current_api_key(&self) -> Result<String, provider::LLMError> {
        let Some(handle) = &self.openai_chatgpt_auth else {
            return Ok(self.api_key.to_string());
        };

        handle
            .refresh_if_needed()
            .await
            .map_err(|e| self.format_auth_error(e))?;
        handle
            .current_api_key()
            .map_err(|e| self.format_auth_error(e))
    }

    fn request_auth_from_session(&self, session: OpenAIChatGptSession) -> OpenAIRequestAuth {
        let bearer_token = if self.is_chatgpt_backend() || session.openai_api_key.trim().is_empty()
        {
            session.access_token
        } else {
            session.openai_api_key
        };

        OpenAIRequestAuth {
            bearer_token,
            chatgpt_account_id: session.account_id,
        }
    }

    async fn current_request_auth(&self) -> Result<OpenAIRequestAuth, provider::LLMError> {
        let Some(handle) = &self.openai_chatgpt_auth else {
            return Ok(OpenAIRequestAuth {
                bearer_token: self.api_key.to_string(),
                chatgpt_account_id: None,
            });
        };

        handle
            .refresh_if_needed()
            .await
            .map_err(|e| self.format_auth_error(e))?;
        let session = handle.snapshot().map_err(|e| self.format_auth_error(e))?;
        Ok(self.request_auth_from_session(session))
    }

    async fn refresh_request_auth_for_retry(
        &self,
    ) -> Result<OpenAIRequestAuth, provider::LLMError> {
        let Some(handle) = &self.openai_chatgpt_auth else {
            return Ok(OpenAIRequestAuth {
                bearer_token: self.api_key.to_string(),
                chatgpt_account_id: None,
            });
        };

        handle
            .force_refresh()
            .await
            .map_err(|e| self.format_auth_error(e))?;
        let session = handle.snapshot().map_err(|e| self.format_auth_error(e))?;
        Ok(self.request_auth_from_session(session))
    }

    async fn refresh_api_key_for_retry(&self) -> Result<String, provider::LLMError> {
        let Some(handle) = &self.openai_chatgpt_auth else {
            return Ok(self.api_key.to_string());
        };

        handle
            .force_refresh()
            .await
            .map_err(|e| self.format_auth_error(e))?;
        handle
            .current_api_key()
            .map_err(|e| self.format_auth_error(e))
    }

    async fn send_authorized<F>(
        &self,
        build_request: F,
    ) -> Result<reqwest::Response, provider::LLMError>
    where
        F: Fn(&OpenAIRequestAuth) -> reqwest::RequestBuilder,
    {
        let auth = self.current_request_auth().await?;
        let response = build_request(&auth)
            .send()
            .await
            .map_err(|e| self.format_network_error(e))?;

        if self.uses_chatgpt_auth() && Self::auth_retryable_status(response.status()) {
            let retry_auth = self.refresh_request_auth_for_retry().await?;
            return build_request(&retry_auth)
                .send()
                .await
                .map_err(|e| self.format_network_error(e));
        }

        Ok(response)
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

    fn validate_inline_file_inputs(
        request: &provider::LLMRequest,
    ) -> Result<(), provider::LLMError> {
        Self::validate_inline_file_inputs_with_limit(request, MAX_INPUT_FILE_BYTES)
    }

    fn validate_inline_file_inputs_with_limit(
        request: &provider::LLMRequest,
        max_inline_file_bytes: u64,
    ) -> Result<(), provider::LLMError> {
        let mut total_inline_file_bytes = 0u64;

        for message in &request.messages {
            let provider::MessageContent::Parts(parts) = &message.content else {
                continue;
            };

            for part in parts {
                let provider::ContentPart::File {
                    filename,
                    file_data,
                    ..
                } = part
                else {
                    continue;
                };
                let Some(file_data) = file_data else {
                    continue;
                };

                let inline_file_bytes = decoded_base64_size(file_data).map_err(|error| {
                    let formatted = error_display::format_llm_error(
                        "OpenAI",
                        &format!("Invalid inline input_file payload: {error}"),
                    );
                    provider::LLMError::InvalidRequest {
                        message: formatted,
                        metadata: None,
                    }
                })?;

                if inline_file_bytes > max_inline_file_bytes {
                    let file_label = filename.as_deref().unwrap_or("attached file");
                    let formatted = error_display::format_llm_error(
                        "OpenAI",
                        &format!(
                            "{INLINE_FILE_LIMIT_ERROR_PREFIX}: '{file_label}' is {} bytes",
                            inline_file_bytes
                        ),
                    );
                    return Err(provider::LLMError::InvalidRequest {
                        message: formatted,
                        metadata: None,
                    });
                }

                total_inline_file_bytes = total_inline_file_bytes
                    .checked_add(inline_file_bytes)
                    .ok_or_else(|| provider::LLMError::InvalidRequest {
                        message: error_display::format_llm_error(
                            "OpenAI",
                            INLINE_FILE_LIMIT_ERROR_PREFIX,
                        ),
                        metadata: None,
                    })?;
            }
        }

        if total_inline_file_bytes > max_inline_file_bytes {
            let formatted = error_display::format_llm_error(
                "OpenAI",
                &format!(
                    "{INLINE_FILE_LIMIT_ERROR_PREFIX}: total inline file bytes = {}",
                    total_inline_file_bytes
                ),
            );
            return Err(provider::LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }

        Ok(())
    }

    fn convert_to_openai_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        let is_native_openai = self.base_url.contains("api.openai.com");
        let prompt_cache_key = if is_native_openai {
            request.prompt_cache_key.as_deref()
        } else {
            None
        };
        let default_service_tier = if is_native_openai {
            self.service_tier.map(OpenAIServiceTier::as_str)
        } else {
            None
        };
        let ctx = request_builder::ChatRequestContext {
            model: &self.model,
            base_url: &self.base_url,
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
            prompt_cache_key,
            default_service_tier,
        };

        request_builder::build_chat_request(request, &ctx)
    }

    fn convert_to_openai_responses_format(
        &self,
        request: &provider::LLMRequest,
    ) -> Result<Value, provider::LLMError> {
        Self::validate_inline_file_inputs(request)?;

        let is_native_openai = self.base_url.contains("api.openai.com");
        let prompt_cache_key = if is_native_openai {
            request.prompt_cache_key.as_deref()
        } else {
            None
        };
        let default_service_tier = if is_native_openai {
            self.service_tier.map(OpenAIServiceTier::as_str)
        } else {
            None
        };
        let is_chatgpt_backend = self.is_chatgpt_backend();
        let ctx = request_builder::ResponsesRequestContext {
            supports_tools: self.supports_tools(&request.model),
            supports_parallel_tool_config: self.supports_parallel_tool_config(&request.model),
            supports_temperature: Self::supports_temperature_parameter(&request.model),
            supports_reasoning_effort: self.supports_reasoning_effort(&request.model),
            supports_reasoning: self.supports_reasoning(&request.model),
            is_responses_api_model: Self::is_responses_api_model(&request.model),
            include_max_output_tokens: is_native_openai,
            include_previous_response_id: is_native_openai,
            include_output_types: !self.is_chatgpt_backend(),
            include_sampling_parameters: !self.is_chatgpt_backend(),
            force_response_store_false: self.uses_chatgpt_auth()
                && self.base_url.contains("chatgpt.com"),
            include_assistant_phase: is_native_openai,
            prompt_cache_key,
            include_prompt_cache_retention: !self.is_chatgpt_backend(),
            prompt_cache_retention: self.prompt_cache_settings.prompt_cache_retention.as_deref(),
            default_service_tier,
            default_response_store: self.responses_store,
            default_responses_include: (!self.responses_include.is_empty())
                .then_some(self.responses_include.as_slice()),
            hosted_shell: self.hosted_shell_for_model(&request.model),
            include_structured_history_in_input: !is_chatgpt_backend,
            preserve_structured_history_on_replay: is_chatgpt_backend,
            preserve_assistant_phase_on_replay: false,
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
        let response = response_parser::parse_chat_response(
            response_json,
            model.clone(),
            include_cached_prompt_tokens,
        )?;
        Ok(Self::normalize_reasoning_output(&model, response))
    }

    fn parse_openai_responses_response(
        &self,
        response_json: Value,
        model: String,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        let response = parse_responses_payload(response_json, model.clone(), include_metrics)?;
        Ok(Self::normalize_reasoning_output(&model, response))
    }
}

#[cfg(test)]
mod tests;

mod harmony_client;
mod provider_impl;
