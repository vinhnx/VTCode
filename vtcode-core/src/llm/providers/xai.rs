use crate::config::constants::{models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse};
use crate::llm::providers::openai::OpenAIProvider;
use crate::llm::types as llm_types;
use async_trait::async_trait;

use super::common::{forward_prompt_cache_with_state, override_base_url, resolve_model};

/// xAI provider that leverages the OpenAI-compatible Grok API surface
pub struct XAIProvider {
    inner: OpenAIProvider,
    model: String,
    prompt_cache_enabled: bool,
}

impl XAIProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::xai::DEFAULT_MODEL.to_string(), None)
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, models::xai::DEFAULT_MODEL);
        let resolved_base_url = override_base_url(urls::XAI_API_BASE, base_url);
        let (prompt_cache_enabled, prompt_cache_forward) = forward_prompt_cache_with_state(
            prompt_cache,
            |cfg| cfg.enabled && cfg.providers.xai.enabled,
            true,
        );
        let inner = OpenAIProvider::from_config(
            api_key,
            Some(resolved_model.clone()),
            Some(resolved_base_url),
            prompt_cache_forward,
        );

        Self {
            inner,
            model: resolved_model,
            prompt_cache_enabled,
        }
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        Self::from_config(Some(api_key), Some(model), None, prompt_cache)
    }
}

#[async_trait]
impl LLMProvider for XAIProvider {
    fn name(&self) -> &str {
        "xai"
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };

        requested == models::xai::GROK_4
            || requested == models::xai::GROK_4_CODE
            || requested == models::xai::GROK_4_CODE_LATEST
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if !self.prompt_cache_enabled {
            // xAI prompt caching is managed by the platform; no additional parameters required.
        }

        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        self.inner.generate(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        models::xai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted = error_display::format_llm_error("xAI", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted));
        }

        if !request.model.trim().is_empty() && !self.supported_models().contains(&request.model) {
            let formatted = error_display::format_llm_error(
                "xAI",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("xAI", &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for XAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        <OpenAIProvider as LLMClient>::generate(&mut self.inner, prompt).await
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::XAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
