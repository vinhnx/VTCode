//! OpenResponses provider setup and transport routing.

use crate::error_display;
use crate::provider::LLMError;
use crate::providers::error_handling::format_network_error;
use anyhow::Result;
use reqwest::Client as HttpClient;
use serde_json::Value;
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};

use crate::providers::common::{override_base_url, resolve_model};

mod compaction;
mod legacy;
mod native;
mod provider_impl;

#[cfg(test)]
use crate::provider::{LLMProvider, LLMRequest, LLMStreamEvent, Message, ResponsesCompactionOptions, ToolCall};
#[cfg(test)]
use serde_json::json;

pub struct OpenResponsesProvider {
    http_client: HttpClient,
    base_url: String,
    model: String,
    api_key: String,
    model_behavior: Option<ModelConfig>,
}

impl OpenResponsesProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::openresponses::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(model, None, api_key, TimeoutsConfig::default(), None)
    }

    fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            http_client,
            base_url,
            model,
            api_key,
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
        let api_key_value = api_key.unwrap_or_default();
        let resolved_model = resolve_model(model, models::openresponses::DEFAULT_MODEL);
        Self::with_model_internal(resolved_model, base_url, api_key_value, timeouts.unwrap_or_default(), model_behavior)
    }

    fn with_model_internal(
        model: String,
        base_url: Option<String>,
        api_key: String,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::http_client::HttpClientFactory;

        Self {
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(urls::OPENRESPONSES_API_BASE, base_url, Some(env_vars::OPENRESPONSES_BASE_URL)),
            model,
            api_key,
            model_behavior,
        }
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url.trim_end_matches('/'))
    }

    /// Leave missing-endpoint responses to the caller's legacy adapter.
    async fn send_native_request(&self, payload: &Value) -> Result<reqwest::Response, LLMError> {
        let response = self
            .http_client
            .post(self.responses_url())
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error("OpenResponses", &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        Ok(response)
    }

    fn responses_compact_url(&self) -> String {
        format!("{}/responses/compact", self.base_url.trim_end_matches('/'))
    }

    /// Client for driving another endpoint's OpenAI-compatible
    /// `/responses/compact` surface (e.g. Vercel AI Gateway OpenAI routes, xAI
    /// Grok models) with this provider's transport and response parsing.
    /// Capability gating stays with the caller: only construct this for routes
    /// whose upstream documents the endpoint.
    pub(crate) fn compact_endpoint_client(configured_model: &str, base_url: &str, api_key: &str, model: &str) -> Self {
        let resolved = if model.trim().is_empty() {
            configured_model.to_string()
        } else {
            model.to_string()
        };
        Self::from_config(Some(api_key.to_string()), Some(resolved), Some(base_url.to_string()), None, None, None, None)
    }

    fn supports_compaction_endpoint(&self) -> bool {
        self.base_url.contains("api.openai.com") || self.base_url.contains("api.openresponses.com")
    }
}

#[cfg(test)]
mod tests;
