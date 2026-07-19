use once_cell::sync::Lazy;

use std::collections::BTreeMap;
use std::str::FromStr;
use vtcode_config::core::ProviderOverrideConfig;
use vtcode_core::config::models::{ModelId, Provider};

#[derive(Clone)]
pub(super) struct ModelOption {
    pub(super) model: ModelId,
    pub(super) provider: Provider,
    pub(super) id: String,
    pub(super) display: String,
    pub(super) description: String,
    pub(super) supports_reasoning: bool,
    pub(super) reasoning_alternative: Option<ModelId>,
}

/// Check if a model should be filtered out from the picker.
///
/// Currently filters out Copilot models except for CopilotAuto.
fn should_filter_model(provider: Provider, model: &ModelId) -> bool {
    provider == Provider::Copilot && !matches!(model, ModelId::CopilotAuto)
}

pub(super) static MODEL_OPTIONS: Lazy<Vec<ModelOption>> = Lazy::new(|| {
    let models = ModelId::all_models();
    let mut options = Vec::with_capacity(models.len());
    for model in models {
        let provider = model.provider();
        if should_filter_model(provider, &model) {
            continue;
        }
        options.push(ModelOption {
            id: model.as_str().into_owned(),
            display: model.display_name().into_owned(),
            description: model.description().into_owned(),
            supports_reasoning: model.supports_reasoning_effort(),
            reasoning_alternative: model.non_reasoning_variant(),
            model: model.clone(),
            provider,
        });
    }
    options
});

/// Build model options list with user-defined provider overrides.
///
/// Merges the hardcoded model list with custom models defined in
/// `[providers.<name>]` config sections. Custom models are appended
/// to the list as `ModelId::Custom` variants.
pub(super) fn build_model_options_with_overrides(
    overrides: &BTreeMap<String, ProviderOverrideConfig>,
) -> Vec<ModelOption> {
    if overrides.is_empty() {
        return MODEL_OPTIONS.clone();
    }

    let models = ModelId::all_models_with_overrides(overrides);
    let mut options = Vec::with_capacity(models.len());
    for model in models {
        let provider = match &model {
            ModelId::Custom(provider_key, _) => Provider::from_str(provider_key).unwrap_or(Provider::OpenAI),
            _ => model.provider(),
        };
        if should_filter_model(provider, &model) {
            continue;
        }
        options.push(ModelOption {
            id: model.as_str().into_owned(),
            display: model.display_name().into_owned(),
            description: model.description().into_owned(),
            supports_reasoning: model.supports_reasoning_effort(),
            reasoning_alternative: model.non_reasoning_variant(),
            model: model.clone(),
            provider,
        });
    }
    options
}

pub(super) fn option_indexes_for_provider(provider: Provider) -> &'static [usize] {
    MODEL_OPTIONS
        .iter()
        .enumerate()
        .filter_map(|(index, option)| (option.provider == provider).then_some(index))
        .collect::<Vec<_>>()
        .leak()
}

pub(super) fn find_option_index(provider: Provider, model_id: &str, options: &[ModelOption]) -> Option<usize> {
    options.iter().enumerate().find_map(|(index, option)| {
        if option.provider == provider && option.id.eq_ignore_ascii_case(model_id) {
            Some(index)
        } else {
            None
        }
    })
}

pub(super) fn picker_provider_order() -> Vec<Provider> {
    let mut providers: Vec<Provider> = Provider::all_providers()
        .into_iter()
        .filter(|provider| !matches!(provider, Provider::Ollama | Provider::LlamaCpp))
        .collect();
    providers.push(Provider::LlamaCpp);
    providers.push(Provider::Ollama);
    providers
}
