mod cache;
mod endpoints;

use hashbrown::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use std::sync::LazyLock;
use reqwest::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use tracing::warn;
use vtcode_config::VTCodeConfig;
use vtcode_config::auth::AuthCredentialsStoreMode;
use vtcode_core::config::constants::defaults;
use vtcode_core::config::models::Provider;
use vtcode_core::copilot::{CopilotAuthStatus, CopilotAuthStatusKind, list_available_models, probe_auth_status};
use vtcode_core::llm::providers::llamacpp::fetch_llamacpp_models;
use vtcode_core::llm::providers::lmstudio::fetch_lmstudio_models;
use vtcode_core::llm::providers::ollama::fetch_ollama_models;

use self::cache::CachedDynamicModelStore;
use self::endpoints::ProviderEndpointConfig;

use super::options::ModelOption;
use super::selection::{SelectionDetail, selection_from_dynamic_with_api_key_env};

type StaticModelIndex = HashMap<Provider, HashSet<String>>;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
static HTTP_CLIENT: LazyLock<Client> =
    LazyLock::new(|| vtcode_commons::http::create_client_with_timeout(MODEL_DISCOVERY_TIMEOUT));

#[derive(Clone, Default)]
pub(crate) struct DynamicModelRegistry {
    pub(super) entries: Vec<SelectionDetail>,
    pub(super) provider_models: HashMap<Provider, Vec<usize>>,
    pub(super) provider_errors: HashMap<Provider, String>,
    pub(super) provider_warnings: HashMap<Provider, String>,
    pub(super) seen_model_ids: HashSet<String>,
}

