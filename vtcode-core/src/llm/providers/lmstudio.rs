#![allow(clippy::result_large_err)]
//! LM Studio provider implementation
//!
//! LM Studio 0.4.0+ provides multiple API surfaces:
//! - Native v1 REST API at `/api/v1/*` (recommended for new integrations)
//! - OpenAI-compatible endpoints at `/v1/*` (used by this implementation)
//! - Anthropic-compatible endpoints at `/v1/*` (added in 0.4.1)
//!
//! This implementation currently uses OpenAI-compatible endpoints for maximum
//! compatibility. Future versions may migrate to the native v1 API for enhanced
//! features like stateful chats, MCP via API, and model management endpoints.
//!
//! See: https://lmstudio.ai/docs/developer/rest

use super::common::resolve_model;
use super::openai::OpenAIProvider;
use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use crate::llm::providers::common::override_base_url;
use crate::llm::types as llm_types;
use crate::utils::http_client;
use anyhow::Result;
use async_trait::async_trait;

pub mod client;

pub use client::LMStudioClient;

#[derive(Debug, Deserialize, Serialize)]
struct LmStudioModelsResponse {
    data: Vec<LmStudioModel>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LmStudioModel {
    id: String,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    owned_by: Option<String>,
}

const LMSTUDIO_CONNECTION_ERROR: &str = "LM Studio is not responding. Install from https://lmstudio.ai/download and run 'lms server start'.";

/// Fetches available models from the LM Studio API endpoint
///
/// Uses OpenAI-compatible `/v1/models` endpoint by default.
/// Set `LMSTUDIO_USE_V1_API=true` to use native v1 API at `/api/v1/models`.
pub async fn fetch_lmstudio_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    let resolved_base_url = override_base_url(
        urls::LMSTUDIO_API_BASE,
        base_url,
        Some(env_vars::LMSTUDIO_BASE_URL),
    );

    // Check if v1 API should be used
    let use_v1_api = std::env::var("LMSTUDIO_USE_V1_API")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Construct the models endpoint URL
    let models_url = if use_v1_api {
        format!("{}/api/v1/models", resolved_base_url)
    } else {
        format!("{}/v1/models", resolved_base_url)
    };

    // Create HTTP client with connection timeout
    let client = http_client::create_client_with_timeout(std::time::Duration::from_secs(5));

    // Make GET request to fetch models
    let response = client
        .get(&models_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Failed to connect to LM Studio server: {e:?}");
            anyhow::anyhow!(LMSTUDIO_CONNECTION_ERROR)
        })?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch LM Studio models: HTTP {}. {}",
            response.status(),
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                "Ensure LM Studio server is running with 'lms server start'."
            } else {
                ""
            }
        ));
    }

    // Parse the response
    let models_response: LmStudioModelsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse LM Studio models response: {}", e))?;

    // Extract model IDs
    let model_ids: Vec<String> = models_response
        .data
        .into_iter()
        .map(|model| model.id)
        .collect();

    Ok(model_ids)
}
use serde::{Deserialize, Serialize};

pub struct LmStudioProvider {
    inner: OpenAIProvider,
}

impl LmStudioProvider {
    fn resolve_base_url(base_url: Option<String>) -> String {
        override_base_url(
            urls::LMSTUDIO_API_BASE,
            base_url,
            Some(env_vars::LMSTUDIO_BASE_URL),
        )
    }

    fn build_inner(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> OpenAIProvider {
        let resolved_model = resolve_model(model, models::lmstudio::DEFAULT_MODEL);
        let resolved_base = Self::resolve_base_url(base_url);
        OpenAIProvider::from_config(
            api_key,
            Some(resolved_model),
            Some(resolved_base),
            prompt_cache,
            _timeouts,
            _anthropic,
            None,
            model_behavior,
        )
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::lmstudio::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(Some(api_key), Some(model), None, None, None)
    }

    pub fn new_with_client(
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        timeouts: TimeoutsConfig,
    ) -> Self {
        let inner = OpenAIProvider::new_with_client(
            "lm-studio".to_string(), // Dummy API key
            model,
            http_client,
            base_url,
            timeouts,
        );
        Self { inner }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        Self::with_model_internal(api_key, model, base_url, prompt_cache, model_behavior)
    }

    fn with_model_internal(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let inner = Self::build_inner(
            api_key,
            model,
            base_url,
            prompt_cache,
            None,
            None,
            model_behavior,
        );
        Self { inner }
    }
}

#[async_trait]
impl LLMProvider for LmStudioProvider {
    fn name(&self) -> &str {
        "lmstudio"
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.inner.supports_reasoning(model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.inner.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.inner.supports_tools(model)
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        self.inner.supports_parallel_tool_config(model)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.inner.generate(request).await
    }

    async fn count_prompt_tokens_exact(
        &self,
        request: &LLMRequest,
    ) -> Result<Option<u32>, LLMError> {
        self.inner.count_prompt_tokens_exact(request).await
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.inner.stream(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        // Hardcoded models prevent expensive network calls. Future enhancements:
        // 1. Lazy initialization via once_cell to fetch models at startup
        // 2. Dynamic fetching with proper caching to avoid repeated network calls
        models::lmstudio::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("LM Studio", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        // Validate messages against provider's requirements
        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("LM Studio", &err);
                return Err(LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for LmStudioProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        LLMClient::generate(&mut self.inner, prompt).await
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        self.inner.backend_kind()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}
