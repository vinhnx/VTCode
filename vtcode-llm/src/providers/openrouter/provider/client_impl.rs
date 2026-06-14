use super::super::OpenRouterProvider;
use crate::client::LLMClient;
use crate::provider::{LLMError, LLMProvider, LLMRequest};
use crate::types as llm_types;
use async_trait::async_trait;

#[async_trait]
impl LLMClient for OpenRouterProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        let request = LLMRequest {
            messages: vec![crate::provider::Message::user(prompt.to_string())],
            model: self.model.clone(),
            ..Default::default()
        };
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
