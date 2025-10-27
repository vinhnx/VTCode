use super::AnthropicProvider;
use crate::config::constants::{models, urls};
use crate::config::core::PromptCachingConfig;
use crate::llm::client::LLMClient;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream};
use async_trait::async_trait;

pub struct MinimaxProvider {
    inner: AnthropicProvider,
}

impl MinimaxProvider {
    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        let effective_model = model.unwrap_or_else(|| models::minimax::MINIMAX_M2.to_string());
        let effective_base_url = base_url.filter(|url| !url.trim().is_empty()).or_else(|| {
            Some(urls::MINIMAX_API_BASE.to_string())
        });

        let inner = AnthropicProvider::from_config(
            api_key,
            Some(effective_model),
            effective_base_url,
            prompt_cache,
        );

        Self { inner }
    }
}

#[async_trait]
impl LLMProvider for MinimaxProvider {
    fn name(&self) -> &str {
        "minimax"
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
        vec![models::minimax::MINIMAX_M2.to_string()]
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        self.inner.validate_request(request)
    }
}

#[async_trait]
impl LLMClient for MinimaxProvider {
    async fn generate(&mut self, prompt: &str) -> Result<crate::llm::types::LLMResponse, LLMError> {
        LLMClient::generate(&mut self.inner, prompt).await
    }

    fn backend_kind(&self) -> crate::llm::types::BackendKind {
        LLMClient::backend_kind(&self.inner)
    }

    fn model_id(&self) -> &str {
        LLMClient::model_id(&self.inner)
    }
}



