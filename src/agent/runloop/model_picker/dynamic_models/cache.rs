use hashbrown::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;
use vtcode_core::config::models::Provider;
use vtcode_core::utils::dot_config::get_dot_manager;
use vtcode_core::utils::file_utils::write_file_with_context;

use super::endpoints::default_provider_base;

const DYNAMIC_MODEL_CACHE_FILENAME: &str = "dynamic_local_models.json";
const DYNAMIC_MODEL_CACHE_TTL_SECS: u64 = 300;

type CacheEntries = HashMap<String, CachedDynamicModelEntry>;

#[derive(Debug, Serialize, Deserialize)]
struct CachedDynamicModelEntry {
    provider: String,
    base_url: String,
    fetched_at: u64,
    models: Vec<String>,
}

pub(super) struct CachedDynamicModelStore {
    entries: CacheEntries,
    dirty: bool,
}

impl CachedDynamicModelStore {
    pub(super) async fn load() -> Self {
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

    pub(super) async fn persist(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let Some(path) = dynamic_model_cache_path() else {
            return Ok(());
        };

        let serialized = serde_json::to_string_pretty(&self.entries)?;
        write_file_with_context(&path, &serialized, "dynamic model cache").await?;
        self.dirty = false;
        Ok(())
    }

    pub(super) async fn fetch_with_cache<F, Fut>(
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

fn dynamic_model_cache_path() -> Option<PathBuf> {
    let manager = get_dot_manager().ok()?.lock().ok()?.clone();
    Some(
        manager
            .cache_dir("models")
            .join(DYNAMIC_MODEL_CACHE_FILENAME),
    )
}
