use super::provider::LLMError;
use super::types::LLMResponse;
use crate::config::models::ModelId;
use async_trait::async_trait;

/// Unified LLM client trait.
///
/// Note: `backend_kind()` lives on [`LLMProvider`](super::provider::LLMProvider)
/// rather than here, following the **single responsibility** principle — the
/// provider knows its own backend identity.
#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError>;
    fn model_id(&self) -> &str;
}

/// Type-erased LLM client
pub type AnyClient = Box<dyn LLMClient>;

/// Create a client based on the model ID
/// Uses the existing factory pattern from factory.rs
pub fn make_client(api_key: String, model: ModelId) -> Result<AnyClient, LLMError> {
    let model_id = model.to_string();
    // Use factory to create provider
    let provider = super::factory::create_provider_for_model(&model_id, api_key, None, None)?;

    // Wrap in a simple client adapter
    Ok(Box::new(ProviderClientAdapter { provider, model_id }))
}

/// Adapter to use LLMProvider as LLMClient
///
/// This allows using the provider interface through the simpler client trait.
pub struct ProviderClientAdapter {
    provider: Box<dyn super::provider::LLMProvider>,
    model_id: String,
}

impl ProviderClientAdapter {
    /// Create a new adapter wrapping an LLMProvider
    pub fn new(provider: Box<dyn super::provider::LLMProvider>, model_id: String) -> Self {
        Self { provider, model_id }
    }
}

#[async_trait]
impl LLMClient for ProviderClientAdapter {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        use super::provider::{LLMRequest, Message};
        let request = LLMRequest {
            messages: vec![Message::user(prompt.to_string())],
            model: self.model_id.clone(),
            ..Default::default()
        };
        Ok(self.provider.generate(request).await?)
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
