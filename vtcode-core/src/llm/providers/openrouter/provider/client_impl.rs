use super::super::OpenRouterProvider;
use crate::llm::client::LLMClient;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest};
use crate::llm::types as llm_types;
use async_trait::async_trait;

#[async_trait]
impl LLMClient for OpenRouterProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::llm::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::OpenRouter
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}