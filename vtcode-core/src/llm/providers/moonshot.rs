use crate::config::constants::{env_vars, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse};
use crate::llm::providers::openai::OpenAIProvider;
use crate::llm::types as llm_types;
use async_trait::async_trait;

use super::common::{forward_prompt_cache_with_state, override_base_url, resolve_model};

/// Moonshot.ai provider implemented as an OpenAI-compatible wrapper.
pub struct MoonshotProvider {
    inner: OpenAIProvider,
    model: String,
}

impl MoonshotProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::moonshot::DEFAULT_MODEL.to_string(), None)
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
        let resolved_model = resolve_model(model, models::moonshot::DEFAULT_MODEL);
        let resolved_base_url = override_base_url(
            urls::MOONSHOT_API_BASE,
            base_url,
            Some(env_vars::MOONSHOT_BASE_URL),
        );
        let (_, prompt_cache_forward) = forward_prompt_cache_with_state(
            prompt_cache,
            |cfg| cfg.enabled && cfg.providers.moonshot.enabled,
            false,
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
impl LLMProvider for MoonshotProvider {
    fn name(&self) -> &str {
        "moonshot"
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.inner.generate(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        models::moonshot::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted = error_display::format_llm_error("Moonshot", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest(formatted));
        }

        if !request.model.trim().is_empty() && !self.supported_models().contains(&request.model) {
            let formatted = error_display::format_llm_error(
                "Moonshot",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("Moonshot", &err);
                return Err(LLMError::InvalidRequest(formatted));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for MoonshotProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        <OpenAIProvider as LLMClient>::generate(&mut self.inner, prompt).await
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Moonshot
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
