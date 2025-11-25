use super::provider::LLMError;
use super::providers::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, LmStudioProvider, MinimaxProvider,
    MoonshotProvider, OllamaProvider, OpenAIProvider, OpenRouterProvider, XAIProvider, ZAIProvider,
};
use super::types::{BackendKind, LLMResponse};
use crate::config::models::{ModelId, Provider};
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
pub fn make_client(api_key: String, model: ModelId) -> AnyClient {
    // Extract model string once to avoid repeated allocations
    let model_str = model.to_string();
    match model.provider() {
        Provider::Gemini => Box::new(GeminiProvider::with_model(api_key, model_str)),
        Provider::OpenAI => Box::new(OpenAIProvider::with_model(api_key, model_str)),
        Provider::Anthropic => Box::new(AnthropicProvider::new(api_key)),
        Provider::Minimax => Box::new(MinimaxProvider::from_config(
            Some(api_key),
            Some(model_str),
            None,
            None,
            None,
        )),
        Provider::DeepSeek => Box::new(DeepSeekProvider::with_model(api_key, model_str)),
        Provider::OpenRouter => Box::new(OpenRouterProvider::with_model(api_key, model_str)),
        Provider::Ollama => Box::new(OllamaProvider::with_model(api_key, model_str)),
        Provider::LmStudio => Box::new(LmStudioProvider::with_model(api_key, model_str)),
        Provider::Moonshot => Box::new(MoonshotProvider::with_model(api_key, model_str)),
        Provider::XAI => Box::new(XAIProvider::with_model(api_key, model_str)),
        Provider::ZAI => Box::new(ZAIProvider::with_model(api_key, model_str)),
    }
}