impl DynamicModelRegistry {
    pub(super) async fn load(options: &[ModelOption], workspace: Option<&Path>, vt_cfg: Option<&VTCodeConfig>) -> Self {
        let static_index = build_static_model_index(options);
        let mut registry = Self {
            seen_model_ids: static_index.values().flatten().cloned().collect(),
            ..Self::default()
        };
        let (endpoints, mut cache_store) =
            tokio::join!(ProviderEndpointConfig::gather(workspace), CachedDynamicModelStore::load());
        let workspace_root = Arc::new(
            workspace
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        );

        let openai_base_url = endpoints.resolved_base_url(Provider::OpenAI);
        let openai_api_key_env = configured_openai_api_key_env(vt_cfg);
        let openai_auth = if cache_store.fresh_models(Provider::OpenAI, Some(&openai_base_url)).is_some() {
            None
        } else {
            resolve_openai_dynamic_auth(vt_cfg, workspace, openai_api_key_env.as_deref())
        };
        let ollama_base_url = endpoints.resolved_base_url(Provider::Ollama);
        let llamacpp_base_url = endpoints.resolved_base_url(Provider::LlamaCpp);
        let lmstudio_base_url = endpoints.resolved_base_url(Provider::LmStudio);
        let copilot_auth_cfg = Arc::new(vt_cfg.map(|cfg| cfg.auth.copilot.clone()).unwrap_or_default());

        // Discovery is best-effort for four independent providers. Keep each
        // provider's cache view isolated while the requests run concurrently;
        // merging whole cloned stores could otherwise let a stale clone
        // overwrite a fresh result from another provider.
        let (openai_state, ollama_state, llamacpp_state, lmstudio_state, copilot_state) = tokio::join!(
            fetch_openai_with_cache(
                cache_store.for_provider(Provider::OpenAI),
                endpoints.base_url(Provider::OpenAI),
                openai_auth,
            ),
            fetch_provider_with_cache(
                cache_store.for_provider(Provider::Ollama),
                Provider::Ollama,
                endpoints.base_url(Provider::Ollama),
                fetch_ollama_models,
            ),
            fetch_provider_with_cache(
                cache_store.for_provider(Provider::LlamaCpp),
                Provider::LlamaCpp,
                endpoints.base_url(Provider::LlamaCpp),
                fetch_llamacpp_models,
            ),
            fetch_provider_with_cache(
                cache_store.for_provider(Provider::LmStudio),
                Provider::LmStudio,
                endpoints.base_url(Provider::LmStudio),
                fetch_lmstudio_models,
            ),
            fetch_copilot_with_cache(cache_store.for_provider(Provider::Copilot), copilot_auth_cfg, workspace_root),
        );
        let (openai_store, openai_fetch) = openai_state;
        let (ollama_store, ollama_result) = ollama_state;
        let (llamacpp_store, llamacpp_result) = llamacpp_state;
        let (lmstudio_store, lmstudio_result) = lmstudio_state;
        let (copilot_store, copilot_status, copilot_fetch) = copilot_state;
        cache_store.merge_provider(Provider::OpenAI, openai_store);
        cache_store.merge_provider(Provider::Ollama, ollama_store);
        cache_store.merge_provider(Provider::LlamaCpp, llamacpp_store);
        cache_store.merge_provider(Provider::LmStudio, lmstudio_store);
        cache_store.merge_provider(Provider::Copilot, copilot_store);
        if let Err(err) = cache_store.persist().await {
            warn!("Failed to persist dynamic model cache: {err}");
        }

        if let Some((openai_result, openai_warning)) = openai_fetch {
            registry.process_fetch(
                Provider::OpenAI,
                openai_result,
                openai_base_url,
                &static_index,
                vt_cfg.map(|cfg| cfg.agent.credential_storage_mode).unwrap_or_default(),
                openai_api_key_env.as_deref(),
            );
            if let Some(warning) = openai_warning {
                registry.record_warning(Provider::OpenAI, warning);
            }
        }
        let storage_mode = vt_cfg.map(|cfg| cfg.agent.credential_storage_mode).unwrap_or_default();
        let (ollama_result, ollama_warning) = ollama_result;
        registry.process_fetch(Provider::Ollama, ollama_result, ollama_base_url, &static_index, storage_mode, None);
        if let Some(warning) = ollama_warning {
            registry.record_warning(Provider::Ollama, warning);
        }
        let (llamacpp_result, llamacpp_warning) = llamacpp_result;
        registry.process_fetch(
            Provider::LlamaCpp,
            llamacpp_result,
            llamacpp_base_url,
            &static_index,
            storage_mode,
            None,
        );
        if let Some(warning) = llamacpp_warning {
            registry.record_warning(Provider::LlamaCpp, warning);
        }
        let (lmstudio_result, lmstudio_warning) = lmstudio_result;
        registry.process_fetch(
            Provider::LmStudio,
            lmstudio_result,
            lmstudio_base_url,
            &static_index,
            storage_mode,
            None,
        );
        if let Some(warning) = lmstudio_warning {
            registry.record_warning(Provider::LmStudio, warning);
        }
        if let Some((copilot_result, copilot_warning)) = copilot_fetch {
            registry.process_fetch(
                Provider::Copilot,
                copilot_result,
                "copilot-cli".to_string(),
                &static_index,
                storage_mode,
                None,
            );
            if let Some(warning) = copilot_warning {
                registry.record_warning(Provider::Copilot, warning);
            }
        } else {
            match copilot_status.kind {
                CopilotAuthStatusKind::Unauthenticated => registry.record_warning(
                    Provider::Copilot,
                    "Run `vtcode login copilot` to load the live GitHub Copilot model list. `copilot-auto` remains available.".to_string(),
                ),
                CopilotAuthStatusKind::ServerUnavailable => registry.record_error(
                    Provider::Copilot,
                    copilot_status
                        .message
                        .unwrap_or_else(|| "GitHub Copilot CLI is unavailable.".to_string()),
                ),
                CopilotAuthStatusKind::AuthFlowFailed => registry.record_warning(
                    Provider::Copilot,
                    copilot_status
                        .message
                        .unwrap_or_else(|| "GitHub Copilot authentication needs attention.".to_string()),
                ),
                CopilotAuthStatusKind::Authenticated => {}
            }
        }
        registry
    }

    pub(super) fn indexes_for(&self, provider: Provider) -> &[usize] {
        self.provider_models.get(&provider).map(Vec::as_slice).unwrap_or(&[])
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
        storage_mode: AuthCredentialsStoreMode,
        api_key_env: Option<&str>,
    ) {
        match result {
            Ok(models) => self.register_provider_models(provider, models, static_index, storage_mode, api_key_env),
            Err(err) => {
                self.record_error(provider, format!("Failed to query {} at {} ({})", provider.label(), base_url, err));
            }
        }
    }

    fn register_provider_models(
        &mut self,
        provider: Provider,
        models: Vec<String>,
        static_index: &StaticModelIndex,
        storage_mode: AuthCredentialsStoreMode,
        api_key_env: Option<&str>,
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
            if static_index.get(&provider).is_some_and(|set| set.contains(&lower)) {
                continue;
            }
            if !self.seen_model_ids.insert(lower) {
                continue;
            }
            if provider == Provider::Ollama && (trimmed.contains(":cloud") || trimmed.contains("-cloud")) {
                continue;
            }

            self.register_model(
                provider,
                selection_from_dynamic_with_api_key_env(
                    provider,
                    trimmed,
                    trimmed,
                    None,
                    None,
                    api_key_env,
                    storage_mode,
                ),
            );
        }
    }

    fn register_model(&mut self, provider: Provider, detail: SelectionDetail) {
        let index = self.entries.len();
        self.entries.push(detail);
        self.provider_models.entry(provider).or_default().push(index);
    }

    fn record_error(&mut self, provider: Provider, message: String) {
        self.provider_errors.insert(provider, message);
    }

    pub(super) fn record_warning(&mut self, provider: Provider, message: String) {
        self.provider_warnings.insert(provider, message);
    }
}

