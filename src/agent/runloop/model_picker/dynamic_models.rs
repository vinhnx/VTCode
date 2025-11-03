use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

use vtcode_core::config::constants::{env_vars, urls};
use vtcode_core::config::models::Provider;
use vtcode_core::llm::providers::lmstudio::fetch_lmstudio_models;
use vtcode_core::llm::providers::ollama::fetch_ollama_models;
use vtcode_core::utils::dot_config::{DotConfig, get_dot_manager, load_user_config};

use super::options::ModelOption;
use super::selection::{SelectionDetail, selection_from_dynamic};

const DYNAMIC_MODEL_CACHE_FILENAME: &str = "dynamic_local_models.json";
const DYNAMIC_MODEL_CACHE_TTL_SECS: u64 = 300;

type StaticModelIndex = HashMap<Provider, HashSet<String>>;
type CacheEntries = HashMap<String, CachedDynamicModelEntry>;

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
        let (lmstudio_result, lmstudio_warning) = cache_store
            .fetch_with_cache(
                Provider::LmStudio,
                endpoints.lmstudio.clone(),
                fetch_lmstudio_models,
            )
            .await;
        let (ollama_result, ollama_warning) = cache_store
            .fetch_with_cache(
                Provider::Ollama,
                endpoints.ollama.clone(),
                fetch_ollama_models,
            )
            .await;
        let _ = cache_store.persist().await;
        let mut registry = Self::default();
        registry.process_fetch(
            Provider::LmStudio,
            lmstudio_result,
            endpoints
                .lmstudio
                .clone()
                .unwrap_or_else(|| urls::LMSTUDIO_API_BASE.to_string()),
            &static_index,
        );
        if let Some(warning) = lmstudio_warning {
            registry.record_warning(Provider::LmStudio, warning);
        }
        registry.process_fetch(
            Provider::Ollama,
            ollama_result,
            endpoints
                .ollama
                .clone()
                .unwrap_or_else(|| urls::OLLAMA_API_BASE.to_string()),
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
        self.provider_errors.get(&provider).map(|msg| msg.as_str())
    }

    pub(super) fn warning_for(&self, provider: Provider) -> Option<&str> {
        self.provider_warnings
            .get(&provider)
            .map(|msg| msg.as_str())
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
                        "Failed to query {} at {} ({} )",
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
            let detail = selection_from_dynamic(provider, trimmed);
            self.register_model(provider, detail);
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

#[derive(Clone, Default)]
struct ProviderEndpointConfig {
    lmstudio: Option<String>,
    ollama: Option<String>,
}

impl ProviderEndpointConfig {
    async fn gather(workspace: Option<&Path>) -> Self {
        let _ = workspace;
        let dot_config = load_user_config().await.ok();
        Self {
            lmstudio: Self::extract_base_url(Provider::LmStudio, dot_config.as_ref()),
            ollama: Self::extract_base_url(Provider::Ollama, dot_config.as_ref()),
        }
    }

    fn extract_base_url(provider: Provider, dot_config: Option<&DotConfig>) -> Option<String> {
        let from_config = dot_config.and_then(|cfg| match provider {
            Provider::LmStudio => cfg
                .providers
                .lmstudio
                .as_ref()
                .and_then(|c| c.base_url.clone()),
            Provider::Ollama => cfg
                .providers
                .ollama
                .as_ref()
                .and_then(|c| c.base_url.clone()),
            _ => None,
        });

        from_config
            .and_then(Self::sanitize_owned)
            .or_else(|| Self::env_override(provider))
    }

    fn sanitize_owned(value: String) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn env_override(provider: Provider) -> Option<String> {
        let key = match provider {
            Provider::LmStudio => env_vars::LMSTUDIO_BASE_URL,
            Provider::Ollama => env_vars::OLLAMA_BASE_URL,
            _ => return None,
        };
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedDynamicModelEntry {
    provider: String,
    base_url: String,
    fetched_at: u64,
    models: Vec<String>,
}

struct CachedDynamicModelStore {
    entries: CacheEntries,
    dirty: bool,
}

impl CachedDynamicModelStore {
    async fn load() -> Self {
        let Some(path) = dynamic_model_cache_path() else {
            return Self {
                entries: HashMap::new(),
                dirty: false,
            };
        };
        match fs::read(&path).await {
            Ok(data) => match serde_json::from_slice::<CacheEntries>(&data) {
                Ok(entries) => Self {
                    entries,
                    dirty: false,
                },
                Err(_) => Self {
                    entries: HashMap::new(),
                    dirty: false,
                },
            },
            Err(_) => Self {
                entries: HashMap::new(),
                dirty: false,
            },
        }
    }

    async fn persist(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = dynamic_model_cache_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let serialized = serde_json::to_vec_pretty(&self.entries)?;
        fs::write(&path, serialized)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;
        self.dirty = false;
        Ok(())
    }

    async fn fetch_with_cache<F, Fut>(
        &mut self,
        provider: Provider,
        mut base_url: Option<String>,
        fetch_fn: F,
    ) -> (Result<Vec<String>>, Option<String>)
    where
        F: Fn(Option<String>) -> Fut,
        Fut: Future<Output = Result<Vec<String>, anyhow::Error>>,
    {
        if let Some(value) = base_url.take() {
            let trimmed = value.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                base_url = None;
            } else {
                base_url = Some(trimmed);
            }
        }

        let resolved_base = base_url
            .clone()
            .unwrap_or_else(|| default_provider_base(provider).to_string());
        let key = Self::cache_key(provider, &resolved_base);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(entry) = self.entries.get(&key)
            && now.saturating_sub(entry.fetched_at) <= DYNAMIC_MODEL_CACHE_TTL_SECS
        {
            return (Ok(entry.models.clone()), None);
        }

        match fetch_fn(base_url.clone()).await {
            Ok(models) => {
                self.entries.insert(
                    key,
                    CachedDynamicModelEntry {
                        provider: provider.to_string(),
                        base_url: resolved_base,
                        fetched_at: now,
                        models: models.clone(),
                    },
                );
                self.dirty = true;
                (Ok(models), None)
            }
            Err(err) => {
                if let Some(entry) = self.entries.get(&key) {
                    let warning = format!(
                        "Using cached {} models fetched {}s ago because {} was unreachable ({}).",
                        provider.label(),
                        now.saturating_sub(entry.fetched_at),
                        resolved_base,
                        err
                    );
                    return (Ok(entry.models.clone()), Some(warning));
                }
                (Err(err), None)
            }
        }
    }

    fn cache_key(provider: Provider, base_url: &str) -> String {
        format!("{}::{}", provider, base_url)
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

fn dynamic_model_cache_path() -> Option<PathBuf> {
    let manager = get_dot_manager().lock().ok()?.clone();
    Some(
        manager
            .cache_dir("models")
            .join(DYNAMIC_MODEL_CACHE_FILENAME),
    )
}

fn default_provider_base(provider: Provider) -> &'static str {
    match provider {
        Provider::LmStudio => urls::LMSTUDIO_API_BASE,
        Provider::Ollama => urls::OLLAMA_API_BASE,
        _ => "",
    }
}
