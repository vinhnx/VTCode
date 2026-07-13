//! LM Studio provider implementation
//!
//! LM Studio provides multiple API surfaces:
//! - OpenAI-compatible endpoints at `/v1/*` (used by this implementation)
//! - Native REST API at `/api/v0/*` (enhanced stats, model info, model management)
//! - Tool calling (since 0.3.6), structured output, and reasoning content (since 0.3.9)
//!
//! This implementation uses OpenAI-compatible endpoints for maximum compatibility.
//! The native REST API at `/api/v0/*` provides richer model metadata, load/unload
//! endpoints, and TTL-based auto-evict for JIT-loaded models.
//!
//! See: <https://lmstudio.ai/docs/developer>

use super::common::resolve_model;
use super::local_readiness::resolve_local_model;
use super::local_server::LocalProvider;
use crate::client::LLMClient;
use crate::error_display;
use crate::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, Message};
use crate::providers::common::override_base_url;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use vtcode_config::TimeoutsConfig;
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};

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

pub use client::LMSTUDIO_CONNECTION_ERROR;

/// Derives the server root URL by stripping the `/v1` suffix from the API base.
///
/// `LMSTUDIO_API_BASE` is `http://localhost:1234/v1`. The native REST API
/// lives at `/api/v0/*` on the server root, so we need `http://localhost:1234`.
fn server_root_from_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

/// Fetches available models from the LM Studio API endpoint
///
/// Uses OpenAI-compatible `/v1/models` endpoint by default.
/// Set `LMSTUDIO_USE_NATIVE_API=true` to use native REST API at `/api/v0/models`.
pub async fn fetch_lmstudio_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    let resolved_base_url = override_base_url(
        urls::LMSTUDIO_API_BASE,
        base_url,
        Some(env_vars::LMSTUDIO_BASE_URL),
    );

    let use_native_api = std::env::var("LMSTUDIO_USE_NATIVE_API")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // LMSTUDIO_API_BASE already includes `/v1`, so for the OpenAI-compatible
    // endpoint we append `/models` directly. For the native REST API we need
    // the server root (without `/v1`) and then append `/api/v0/models`.
    let models_url = if use_native_api {
        let root = server_root_from_api_base(&resolved_base_url);
        format!("{root}/api/v0/models")
    } else {
        format!("{}/models", resolved_base_url.trim_end_matches('/'))
    };

    // Create HTTP client with connection timeout
    let client =
        vtcode_commons::http::create_client_with_timeout(std::time::Duration::from_secs(5));

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
        .map_err(|e| anyhow::anyhow!("Failed to parse LM Studio models response: {e}"))?;

    // Extract model IDs
    let model_ids: Vec<String> = models_response
        .data
        .into_iter()
        .map(|model| model.id)
        .collect();

    Ok(model_ids)
}

pub struct LmStudioProvider {
    inner: Box<dyn LLMProvider>,
    model_id: String,
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
        timeouts: Option<TimeoutsConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> (Box<dyn LLMProvider>, String) {
        let resolved_model = resolve_model(model, models::lmstudio::DEFAULT_MODEL);
        let resolved_base = Self::resolve_base_url(base_url);
        let inner = Box::new(crate::providers::OpenAIProvider::from_config(
            api_key,
            None,
            Some(resolved_model.clone()),
            Some(resolved_base),
            prompt_cache,
            timeouts,
            anthropic,
            None,
            model_behavior,
        ));
        (inner, resolved_model)
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
        let inner = Box::new(crate::providers::OpenAIProvider::new_with_client(
            "lm-studio".to_string(), // Dummy API key
            None,
            model.clone(),
            http_client,
            base_url,
            timeouts,
        ));
        Self {
            inner,
            model_id: model,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let (inner, model_id) = Self::build_inner(
            api_key,
            model,
            base_url,
            prompt_cache,
            timeouts,
            anthropic,
            model_behavior,
        );
        Self { inner, model_id }
    }

    fn with_model_internal(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let (inner, model_id) = Self::build_inner(
            api_key,
            model,
            base_url,
            prompt_cache,
            None,
            None,
            model_behavior,
        );
        Self { inner, model_id }
    }

    /// Verify the LM Studio server is up and the requested model is loaded
    /// before generating. Returns the (possibly substituted) model id or a
    /// structured error with a recovery command (`lms load <model>` /
    /// `/local start lmstudio`).
    async fn ensure_ready(&self, requested: &str) -> Result<String, LLMError> {
        match resolve_local_model(LocalProvider::LmStudio, requested, None).await {
            Ok(model) => Ok(model),
            Err(err) => Err(err.to_llm_error("LM Studio")),
        }
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

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let resolved = self.ensure_ready(&request.model).await?;
        if !resolved.is_empty() {
            request.model = resolved;
        }
        self.inner.generate(request).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        let resolved = self.ensure_ready(&request.model).await?;
        if !resolved.is_empty() {
            request.model = resolved;
        }
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
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        LLMProvider::generate(
            self,
            LLMRequest {
                messages: vec![Message::user(prompt.to_string())],
                model: self.model_id.clone(),
                ..Default::default()
            },
        )
        .await
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