async fn fetch_provider_with_cache<F, Fut>(
    mut cache_store: CachedDynamicModelStore,
    provider: Provider,
    base_url: Option<String>,
    fetch_fn: F,
) -> (CachedDynamicModelStore, (Result<Vec<String>>, Option<String>))
where
    F: Fn(Option<String>) -> Fut,
    Fut: Future<Output = Result<Vec<String>, anyhow::Error>>,
{
    let fetched = cache_store.fetch_with_cache(provider, base_url, fetch_fn).await;
    (cache_store, fetched)
}

async fn fetch_openai_with_cache(
    mut cache_store: CachedDynamicModelStore,
    base_url: Option<String>,
    api_key: Option<String>,
) -> (CachedDynamicModelStore, Option<(Result<Vec<String>>, Option<String>)>) {
    let fetched = if let Some(api_key) = api_key {
        let (result, warning) = cache_store
            .fetch_with_cache(Provider::OpenAI, base_url, move |base_url| {
                fetch_openai_models(base_url, api_key.clone())
            })
            .await;
        Some((result, warning))
    } else {
        cache_store
            .fresh_models(Provider::OpenAI, base_url.as_deref())
            .map(|models| (Ok(models), None))
    };
    (cache_store, fetched)
}

async fn fetch_copilot_with_cache(
    mut cache_store: CachedDynamicModelStore,
    copilot_auth_cfg: Arc<vtcode_config::auth::CopilotAuthConfig>,
    workspace_root: Arc<PathBuf>,
) -> (CachedDynamicModelStore, CopilotAuthStatus, Option<(Result<Vec<String>>, Option<String>)>) {
    let cache_base = copilot_cache_base(&copilot_auth_cfg);
    // A fresh cache is sufficient to populate the picker. Avoid probing the
    // Copilot CLI/GitHub authentication path in this case because an optional
    // `gh auth status` fallback can take several seconds when offline.
    if let Some(models) = cache_store.fresh_models(Provider::Copilot, Some(&cache_base)) {
        return (
            cache_store,
            CopilotAuthStatus {
                kind: CopilotAuthStatusKind::Authenticated,
                message: None,
            },
            Some((Ok(models), None)),
        );
    }

    let copilot_status = probe_auth_status(&copilot_auth_cfg, Some(workspace_root.as_path())).await;
    let copilot_fetch = if matches!(copilot_status.kind, CopilotAuthStatusKind::Authenticated) {
        let copilot_auth_cfg = Arc::clone(&copilot_auth_cfg);
        let workspace_root = Arc::clone(&workspace_root);
        let (result, warning) = cache_store
            .fetch_with_cache(Provider::Copilot, Some(cache_base), move |_| {
                let copilot_auth_cfg = Arc::clone(&copilot_auth_cfg);
                let workspace_root = Arc::clone(&workspace_root);
                async move {
                    let models = list_available_models(&copilot_auth_cfg, workspace_root.as_path()).await?;
                    Ok(models.into_iter().map(|model| model.id).collect())
                }
            })
            .await;
        Some((result, warning))
    } else {
        // A recent Copilot cache is still useful when the CLI is unavailable
        // or authentication has expired. The closure fails immediately, so
        // stale-cache fallback never starts a process in this path.
        let (result, warning) = cache_store
            .fetch_with_cache(Provider::Copilot, Some(cache_base), |_| async {
                Err::<Vec<String>, anyhow::Error>(anyhow!("Copilot authentication is unavailable"))
            })
            .await;
        result.ok().map(|models| (Ok(models), warning))
    };
    (cache_store, copilot_status, copilot_fetch)
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

fn resolve_openai_dynamic_auth(
    vt_cfg: Option<&VTCodeConfig>,
    workspace: Option<&Path>,
    api_key_env: Option<&str>,
) -> Option<String> {
    let auth_config = vt_cfg.map(|cfg| cfg.auth.openai.clone()).unwrap_or_default();
    let storage_mode = vt_cfg.map(|cfg| cfg.agent.credential_storage_mode).unwrap_or_default();
    let default_api_key_env = Provider::OpenAI.default_api_key_env();
    let requested_api_key_env = api_key_env.unwrap_or(default_api_key_env);
    let is_default_key = requested_api_key_env.eq_ignore_ascii_case(default_api_key_env);
    let api_key =
        vtcode_config::api_keys::resolve_credential_with_mode("openai", requested_api_key_env, workspace, storage_mode)
            .ok()
            .flatten()
            .and_then(|resolved| resolved.secret);

    if !is_default_key && api_key.is_none() {
        return None;
    }

    let auth_config = if is_default_key {
        auth_config
    } else {
        let mut api_key_auth_config = auth_config;
        api_key_auth_config.preferred_method = vtcode_config::OpenAIPreferredMethod::ApiKey;
        api_key_auth_config
    };
    vtcode_config::resolve_openai_auth(&auth_config, storage_mode, api_key)
        .ok()
        .map(|resolved| resolved.api_key().to_string())
}

fn configured_openai_api_key_env(vt_cfg: Option<&VTCodeConfig>) -> Option<String> {
    let cfg = vt_cfg?;
    cfg.configured_api_key_env(Provider::OpenAI.as_ref()).or_else(|| {
        if !cfg.agent.provider.eq_ignore_ascii_case(Provider::OpenAI.as_ref()) {
            return None;
        }
        let configured = cfg.agent.api_key_env.trim();
        if configured.is_empty()
            || configured.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
            || configured.eq_ignore_ascii_case(Provider::OpenAI.default_api_key_env())
        {
            None
        } else {
            Some(configured.to_owned())
        }
    })
}

fn copilot_cache_base(config: &vtcode_config::auth::CopilotAuthConfig) -> String {
    config
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("copilot-cli://{}", value.trim_end_matches('/')))
        .unwrap_or_else(|| "copilot-cli://github.com".to_string())
}

