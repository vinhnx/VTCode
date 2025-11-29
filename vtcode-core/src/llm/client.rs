use super::provider::LLMError;
use super::providers::GeminiProvider;
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
pub fn make_client(api_key: String, model: ModelId) -> AnyClient {
    // Use factory to create provider
    let provider = super::factory::create_provider_for_model(
        &model.to_string(),
        api_key,
        None,
    ).expect("Failed to create provider");
    
    // Wrap in a simple client adapter
    Box::new(ProviderClientAdapter {
        provider,
        model_id: model.to_string(),
    })
}

/// Adapter to use LLMProvider as LLMClient
struct ProviderClientAdapter {
    provider: Box<dyn super::provider::LLMProvider>,
    model_id: String,
}

#[async_trait]
impl LLMClient for ProviderClientAdapter {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        use super::provider::{LLMRequest, Message};
        let request = LLMRequest {
            messages: vec![Message::user(prompt.to_string())],
            system_prompt: None,
            tools: None,
            model: self.model_id.clone(),
            max_tokens: None,
            temperature: None,
            stream: false,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
        };
        self.provider.send(request).await
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::External
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
