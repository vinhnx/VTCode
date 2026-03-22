use std::borrow::Cow;
use std::str::FromStr;

use crate::config::api_keys::api_key_env_var;
use crate::config::models::{
    ModelCatalogEntry, ModelPricing, Provider, catalog_provider_keys, model_catalog_entry,
};
use crate::llm::provider::Usage;
use vtcode_config::auth::{AuthCredentialsStoreMode, CustomApiKeyStorage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAvailability {
    Available,
    MissingCredential,
    ManagedAuthAvailable,
    Misconfigured,
    LocalOnly,
}

impl ModelAvailability {
    pub fn requires_api_key(&self) -> bool {
        matches!(self, Self::MissingCredential | Self::Misconfigured)
    }

    pub fn uses_managed_auth(&self) -> bool {
        matches!(self, Self::ManagedAuthAvailable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicModelMeta {
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct DynamicModelRef<'a> {
    pub provider: Provider,
    pub model_id: &'a str,
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider: Provider,
    pub model_id: String,
    pub catalog: Option<ModelCatalogEntry>,
    pub dynamic: Option<DynamicModelMeta>,
    pub availability: ModelAvailability,
}

impl ResolvedModel {
    pub fn known_model(&self) -> bool {
        self.catalog.is_some()
    }

    pub fn reasoning_supported(&self) -> bool {
        self.catalog
            .map(|entry| entry.reasoning)
            .unwrap_or_else(|| self.provider.supports_reasoning_effort(&self.model_id))
    }

    pub fn service_tier_supported(&self) -> bool {
        self.provider.supports_service_tier(&self.model_id)
    }

    pub fn supports_tool_calls(&self) -> bool {
        self.catalog.map(|entry| entry.tool_call).unwrap_or(true)
    }

    pub fn context_window(&self) -> Option<usize> {
        self.catalog
            .map(|entry| entry.context_window)
            .filter(|value| *value > 0)
            .or_else(|| {
                self.dynamic
                    .as_ref()
                    .and_then(|dynamic| dynamic.context_window)
            })
    }

    pub fn input_modalities(&self) -> &'static [&'static str] {
        self.catalog
            .map(|entry| entry.input_modalities)
            .unwrap_or(&[])
    }

    pub fn display_name(&self) -> Cow<'_, str> {
        if let Some(catalog) = self.catalog {
            return Cow::Borrowed(catalog.display_name);
        }
        if let Some(dynamic) = &self.dynamic {
            return Cow::Borrowed(dynamic.display_name.as_str());
        }
        Cow::Borrowed(self.model_id.as_str())
    }

    pub fn description(&self) -> Option<Cow<'_, str>> {
        if let Some(catalog) = self.catalog {
            return (!catalog.description.is_empty()).then_some(Cow::Borrowed(catalog.description));
        }
        self.dynamic.as_ref().and_then(|dynamic| {
            dynamic
                .description
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(Cow::Borrowed)
        })
    }

    pub fn pricing(&self) -> Option<ModelPricing> {
        self.catalog.map(|entry| entry.pricing).filter(|pricing| {
            pricing.input.is_some()
                || pricing.output.is_some()
                || pricing.cache_read.is_some()
                || pricing.cache_write.is_some()
        })
    }

    pub fn env_key(&self) -> String {
        api_key_env_var(self.provider.as_ref())
    }
}

pub struct ModelResolver;

impl ModelResolver {
    pub fn resolve(
        provider_override: Option<&str>,
        model: &str,
        dynamic_models: &[DynamicModelRef<'_>],
        dynamic_meta: Option<DynamicModelMeta>,
    ) -> Option<ResolvedModel> {
        let model = model.trim();
        if model.is_empty() {
            return None;
        }

        if let Some(provider) = provider_override.and_then(parse_provider_override) {
            return Some(Self::resolve_for_provider(
                provider,
                model,
                dynamic_models,
                dynamic_meta,
            ));
        }

        if let Some((provider, entry)) = find_catalog_provider(model) {
            return Some(ResolvedModel {
                provider,
                model_id: model.to_string(),
                catalog: Some(entry),
                dynamic: dynamic_meta,
                availability: Self::availability(provider, model),
            });
        }

        if let Some(provider) = find_dynamic_provider(model, dynamic_models) {
            return Some(Self::resolve_for_provider(
                provider,
                model,
                dynamic_models,
                dynamic_meta,
            ));
        }

        let provider = heuristic_provider_from_model(model)?;
        Some(Self::resolve_for_provider(
            provider,
            model,
            dynamic_models,
            dynamic_meta,
        ))
    }

    pub fn resolve_provider(
        provider_override: Option<&str>,
        model: &str,
        dynamic_models: &[DynamicModelRef<'_>],
    ) -> Option<Provider> {
        Self::resolve(provider_override, model, dynamic_models, None)
            .map(|resolved| resolved.provider)
    }

    pub fn availability(provider: Provider, model: &str) -> ModelAvailability {
        if provider.is_local() && !local_model_requires_remote_auth(provider, model) {
            return ModelAvailability::LocalOnly;
        }

        if provider.uses_managed_auth() {
            return ModelAvailability::ManagedAuthAvailable;
        }

        if provider == Provider::OpenAI
            && vtcode_config::auth::load_openai_chatgpt_session()
                .ok()
                .flatten()
                .is_some()
        {
            return ModelAvailability::ManagedAuthAvailable;
        }

        if provider == Provider::OpenRouter
            && vtcode_config::auth::load_oauth_token()
                .ok()
                .flatten()
                .is_some()
        {
            return ModelAvailability::ManagedAuthAvailable;
        }

        let env_key = api_key_env_var(provider.as_ref());
        if env_key.trim().is_empty() {
            return ModelAvailability::ManagedAuthAvailable;
        }

        if has_env_value(&env_key) || has_stored_key(provider) {
            return ModelAvailability::Available;
        }

        if std::env::var(&env_key).is_ok() {
            return ModelAvailability::Misconfigured;
        }

        ModelAvailability::MissingCredential
    }

    pub fn estimate_cost(pricing: ModelPricing, usage: &Usage) -> Option<f64> {
        let input_cost = pricing.input?;
        let output_cost = pricing.output?;

        let mut total = (usage.prompt_tokens as f64 * input_cost)
            + (usage.completion_tokens as f64 * output_cost);

        if let Some(cache_read_cost) = pricing.cache_read {
            total += usage.cache_read_tokens_or_fallback() as f64 * cache_read_cost;
        }

        if let Some(cache_write_cost) = pricing.cache_write {
            total += usage.cache_creation_tokens_or_zero() as f64 * cache_write_cost;
        }

        Some(total)
    }

    fn resolve_for_provider(
        provider: Provider,
        model: &str,
        dynamic_models: &[DynamicModelRef<'_>],
        dynamic_meta: Option<DynamicModelMeta>,
    ) -> ResolvedModel {
        let catalog = model_catalog_entry(provider.as_ref(), model);
        let dynamic = if catalog.is_some() || !has_dynamic_model(provider, model, dynamic_models) {
            None
        } else {
            dynamic_meta.or_else(|| {
                Some(DynamicModelMeta {
                    display_name: model.to_string(),
                    description: None,
                    context_window: None,
                })
            })
        };

        ResolvedModel {
            provider,
            model_id: model.to_string(),
            catalog,
            dynamic,
            availability: Self::availability(provider, model),
        }
    }
}

fn parse_provider_override(value: &str) -> Option<Provider> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Provider::from_str(trimmed).ok()
    }
}

fn find_catalog_provider(model: &str) -> Option<(Provider, ModelCatalogEntry)> {
    let mut matches: Vec<(Provider, ModelCatalogEntry)> = catalog_provider_keys()
        .iter()
        .filter_map(|provider_key| {
            let provider = Provider::from_str(provider_key).ok()?;
            model_catalog_entry(provider_key, model).map(|entry| (provider, entry))
        })
        .collect();
    matches.sort_by_key(|(provider, _)| provider_precedence(*provider));
    matches.into_iter().next()
}

fn find_dynamic_provider(model: &str, dynamic_models: &[DynamicModelRef<'_>]) -> Option<Provider> {
    let mut matches = dynamic_models
        .iter()
        .filter(|candidate| candidate.model_id.eq_ignore_ascii_case(model))
        .map(|candidate| candidate.provider);
    let first = matches.next()?;
    if matches.all(|provider| provider == first) {
        Some(first)
    } else {
        None
    }
}

fn has_dynamic_model(
    provider: Provider,
    model: &str,
    dynamic_models: &[DynamicModelRef<'_>],
) -> bool {
    dynamic_models.iter().any(|candidate| {
        candidate.provider == provider && candidate.model_id.eq_ignore_ascii_case(model)
    })
}

fn provider_precedence(provider: Provider) -> usize {
    match provider {
        Provider::OpenAI => 0,
        Provider::Anthropic => 1,
        Provider::Gemini => 2,
        Provider::DeepSeek => 3,
        Provider::ZAI => 4,
        Provider::Minimax => 5,
        Provider::Moonshot => 6,
        Provider::OpenRouter => 7,
        Provider::HuggingFace => 8,
        Provider::LiteLLM => 9,
        Provider::Copilot => 10,
        Provider::Ollama => 11,
        Provider::LmStudio => 12,
    }
}

fn local_model_requires_remote_auth(provider: Provider, model: &str) -> bool {
    provider == Provider::Ollama && (model.contains(":cloud") || model.contains("-cloud"))
}

fn has_env_value(env_key: &str) -> bool {
    matches!(std::env::var(env_key), Ok(value) if !value.trim().is_empty())
}

fn has_stored_key(provider: Provider) -> bool {
    CustomApiKeyStorage::new(provider.as_ref())
        .load(AuthCredentialsStoreMode::default())
        .ok()
        .flatten()
        .is_some()
}

pub(crate) fn heuristic_provider_from_model(model: &str) -> Option<Provider> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains(':') && !trimmed.contains('/') && !trimmed.contains('@') {
        return Some(Provider::Ollama);
    }

    let model = trimmed.to_ascii_lowercase();
    if model.starts_with("gpt-oss-")
        || model.starts_with("gpt-")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("codex")
    {
        Some(Provider::OpenAI)
    } else if model == "copilot" || model.starts_with("copilot-") {
        Some(Provider::Copilot)
    } else if model.starts_with("claude-") {
        Some(Provider::Anthropic)
    } else if model.starts_with("deepseek-") {
        Some(Provider::DeepSeek)
    } else if model.contains("gemini") || model.starts_with("palm") {
        Some(Provider::Gemini)
    } else if model.starts_with("glm-") {
        Some(Provider::ZAI)
    } else if model.starts_with("litellm/") {
        Some(Provider::LiteLLM)
    } else if model.starts_with("lmstudio-community/") {
        Some(Provider::LmStudio)
    } else if model.starts_with("moonshot-") || model.starts_with("kimi-") {
        Some(Provider::Moonshot)
    } else if model.starts_with("deepseek-ai/")
        || model.starts_with("openai/gpt-oss-")
        || model.starts_with("zai-org/")
        || model.starts_with("moonshotai/")
        || model.starts_with("minimaxai/")
    {
        Some(Provider::HuggingFace)
    } else if model.starts_with("mistral-")
        || model.starts_with("mixtral-")
        || model.starts_with("qwen-")
        || model.starts_with("meta-")
        || model.starts_with("llama-")
        || model.starts_with("command-")
        || model.contains('/')
        || model.contains('@')
    {
        Some(Provider::OpenRouter)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_prefers_catalog_match_over_heuristic() {
        let resolved = ModelResolver::resolve(None, "gpt-5.4", &[], None).expect("model");

        assert_eq!(resolved.provider, Provider::OpenAI);
        assert!(resolved.known_model());
        assert_eq!(resolved.display_name(), "GPT-5.4");
    }

    #[test]
    fn resolver_uses_provider_override_for_dynamic_model() {
        let dynamic_models = [DynamicModelRef {
            provider: Provider::Ollama,
            model_id: "custom-local-model",
        }];
        let resolved = ModelResolver::resolve(
            Some("ollama"),
            "custom-local-model",
            &dynamic_models,
            Some(DynamicModelMeta {
                display_name: "Custom Local Model".to_string(),
                description: Some("dynamic".to_string()),
                context_window: Some(32_000),
            }),
        )
        .expect("resolved model");

        assert_eq!(resolved.provider, Provider::Ollama);
        assert!(!resolved.known_model());
        assert_eq!(resolved.context_window(), Some(32_000));
    }

    #[test]
    fn estimate_cost_uses_usage_totals() {
        let pricing = ModelPricing {
            input: Some(0.001),
            output: Some(0.002),
            cache_read: Some(0.0001),
            cache_write: Some(0.0002),
        };
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cached_prompt_tokens: Some(20),
            cache_creation_tokens: Some(10),
            cache_read_tokens: None,
        };

        let total = ModelResolver::estimate_cost(pricing, &usage).expect("cost");
        assert!(total > 0.0);
    }
}