async fn fetch_openai_models(base_url: Option<String>, api_key: String) -> Result<Vec<String>, anyhow::Error> {
    #[derive(Debug, Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    #[derive(Debug, Deserialize)]
    struct ModelEntry {
        id: String,
    }

    let resolved_base = base_url
        .unwrap_or_else(|| endpoints::default_provider_base(Provider::OpenAI).to_string())
        .trim_end_matches('/')
        .to_string();
    let models_url = format!("{resolved_base}/models");
    let response = HTTP_CLIENT
        .get(&models_url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|err| anyhow!("failed to connect to OpenAI models endpoint: {err}"))?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(anyhow!("OpenAI authentication failed while listing remote models"));
    }
    if !response.status().is_success() {
        return Err(anyhow!("failed to fetch OpenAI models: HTTP {}", response.status()));
    }

    let parsed: ModelsResponse = response
        .json()
        .await
        .map_err(|err| anyhow!("failed to parse OpenAI models response: {err}"))?;

    Ok(parsed
        .data
        .into_iter()
        .map(|entry| entry.id)
        .filter(|id| is_supported_openai_remote_model(id))
        .collect())
}

fn is_supported_openai_remote_model(model_id: &str) -> bool {
    let lower = model_id.to_ascii_lowercase();
    lower.starts_with("gpt")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("codex")
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
            AuthCredentialsStoreMode::default(),
            None,
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
            AuthCredentialsStoreMode::default(),
            None,
        );

        let indexes = registry.indexes_for(Provider::Ollama);
        assert_eq!(indexes.len(), 1);
        let detail = registry.detail(indexes[0]).expect("only local dynamic model should remain");
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
            AuthCredentialsStoreMode::default(),
            None,
        );

        assert!(
            registry
                .error_for(Provider::Ollama)
                .expect("error should be captured")
                .contains("boom")
        );
    }

    #[test]
    fn copilot_cache_base_defaults_to_github_com() {
        assert_eq!(copilot_cache_base(&vtcode_config::auth::CopilotAuthConfig::default()), "copilot-cli://github.com");
    }

    #[test]
    fn register_provider_models_carries_api_key_override() {
        let static_index = build_static_model_index(MODEL_OPTIONS.as_slice());
        let mut registry = DynamicModelRegistry::default();

        registry.register_provider_models(
            Provider::OpenAI,
            vec!["gpt-corporate".to_string()],
            &static_index,
            AuthCredentialsStoreMode::File,
            Some("CORPORATE_OPENAI_KEY"),
        );

        let index = registry.indexes_for(Provider::OpenAI)[0];
        let detail = registry.detail(index).expect("dynamic selection detail");
        assert_eq!(detail.env_key, "CORPORATE_OPENAI_KEY");
    }
}
