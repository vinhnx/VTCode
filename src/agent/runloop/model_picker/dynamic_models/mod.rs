mod cache;
mod endpoints;

use hashbrown::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use vtcode_core::config::models::Provider;
use vtcode_core::llm::providers::ollama::fetch_ollama_models;

use self::cache::CachedDynamicModelStore;
use self::endpoints::ProviderEndpointConfig;

use super::options::ModelOption;
use super::selection::{SelectionDetail, selection_from_dynamic};

type StaticModelIndex = HashMap<Provider, HashSet<String>>;

#[derive(Clone, Default)]
pub(super) struct DynamicModelRegistry {
    pub(super) entries: Vec<SelectionDetail>,
    pub(super) provider_models: HashMap<Provider, Vec<usize>>,
    pub(super) provider_errors: HashMap<Provider, String>,
    pub(super) provider_warnings: HashMap<Provider, String>,
}

impl DynamicModelRegistry {
    pub(super) async fn load(options: &[ModelOption], workspace: Option<&Path>) -> Self {
        let endpoints = ProviderEndpointConfig::gather(workspace).await;
        let static_index = build_static_model_index(options);
        let mut cache_store = CachedDynamicModelStore::load().await;

        let ollama_base_url = endpoints.resolved_base_url(Provider::Ollama);
        let (ollama_result, ollama_warning) = cache_store
            .fetch_with_cache(
                Provider::Ollama,
                endpoints.base_url(Provider::Ollama),
                fetch_ollama_models,
            )
            .await;
        let _ = cache_store.persist().await;

        let mut registry = Self::default();
        registry.process_fetch(
            Provider::Ollama,
            ollama_result,
            ollama_base_url,
            &static_index,
        );
        if let Some(warning) = ollama_warning {
            registry.record_warning(Provider::Ollama, warning);
        }
        registry
    }

    pub(super) fn indexes_for(&self, provider: Provider) -> Vec<usize> {
        self.provider_models
            .get(&provider)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn detail(&self, index: usize) -> Option<&SelectionDetail> {
        self.entries.get(index)
    }

    pub(super) fn dynamic_detail(&self, index: usize) -> Option<SelectionDetail> {
        self.entries.get(index).cloned()
    }

    pub(super) fn error_for(&self, provider: Provider) -> Option<&str> {
        self.provider_errors.get(&provider).map(String::as_str)
    }

    pub(super) fn warning_for(&self, provider: Provider) -> Option<&str> {
        self.provider_warnings.get(&provider).map(String::as_str)
    }

    fn process_fetch(
        &mut self,
        provider: Provider,
        result: Result<Vec<String>>,
        base_url: String,
        static_index: &StaticModelIndex,
    ) {
        match result {
            Ok(models) => self.register_provider_models(provider, models, static_index),
            Err(err) => {
                self.record_error(
                    provider,
                    format!(
                        "Failed to query {} at {} ({})",
                        provider.label(),
                        base_url,
                        err
                    ),
                );
            }
        }
    }

    fn register_provider_models(
        &mut self,
        provider: Provider,
        models: Vec<String>,
        static_index: &StaticModelIndex,
    ) {
        if !models.is_empty() {
            self.provider_errors.remove(&provider);
            self.provider_warnings.remove(&provider);
        }

        for model_id in models {
            let trimmed = model_id.trim();
            if trimmed.is_empty() {
                continue;
            }

            let lower = trimmed.to_ascii_lowercase();
            if static_index
                .get(&provider)
                .is_some_and(|set| set.contains(&lower))
            {
                continue;
            }
            if self.has_model(provider, trimmed) {
                continue;
            }
            if provider == Provider::Ollama
                && (trimmed.contains(":cloud") || trimmed.contains("-cloud"))
            {
                continue;
            }

            self.register_model(provider, selection_from_dynamic(provider, trimmed));
        }
    }

    fn register_model(&mut self, provider: Provider, detail: SelectionDetail) {
        let index = self.entries.len();
        self.entries.push(detail);
        self.provider_models
            .entry(provider)
            .or_default()
            .push(index);
    }

    fn has_model(&self, provider: Provider, candidate: &str) -> bool {
        if let Some(indexes) = self.provider_models.get(&provider) {
            for index in indexes {
                if let Some(entry) = self.entries.get(*index)
                    && entry.model_id.eq_ignore_ascii_case(candidate)
                {
                    return true;
                }
            }
        }
        false
    }

    fn record_error(&mut self, provider: Provider, message: String) {
        self.provider_errors.insert(provider, message);
    }

    pub(super) fn record_warning(&mut self, provider: Provider, message: String) {
        self.provider_warnings.insert(provider, message);
    }
}

fn build_static_model_index(options: &[ModelOption]) -> StaticModelIndex {
    let mut index = HashMap::new();
    for option in options {
        index
            .entry(option.provider)
            .or_insert_with(HashSet::new)
            .insert(option.id.to_ascii_lowercase());
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::model_picker::options::MODEL_OPTIONS;

    #[test]
    fn register_provider_models_adds_new_dynamic_models() {
        let static_index = build_static_model_index(MODEL_OPTIONS.as_slice());
        let mut registry = DynamicModelRegistry::default();

        registry.register_provider_models(
            Provider::Ollama,
            vec!["custom-local-model".to_string()],
            &static_index,
        );

        let indexes = registry.indexes_for(Provider::Ollama);
        assert_eq!(indexes.len(), 1);
        let detail = registry
            .detail(indexes[0])
            .expect("dynamic selection detail should be recorded");
        assert_eq!(detail.model_id, "custom-local-model");
    }

    #[test]
    fn register_provider_models_skips_known_and_cloud_models() {
        let static_index = build_static_model_index(MODEL_OPTIONS.as_slice());
        let mut registry = DynamicModelRegistry::default();
        let known_ollama_model = MODEL_OPTIONS
            .iter()
            .find(|option| option.provider == Provider::Ollama)
            .expect("expected at least one built-in Ollama model")
            .id
            .to_string();

        registry.register_provider_models(
            Provider::Ollama,
            vec![
                known_ollama_model,
                "custom-cloud-model:cloud".to_string(),
                "custom-local-model".to_string(),
            ],
            &static_index,
        );

        let indexes = registry.indexes_for(Provider::Ollama);
        assert_eq!(indexes.len(), 1);
        let detail = registry
            .detail(indexes[0])
            .expect("only local dynamic model should remain");
        assert_eq!(detail.model_id, "custom-local-model");
    }

    #[test]
    fn process_fetch_records_provider_error() {
        let static_index = build_static_model_index(MODEL_OPTIONS.as_slice());
        let mut registry = DynamicModelRegistry::default();

        registry.process_fetch(
            Provider::Ollama,
            Err(anyhow::anyhow!("boom")),
            "http://localhost:11434/api".to_string(),
            &static_index,
        );

        assert!(
            registry
                .error_for(Provider::Ollama)
                .expect("error should be captured")
                .contains("boom")
        );
    }
}
