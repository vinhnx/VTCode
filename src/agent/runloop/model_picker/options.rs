use hashbrown::HashMap;
use once_cell::sync::Lazy;

use vtcode_core::config::models::{ModelId, Provider};

#[derive(Clone, Copy)]
pub(super) struct ModelOption {
    pub(super) model: ModelId,
    pub(super) provider: Provider,
    pub(super) id: &'static str,
    pub(super) display: &'static str,
    pub(super) description: &'static str,
    pub(super) supports_reasoning: bool,
    pub(super) reasoning_alternative: Option<ModelId>,
}

static EMPTY_OPTION_INDEXES: [usize; 0] = [];

pub(super) static MODEL_OPTIONS: Lazy<Vec<ModelOption>> = Lazy::new(|| {
    let models = ModelId::all_models();
    let mut options = Vec::with_capacity(models.len());
    for model in models {
        let provider = model.provider();
        options.push(ModelOption {
            model,
            provider,
            id: model.as_str(),
            display: model.display_name(),
            description: model.description(),
            supports_reasoning: model.supports_reasoning_effort(),
            reasoning_alternative: model.non_reasoning_variant(),
        });
    }
    options
});

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

static MODEL_OPTION_INDEX_BY_PROVIDER_MODEL: Lazy<HashMap<Provider, HashMap<String, usize>>> =
    Lazy::new(|| {
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
        .filter(|provider| !matches!(provider, Provider::Ollama))
        .collect();
    providers.push(Provider::Ollama);
    providers
}
