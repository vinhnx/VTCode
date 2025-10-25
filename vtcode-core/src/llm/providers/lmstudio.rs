use super::common::resolve_model;
use super::openai::OpenAIProvider;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use crate::llm::providers::common::override_base_url;
use crate::llm::types as llm_types;
use anyhow::Result;
use async_trait::async_trait;

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

/// Fetches available models from the LM Studio API endpoint
pub async fn fetch_lmstudio_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    let resolved_base_url = override_base_url(
        urls::LMSTUDIO_API_BASE,
        base_url,
        Some(env_vars::LMSTUDIO_BASE_URL),
    );

    // Construct the models endpoint URL
    let models_url = format!("{}/models", resolved_base_url);

    // Create HTTP client
    let client = reqwest::Client::new();

    // Make GET request to fetch models
    let response = client
        .get(&models_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch LM Studio models: {}", e))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch LM Studio models: HTTP {}",
            response.status()
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
    ) -> OpenAIProvider {
        let resolved_model = resolve_model(model, models::lmstudio::DEFAULT_MODEL);
        let resolved_base = Self::resolve_base_url(base_url);
        OpenAIProvider::from_config(
            api_key,
            Some(resolved_model),
            Some(resolved_base),
            prompt_cache,
        )
    }

    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, models::lmstudio::DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(Some(api_key), Some(model), None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        Self::with_model_internal(api_key, model, base_url, prompt_cache)
    }

    fn with_model_internal(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let inner = Self::build_inner(api_key, model, base_url, prompt_cache);
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

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.inner.stream(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        // For now, return hardcoded models to maintain compatibility
        // In the future, we could fetch dynamic models, but this needs to be done
        // in a way that doesn't make network calls for every supported_models() call
        models::lmstudio::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("LM Studio", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted_error));
        }

        // First check if it's one of the known hardcoded models
        if models::lmstudio::SUPPORTED_MODELS.contains(&request.model.as_str()) {
            // Model is in the known list, proceed with validation
        } else {
            // For now, we'll allow any model that isn't explicitly unsupported
            // In a future enhancement, we could validate against the actual dynamic list
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("LM Studio", &err);
                return Err(LLMError::InvalidRequest(formatted));
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
