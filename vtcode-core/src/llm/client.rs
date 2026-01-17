use super::provider::LLMError;
use super::types::{BackendKind, LLMResponse};
use crate::config::models::ModelId;
use async_trait::async_trait;

/// Unified LLM client trait
#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError>;
    fn backend_kind(&self) -> BackendKind;
    fn model_id(&self) -> &str;
}

/// Type-erased LLM client
pub type AnyClient = Box<dyn LLMClient>;

/// Create a client based on the model ID
/// Uses the existing factory pattern from factory.rs
#[allow(clippy::result_large_err)]
pub fn make_client(api_key: String, model: ModelId) -> Result<AnyClient, LLMError> {
    let model_id = model.to_string();
    // Use factory to create provider
    let provider = super::factory::create_provider_for_model(&model_id, api_key, None)?;

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
        let provider_response = self.provider.generate(request).await?;
        // Convert provider::LLMResponse to types::LLMResponse
        Ok(LLMResponse {
            content: provider_response.content.unwrap_or_default(),
            model: self.model_id.clone(),
            usage: provider_response.usage.map(|u| super::types::Usage {
                prompt_tokens: u.prompt_tokens as usize,
                completion_tokens: u.completion_tokens as usize,
                total_tokens: u.total_tokens as usize,
                cached_prompt_tokens: u.cached_prompt_tokens.map(|v| v as usize),
                cache_creation_tokens: u.cache_creation_tokens.map(|v| v as usize),
                cache_read_tokens: u.cache_read_tokens.map(|v| v as usize),
            }),
            reasoning: provider_response.reasoning,
            reasoning_details: provider_response.reasoning_details,
            request_id: provider_response.request_id,
            organization_id: provider_response.organization_id,
        })
    }

    fn backend_kind(&self) -> BackendKind {
        // Determine backend kind from provider name
        match self.provider.name() {
            "gemini" => BackendKind::Gemini,
            "openai" => BackendKind::OpenAI,
            "anthropic" => BackendKind::Anthropic,
            "deepseek" => BackendKind::DeepSeek,
            "openrouter" => BackendKind::OpenRouter,
            "ollama" => BackendKind::Ollama,
            "xai" => BackendKind::XAI,
            "zai" => BackendKind::ZAI,
            "moonshot" => BackendKind::Moonshot,
            _ => BackendKind::OpenAI, // Default fallback
        }
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
