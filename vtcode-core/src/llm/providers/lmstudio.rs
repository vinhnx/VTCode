use super::common::resolve_model;
use super::openai::OpenAIProvider;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use crate::llm::providers::common::override_base_url;
use crate::llm::types as llm_types;
use async_trait::async_trait;

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

        if !models::lmstudio::SUPPORTED_MODELS.contains(&request.model.as_str()) {
            let formatted_error = error_display::format_llm_error(
                "LM Studio",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted_error));
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
