use once_cell::sync::Lazy;

use vtcode_core::config::models::{ModelId, Provider};

#[derive(Clone, Copy)]
pub(super) struct ModelOption {
    pub(super) provider: Provider,
    pub(super) id: &'static str,
    pub(super) display: &'static str,
    pub(super) description: &'static str,
    pub(super) supports_reasoning: bool,
    pub(super) reasoning_alternative: Option<ModelId>,
}

pub(super) static MODEL_OPTIONS: Lazy<Vec<ModelOption>> = Lazy::new(|| {
    let mut options = Vec::new();
    for provider in Provider::all_providers() {
        for model in ModelId::models_for_provider(provider) {
            options.push(ModelOption {
                provider,
                id: model.as_str(),
                display: model.display_name(),
                description: model.description(),
                supports_reasoning: model.supports_reasoning_effort(),
                reasoning_alternative: model.non_reasoning_variant(),
            });
        }
    }
    options
});

pub(super) fn picker_provider_order() -> Vec<Provider> {
    let mut providers: Vec<Provider> = Provider::all_providers()
        .into_iter()
        .filter(|provider| !matches!(provider, Provider::LmStudio | Provider::Ollama))
        .collect();
    providers.push(Provider::LmStudio);
    providers.push(Provider::Ollama);
    providers
}
