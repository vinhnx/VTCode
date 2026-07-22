use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use tracing::warn;

use vtcode_config::VTCodeConfig;
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

static PROVIDER_OPTION_INDEXES: Lazy<HashMap<Provider, Box<[usize]>>> = Lazy::new(|| {
    let mut map = HashMap::<Provider, Vec<usize>>::with_capacity(64);
    for (index, option) in MODEL_OPTIONS.iter().enumerate() {
        map.entry(option.provider).or_default().push(index);
    }
    map.into_iter().map(|(k, v)| (k, v.into_boxed_slice())).collect()
});

static PICKER_PROVIDER_ORDER: Lazy<Box<[Provider]>> = Lazy::new(|| {
    Provider::all_providers()
        .into_iter()
        .filter(|p| !matches!(p, Provider::Ollama | Provider::LlamaCpp))
        .chain([Provider::LlamaCpp, Provider::Ollama])
        .collect::<Vec<_>>()
        .into_boxed_slice()
});

pub(super) fn build_model_options_with_overrides(
    overrides: &BTreeMap<String, ProviderOverrideConfig>,
) -> Cow<'static, [ModelOption]> {
    if overrides.is_empty() {
        return Cow::Borrowed(MODEL_OPTIONS.as_slice());
    }

    let models = ModelId::all_models_with_overrides(overrides);
    let mut options = Vec::with_capacity(models.len());
    for model in models {
        let provider = match &model {
            ModelId::Custom(provider_key, _) => match Provider::from_str(provider_key) {
                Ok(parsed) => parsed,
                Err(_) => {
                    warn!("Unknown provider key '{}' in provider_overrides; defaulting to OpenAI", provider_key);
                    Provider::OpenAI
                }
            },
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
    Cow::Owned(options)
}

pub(super) fn option_indexes_for_provider(provider: Provider) -> &'static [usize] {
    PROVIDER_OPTION_INDEXES.get(&provider).map_or(&[], Box::as_ref)
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

pub(super) fn build_filtered_options(vt_cfg: Option<&VTCodeConfig>) -> Cow<'static, [ModelOption]> {
    let Some(cfg) = vt_cfg else {
        return Cow::Borrowed(MODEL_OPTIONS.as_slice());
    };
    let opts: Cow<'static, [ModelOption]> = if !cfg.provider_overrides.is_empty() {
        build_model_options_with_overrides(&cfg.provider_overrides)
    } else {
        Cow::Borrowed(MODEL_OPTIONS.as_slice())
    };
    filter_options_by_whitelist(opts, &cfg.providers_whitelist)
}

pub(super) fn picker_provider_order() -> &'static [Provider] {
    PICKER_PROVIDER_ORDER.as_ref()
}

pub(super) fn picker_provider_order_with_whitelist(whitelist: &[String]) -> Vec<Provider> {
    if whitelist.is_empty() {
        return PICKER_PROVIDER_ORDER.to_vec();
    }
    PICKER_PROVIDER_ORDER
        .iter()
        .copied()
        .filter(|p| whitelist.iter().any(|w| w.eq_ignore_ascii_case(p.as_ref())))
        .collect()
}

pub(super) fn filter_options_by_whitelist(
    options: Cow<'static, [ModelOption]>,
    whitelist: &[String],
) -> Cow<'static, [ModelOption]> {
    if whitelist.is_empty() {
        return options;
    }
    let allowed: HashSet<String> = whitelist.iter().map(|w| w.to_ascii_lowercase()).collect();
    let filtered: Vec<ModelOption> = options
        .iter()
        .filter(|opt| allowed.contains(&opt.provider.as_ref().to_ascii_lowercase()))
        .cloned()
        .collect();
    Cow::Owned(filtered)
}
