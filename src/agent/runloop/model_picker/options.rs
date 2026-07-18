use hashbrown::HashMap;
use once_cell::sync::Lazy;

use std::collections::BTreeMap;
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

static EMPTY_OPTION_INDEXES: [usize; 0] = [];

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
            ModelId::Custom(provider_key, _) => {
                // For custom models, resolve the provider from the override key.
                // If the key doesn't match a known provider, treat it as a custom
                // provider that routes through the OpenAI-compatible endpoint.
                use std::str::FromStr;
                Provider::from_str(provider_key).unwrap_or(Provider::OpenAI)
            }
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

static MODEL_OPTION_INDEXES_BY_PROVIDER: Lazy<HashMap<Provider, Box<[usize]>>> = Lazy::new(|| {
    let mut indexes: HashMap<Provider, Vec<usize>> = HashMap::new();
    for (index, option) in MODEL_OPTIONS.iter().enumerate() {
        indexes.entry(option.provider).or_default().push(index);
    }

    indexes
        .into_iter()
        .map(|(provider, provider_indexes)| (provider, provider_indexes.into_boxed_slice()))
        .collect()
});

static MODEL_OPTION_INDEX_BY_PROVIDER_MODEL: Lazy<HashMap<Provider, HashMap<String, usize>>> = Lazy::new(|| {
    let mut index = HashMap::new();
    for (option_index, option) in MODEL_OPTIONS.iter().enumerate() {
        index
            .entry(option.provider)
            .or_insert_with(HashMap::new)
            .insert(option.id.to_ascii_lowercase(), option_index);
    }
    index
});

pub(super) fn option_indexes_for_provider(provider: Provider) -> &'static [usize] {
    MODEL_OPTION_INDEXES_BY_PROVIDER
        .get(&provider)
        .map(Box::as_ref)
        .unwrap_or(&EMPTY_OPTION_INDEXES)
}

pub(super) fn find_option_index(provider: Provider, model_id: &str) -> Option<usize> {
    MODEL_OPTION_INDEX_BY_PROVIDER_MODEL
        .get(&provider)
        .and_then(|provider_models| provider_models.get(&model_id.to_ascii_lowercase()).copied())
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
