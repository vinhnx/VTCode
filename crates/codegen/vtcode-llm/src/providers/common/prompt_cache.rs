//! Provider prompt-cache configuration forwarding.

use vtcode_config::core::{PromptCachingConfig, ProviderPromptCachingConfig};

pub(crate) fn extract_prompt_cache_settings_default(
    prompt_cache: Option<PromptCachingConfig>,
    _provider_key: &str,
) -> (bool, bool) {
    match prompt_cache {
        Some(cfg) if cfg.enabled => (true, cfg.enabled),
        _ => (false, false),
    }
}

pub(crate) fn extract_prompt_cache_settings<T, SelectFn, EnabledFn>(
    prompt_cache: Option<PromptCachingConfig>,
    select_settings: SelectFn,
    enabled: EnabledFn,
) -> (bool, T)
where
    T: Clone + Default,
    SelectFn: Fn(&ProviderPromptCachingConfig) -> &T,
    EnabledFn: Fn(&PromptCachingConfig, &T) -> bool,
{
    if let Some(cfg) = prompt_cache {
        let provider_settings = select_settings(&cfg.providers).clone();
        let is_enabled = enabled(&cfg, &provider_settings);
        (is_enabled, provider_settings)
    } else {
        (false, T::default())
    }
}

pub fn forward_prompt_cache_with_state<PredicateFn>(
    prompt_cache: Option<PromptCachingConfig>,
    predicate: PredicateFn,
    default_enabled: bool,
) -> (bool, Option<PromptCachingConfig>)
where
    PredicateFn: Fn(&PromptCachingConfig) -> bool,
{
    match prompt_cache {
        Some(cfg) => {
            if predicate(&cfg) {
                (true, Some(cfg))
            } else {
                (false, None)
            }
        }
        None => (default_enabled, None),
    }
}
