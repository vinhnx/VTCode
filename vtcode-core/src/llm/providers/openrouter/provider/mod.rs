#![allow(clippy::collapsible_if, clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, OpenRouterPromptCacheSettings, PromptCachingConfig};
use crate::config::models::ModelId;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMRequest, Message, MessageRole, ToolChoice};
use crate::llm::providers::common::{
    extract_prompt_cache_settings, override_base_url, resolve_model,
};
use reqwest::{Client as HttpClient, Response, StatusCode};
use serde_json::Value;
use std::borrow::Cow;
use std::str::FromStr;

mod client_impl;
mod parsing;
mod provider_impl;
#[cfg(test)]
mod tests;

pub struct OpenRouterProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenRouterPromptCacheSettings,
}

impl OpenRouterProvider {
    const TOOL_UNSUPPORTED_ERROR: &'static str = "No endpoints found that support tool use";

    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openrouter::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, TimeoutsConfig::default())
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
            prompt_cache_settings: OpenRouterPromptCacheSettings::default(),
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = resolve_model(model, models::openrouter::DEFAULT_MODEL);

        Self::with_model_internal(
            api_key_value,
            model_value,
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openrouter,
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::OPENROUTER_API_BASE,
                base_url,
                Some(env_vars::OPENROUTER_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    pub(super) fn is_gpt5_codex_model(model: &str) -> bool {
        model == models::openrouter::OPENAI_GPT_5_CODEX
    }

    pub(super) fn resolve_model<'a>(&'a self, request: &'a LLMRequest) -> &'a str {
        if request.model.trim().is_empty() {
            self.model.as_str()
        } else {
            request.model.as_str()
        }
    }

    fn uses_responses_api_for(&self, request: &LLMRequest) -> bool {
        Self::is_gpt5_codex_model(self.resolve_model(request))
    }

    fn request_includes_tools(request: &LLMRequest) -> bool {
        request
            .tools
            .as_ref()
            .map(|tools| !tools.is_empty())
            .unwrap_or(false)
    }

    fn enforce_tool_capabilities<'a>(&'a self, request: &'a LLMRequest) -> Cow<'a, LLMRequest> {
        let resolved_model = self.resolve_model(request);
        let tools_requested = Self::request_includes_tools(request);
        let tool_restricted = if let Ok(model_id) = ModelId::from_str(resolved_model) {
            !model_id.supports_tool_calls()
        } else {
            models::openrouter::TOOL_UNAVAILABLE_MODELS.contains(&resolved_model)
        };

        if tools_requested && tool_restricted {
            Cow::Owned(Self::tool_free_request(request))
        } else {
            Cow::Borrowed(request)
        }
    }

    fn tool_free_request(original: &LLMRequest) -> LLMRequest {
        let mut sanitized = original.clone();
        sanitized.tools = None;
        sanitized.tool_choice = Some(ToolChoice::None);
        sanitized.parallel_tool_calls = None;
        sanitized.parallel_tool_config = None;

        let mut normalized_messages: Vec<Message> = Vec::with_capacity(original.messages.len());

        for message in &original.messages {
            match message.role {
                MessageRole::Assistant => {
                    let mut cleaned = message.clone();
                    cleaned.tool_calls = None;
                    cleaned.tool_call_id = None;

                    let content_text = cleaned.content.as_text();
                    let has_content = !content_text.trim().is_empty();
                    if has_content || cleaned.reasoning.is_some() {
                        normalized_messages.push(cleaned);
                    }
                }
                MessageRole::Tool => {
                    let content_text = message.content.as_text();
                    if content_text.trim().is_empty() {
                        continue;
                    }

                    let mut converted = Message::user(content_text.into_owned());
                    converted.reasoning = message.reasoning.clone();
                    normalized_messages.push(converted);
                }
                _ => {
                    normalized_messages.push(message.clone());
                }
            }
        }

        sanitized.messages = normalized_messages;
        sanitized
    }

    fn build_provider_payload(&self, request: &LLMRequest) -> Result<(Value, String), LLMError> {
        if self.uses_responses_api_for(request) {
            Ok((
                self.convert_to_openrouter_responses_format(request)?,
                format!("{}/responses", self.base_url),
            ))
        } else {
            Ok((
                self.convert_to_openrouter_format(request)?,
                format!("{}/chat/completions", self.base_url),
            ))
        }
    }

    async fn dispatch_request(&self, url: &str, payload: &Value) -> Result<Response, LLMError> {
        self.http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", &format!("Network error: {}", e));
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })
    }

    fn is_tool_unsupported_error(status: StatusCode, body: &str) -> bool {
        status == StatusCode::NOT_FOUND && body.contains(Self::TOOL_UNSUPPORTED_ERROR)
    }

    async fn send_with_tool_fallback(
        &self,
        request: &LLMRequest,
        stream_override: Option<bool>,
    ) -> Result<Response, LLMError> {
        let adjusted_request = self.enforce_tool_capabilities(request);
        let request_ref = adjusted_request.as_ref();
        let request_with_tools = Self::request_includes_tools(request_ref);

        let (mut payload, url) = self.build_provider_payload(request_ref)?;
        if let Some(stream_flag) = stream_override {
            payload["stream"] = Value::Bool(stream_flag);
        }

        let response = self.dispatch_request(&url, &payload).await?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        if status.as_u16() == 429 || error_text.contains("quota") {
            return Err(LLMError::RateLimit { metadata: None });
        }

        if request_with_tools && Self::is_tool_unsupported_error(status, &error_text) {
            let fallback_request = Self::tool_free_request(request_ref);
            let (mut fallback_payload, fallback_url) =
                self.build_provider_payload(&fallback_request)?;
            if let Some(stream_flag) = stream_override {
                fallback_payload["stream"] = Value::Bool(stream_flag);
            }

            let fallback_response = self
                .dispatch_request(&fallback_url, &fallback_payload)
                .await?;
            if fallback_response.status().is_success() {
                return Ok(fallback_response);
            }

            let fallback_status = fallback_response.status();
            let fallback_text = fallback_response.text().await.unwrap_or_default();

            if fallback_status.as_u16() == 429 || fallback_text.contains("quota") {
                return Err(LLMError::RateLimit { metadata: None });
            }

            let combined_error = format!(
                "HTTP {}: {} | Tool fallback failed with HTTP {}: {}",
                status, error_text, fallback_status, fallback_text
            );
            let formatted_error = error_display::format_llm_error("OpenRouter", &combined_error);
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        // Use unified error parsing for consistent error categorization
        use crate::llm::providers::error_handling::parse_api_error;
        Err(parse_api_error("OpenRouter", status, &error_text))
    }
}
