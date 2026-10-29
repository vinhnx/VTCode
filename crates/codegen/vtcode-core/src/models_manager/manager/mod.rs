//! Models Manager - Coordinates model discovery, caching, and selection.
//!
//! This module provides the main `ModelsManager` struct that coordinates:
//! - Local model presets (built-in configurations)
//! - Remote model discovery (fetching from provider APIs)
//! - Disk caching with TTL
//! - Model family resolution

use anyhow::Context;
use chrono::Utc;
use hashbrown::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info};

use super::cache::{self, ModelsCache};
use super::model_family::{ModelFamily, find_family_for_model};
use super::model_presets::{
    ModelInfo, ModelPreset, ReasoningEffortPreset, builtin_model_presets, presets_for_provider,
};
use crate::config::models::Provider;
use crate::llm::providers::{
    MergeCatalogAvailability, MergeCatalogFilters, MergeCatalogModel, MergeGatewayCatalogClient,
    llamacpp::fetch_llamacpp_models,
};
use vtcode_commons::VtCodePaths;
use vtcode_config::constants::{env_vars, urls};

/// Legacy cache file name retained for one-time compatible reads.
const LEGACY_MODEL_CACHE_FILE: &str = "models_cache.json";

/// Default cache TTL (2 minutes)
const DEFAULT_MODEL_CACHE_TTL: Duration = Duration::from_secs(120);

/// Default model for Gemini provider
const GEMINI_DEFAULT_MODEL: &str = "gemini-3-flash-preview";

/// Default model for OpenAI provider
const OPENAI_DEFAULT_MODEL: &str = "gpt-5.6-sol";

/// Default model for Anthropic provider
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-sonnet-5";

/// Coordinates remote model discovery plus cached metadata on disk.
#[derive(Debug)]
pub struct ModelsManager {
    /// Local built-in model presets
    local_models: Vec<ModelPreset>,
    /// Remote models fetched from provider APIs
    remote_models: RwLock<Vec<ModelInfo>>,
    /// ETag for conditional requests
    etag: RwLock<Option<String>>,
    /// VT Code home directory for cache storage
    vtcode_home: PathBuf,
    /// Cache TTL
    cache_ttl: Duration,
    /// Current active provider
    current_provider: RwLock<Provider>,
    /// Monotonically increasing generation used to reject late refresh results.
    refresh_generation: AtomicU64,
    /// Whether remote model fetching is enabled
    remote_models_enabled: bool,
    /// Serializes cache writes so stale concurrent refreshes cannot overwrite newer snapshots.
    cache_write_lock: Mutex<()>,
}

impl Default for ModelsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelsManager {
    /// Construct a new ModelsManager with default settings
    pub fn new() -> Self {
        let vtcode_home = Self::default_vtcode_home();
        Self {
            local_models: builtin_model_presets(),
            remote_models: RwLock::new(Vec::new()),
            etag: RwLock::new(None),
            vtcode_home,
            cache_ttl: DEFAULT_MODEL_CACHE_TTL,
            current_provider: RwLock::new(Provider::default()),
            refresh_generation: AtomicU64::new(0),
            remote_models_enabled: true,
            cache_write_lock: Mutex::new(()),
        }
    }

    /// Construct with a specific home directory
    pub fn with_home(vtcode_home: PathBuf) -> Self {
        Self {
            local_models: builtin_model_presets(),
            remote_models: RwLock::new(Vec::new()),
            etag: RwLock::new(None),
            vtcode_home,
            cache_ttl: DEFAULT_MODEL_CACHE_TTL,
            current_provider: RwLock::new(Provider::default()),
            refresh_generation: AtomicU64::new(0),
            remote_models_enabled: true,
            cache_write_lock: Mutex::new(()),
        }
    }

    /// Construct with a specific provider
    pub fn with_provider(provider: Provider) -> Self {
        let vtcode_home = Self::default_vtcode_home();
        Self {
            local_models: presets_for_provider(provider),
            remote_models: RwLock::new(Vec::new()),
            etag: RwLock::new(None),
            vtcode_home,
            cache_ttl: DEFAULT_MODEL_CACHE_TTL,
            current_provider: RwLock::new(provider),
            refresh_generation: AtomicU64::new(0),
            remote_models_enabled: true,
            cache_write_lock: Mutex::new(()),
        }
    }

    /// Construct with specific home directory and provider
    pub fn with_home_and_provider(vtcode_home: PathBuf, provider: Provider) -> Self {
        Self {
            local_models: presets_for_provider(provider),
            remote_models: RwLock::new(Vec::new()),
            etag: RwLock::new(None),
            vtcode_home,
            cache_ttl: DEFAULT_MODEL_CACHE_TTL,
            current_provider: RwLock::new(provider),
            refresh_generation: AtomicU64::new(0),
            remote_models_enabled: true,
            cache_write_lock: Mutex::new(()),
        }
    }

    /// Enable or disable remote model fetching
    pub fn set_remote_models_enabled(&mut self, enabled: bool) {
        self.remote_models_enabled = enabled;
    }

    /// Set the cache TTL
    pub fn set_cache_ttl(&mut self, ttl: Duration) {
        self.cache_ttl = ttl;
    }

    /// Get the default VT Code home directory
    fn default_vtcode_home() -> PathBuf {
        VtCodePaths::resolve()
            .map(|paths| paths.cache_dir().to_path_buf())
            .unwrap_or_else(|_| PathBuf::from(".cache/vtcode"))
    }

    /// Refresh available models, using cache if fresh
    pub async fn refresh_available_models(&self) -> anyhow::Result<()> {
        if !self.remote_models_enabled {
            debug!("Remote model fetching is disabled");
            return Ok(());
        }

        let provider = *self.current_provider.read().await;
        let generation = self.refresh_generation.fetch_add(1, Ordering::AcqRel) + 1;

        // Try to load from cache first
        if self.try_load_cache_for_at(provider, generation).await {
            debug!("Using cached models");
            return Ok(());
        }

        match provider {
            Provider::Ollama => {
                debug!("Fetching remote models for Ollama...");
                match self.fetch_ollama_models().await {
                    Ok(models) => {
                        info!("Fetched {} models from Ollama", models.len());
                        if self
                            .apply_remote_state_for_provider_at(provider, generation, models.clone(), None)
                            .await
                        {
                            self.persist_cache_for_at(provider, generation, &models, None).await;
                        }
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to fetch Ollama models: {e}");
                        // Fall back to local presets if fetch fails
                        Ok(())
                    }
                }
            }
            Provider::LlamaCpp => {
                debug!("Fetching remote models for llama.cpp...");
                match self.fetch_llamacpp_models().await {
                    Ok(models) => {
                        info!("Fetched {} models from llama.cpp", models.len());
                        if self
                            .apply_remote_state_for_provider_at(provider, generation, models.clone(), None)
                            .await
                        {
                            self.persist_cache_for_at(provider, generation, &models, None).await;
                        }
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to fetch llama.cpp models: {e}");
                        Ok(())
                    }
                }
            }
            Provider::MergeGateway => self.refresh_merge_gateway_models(generation).await,
            _ => {
                // For other providers, we don't have remote discovery yet
                info!("Remote model discovery for {:?} not implemented, using local presets", provider);
                Ok(())
            }
        }
    }

    async fn refresh_merge_gateway_models(&self, generation: u64) -> anyhow::Result<()> {
        let api_key = std::env::var(env_vars::MERGE_GATEWAY_API_KEY)
            .ok()
            .filter(|value| !value.trim().is_empty());
        let Some(api_key) = api_key else {
            debug!("Merge Gateway API key is unavailable; using cached or static models");
            let _ = self.try_load_stale_cache_for_at(Provider::MergeGateway, generation).await;
            return Ok(());
        };

        let base_url = std::env::var(env_vars::MERGE_GATEWAY_BASE_URL)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| urls::MERGE_GATEWAY_NATIVE_API_BASE.to_string());
        let client = MergeGatewayCatalogClient::try_with_timeouts(api_key, base_url, None)
            .context("failed to initialize Merge Gateway catalog client")?;
        let cached = self.load_cache_for(Provider::MergeGateway).await;
        let etag = self
            .etag
            .read()
            .await
            .clone()
            .or_else(|| cached.as_ref().and_then(|cache| cache.etag.clone()));

        match client.fetch_snapshot(&MergeCatalogFilters::default(), etag.as_deref()).await {
            Ok(None) => {
                debug!("Merge Gateway catalog is unchanged");
                if let Some(cache) = cached {
                    let _ = self
                        .apply_remote_state_for_provider_at(
                            Provider::MergeGateway,
                            generation,
                            cache.models,
                            cache.etag,
                        )
                        .await;
                }
                Ok(())
            }
            Ok(Some(snapshot)) => {
                let models = snapshot
                    .models
                    .into_iter()
                    .filter_map(Self::merge_catalog_model_info)
                    .collect::<Vec<_>>();
                if self
                    .apply_remote_state_for_provider_at(
                        Provider::MergeGateway,
                        generation,
                        models.clone(),
                        snapshot.etag.clone(),
                    )
                    .await
                {
                    self.persist_cache_for_at(Provider::MergeGateway, generation, &models, snapshot.etag)
                        .await;
                }
                Ok(())
            }
            Err(error) => {
                error!("Failed to fetch Merge Gateway model catalog: {error}");
                let _ = self.try_load_stale_cache_for_at(Provider::MergeGateway, generation).await;
                Ok(())
            }
        }
    }

    fn merge_catalog_model_info(model: MergeCatalogModel) -> Option<ModelInfo> {
        if model.availability != MergeCatalogAvailability::Available {
            return None;
        }

        let display_name = model.display_name.unwrap_or_else(|| model.model.clone());
        let supported_reasoning_levels = Self::merge_supported_reasoning_presets(model.supports_reasoning);
        Some(ModelInfo {
            slug: model.model.clone(),
            display_name,
            description: format!("Merge Gateway model: {}", model.model),
            provider: Provider::MergeGateway,
            default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
            supported_reasoning_levels,
            context_window: model.context_window.map(i64::from),
            supports_tool_use: model.supports_tool_use,
            supports_streaming: model.supports_streaming,
            supports_vision: model.supports_vision,
            supports_structured_output: model.supports_structured_output,
            supports_reasoning: model.supports_reasoning,
            max_output_tokens: model.max_output_tokens.map(i64::from),
            priority: 50,
            visibility: "list".to_string(),
            supported_in_api: true,
            upgrade: None,
        })
    }

    fn merge_supported_reasoning_presets(supports_reasoning: bool) -> Vec<ReasoningEffortPreset> {
        if !supports_reasoning {
            return Vec::new();
        }

        use crate::config::types::ReasoningEffortLevel;

        vec![
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Minimal,
                description: "Minimal reasoning depth".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast responses with lightweight reasoning".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced depth and speed".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::High,
                description: "Deep reasoning for complex problems".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::XHigh,
                description: "Extra reasoning for the hardest long-running tasks".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Max,
                description: "Maximum reasoning depth".to_string(),
            },
        ]
    }

    /// Fetch models from Ollama API
    async fn fetch_ollama_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let client = reqwest::Client::new();
        let resp = client.get("http://localhost:11434/api/tags").send().await?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Ollama API returned {}", resp.status()));
        }

        let json: serde_json::Value = resp.json().await?;
        let mut models = Vec::new();

        if let Some(ollama_models) = json.get("models").and_then(|m| m.as_array()) {
            for m in ollama_models {
                if let Some(name) = m.get("name").and_then(|s| s.as_str()) {
                    models.push(ModelInfo {
                        slug: name.to_string(),
                        display_name: format!("{name} (Ollama)"),
                        description: format!("Ollama model: {name}"),
                        provider: Provider::Ollama,
                        default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
                        supported_reasoning_levels: vec![],
                        context_window: Some(32_000), // Default for most Ollama models
                        supports_tool_use: true,
                        supports_streaming: true,
                        supports_vision: false,
                        supports_structured_output: false,
                        supports_reasoning: false,
                        max_output_tokens: None,
                        priority: 100,
                        visibility: "list".to_string(),
                        supported_in_api: true,
                        upgrade: None,
                    });
                }
            }
        }

        Ok(models)
    }

    async fn fetch_llamacpp_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        for model in fetch_llamacpp_models(None).await? {
            models.push(ModelInfo {
                slug: model.clone(),
                display_name: format!("{model} (llama.cpp)"),
                description: format!("llama.cpp model: {model}"),
                provider: Provider::LlamaCpp,
                default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
                supported_reasoning_levels: vec![],
                context_window: Some(131_072),
                supports_tool_use: true,
                supports_streaming: true,
                supports_vision: false,
                supports_structured_output: false,
                supports_reasoning: true,
                max_output_tokens: None,
                priority: 100,
                visibility: "list".to_string(),
                supported_in_api: true,
                upgrade: None,
            });
        }

        Ok(models)
    }

    /// List available models for the current provider
    pub async fn list_models(&self) -> Vec<ModelPreset> {
        if let Err(err) = self.refresh_available_models().await {
            error!("Failed to refresh available models: {err}");
        }
        let remote_models = self.remote_models.read().await;
        self.build_available_models(remote_models.clone())
    }

    /// List available models for a specific provider
    pub async fn list_models_for_provider(&self, provider: Provider) -> Vec<ModelPreset> {
        let all_models = self.list_models().await;
        all_models.into_iter().filter(|m| m.provider == provider).collect()
    }

    /// Try to list models without async refresh (uses cache only)
    pub fn try_list_models(&self) -> Result<Vec<ModelPreset>, tokio::sync::TryLockError> {
        let remote_models = self.remote_models.try_read()?;
        Ok(self.build_available_models(remote_models.clone()))
    }

    /// Get the model family for a given model slug
    pub async fn construct_model_family(&self, model: &str) -> ModelFamily {
        find_family_for_model(model)
    }

    /// Get the model to use, resolving defaults if not specified
    pub async fn get_model(&self, model: Option<&str>) -> String {
        if let Some(m) = model {
            return m.to_string();
        }

        // Refresh models to ensure we have the latest
        if let Err(err) = self.refresh_available_models().await {
            error!("Failed to refresh available models: {err}");
        }

        // Return default for current provider
        let provider = *self.current_provider.read().await;
        self.get_default_model_for_provider(provider)
    }

    /// Get the default model for a specific provider
    pub fn get_default_model_for_provider(&self, provider: Provider) -> String {
        // First check if there's a default in local presets
        if let Some(preset) = self.local_models.iter().find(|p| p.provider == provider && p.is_default) {
            return preset.model.clone();
        }

        // Fall back to hardcoded defaults
        match provider {
            Provider::Gemini => GEMINI_DEFAULT_MODEL.to_string(),
            Provider::OpenAI => OPENAI_DEFAULT_MODEL.to_string(),
            Provider::Anthropic => ANTHROPIC_DEFAULT_MODEL.to_string(),
            Provider::Copilot => crate::config::constants::models::copilot::DEFAULT_MODEL.to_string(),
            Provider::DeepSeek => "deepseek-reasoner".to_string(),
            Provider::Meta => crate::config::constants::models::meta::DEFAULT_MODEL.to_string(),
            Provider::ZAI => "glm-5.3".to_string(),
            Provider::Minimax => crate::config::constants::models::minimax::DEFAULT_MODEL.to_string(),
            Provider::Mistral => crate::config::constants::models::mistral::MISTRAL_LARGE_3.to_string(),
            Provider::OpenRouter => "xiaomi/mimo-v2.6-pro".to_string(),
            Provider::Ollama => "gpt-oss:20b".to_string(),
            Provider::OllamaCloud => crate::config::constants::models::ollama::DEFAULT_CLOUD_MODEL.to_string(),
            Provider::LmStudio => crate::config::constants::models::lmstudio::DEFAULT_MODEL.to_string(),
            Provider::LlamaCpp => crate::config::constants::models::llamacpp::DEFAULT_MODEL.to_string(),
            Provider::Moonshot => crate::config::constants::models::moonshot::DEFAULT_MODEL.to_string(),
            Provider::HuggingFace => "deepseek-ai/DeepSeek-V3-0324".to_string(),
            Provider::OpenCodeZen => crate::config::constants::models::opencode_zen::DEFAULT_MODEL.to_string(),
            Provider::OpenCodeGo => crate::config::constants::models::opencode_go::DEFAULT_MODEL.to_string(),
            Provider::MiMo => crate::config::constants::models::mimo::DEFAULT_MODEL.to_string(),
            Provider::Qwen => crate::config::constants::models::qwen::DEFAULT_MODEL.to_string(),
            Provider::StepFun => crate::config::constants::models::stepfun::DEFAULT_MODEL.to_string(),
            Provider::Evolink => crate::config::constants::models::evolink::DEFAULT_MODEL.to_string(),
            Provider::Poolside => crate::config::constants::models::poolside::DEFAULT_MODEL.to_string(),
            Provider::XAI => crate::config::constants::models::xai::DEFAULT_MODEL.to_string(),
            Provider::NVIDIA => crate::config::constants::models::nvidia::DEFAULT_MODEL.to_string(),
            Provider::MergeGateway => crate::config::constants::models::merge_gateway::DEFAULT_MODEL.to_string(),
            Provider::Vercel => crate::config::constants::models::vercel::DEFAULT_MODEL.to_string(),
        }
    }

    /// Get model offline (without network) for testing
    #[cfg(test)]
    pub fn get_model_offline(model: Option<&str>) -> String {
        model.unwrap_or(GEMINI_DEFAULT_MODEL).to_string()
    }

    /// Construct model family offline for testing
    #[cfg(test)]
    pub fn construct_model_family_offline(model: &str) -> ModelFamily {
        find_family_for_model(model)
    }

    /// Apply remote models (replace cached state)
    #[cfg(test)]
    async fn apply_remote_state_for_provider(
        &self,
        provider: Provider,
        models: Vec<ModelInfo>,
        etag: Option<String>,
    ) -> bool {
        let generation = self.refresh_generation.load(Ordering::Acquire);
        self.apply_remote_state_for_provider_at(provider, generation, models, etag)
            .await
    }

    async fn apply_remote_state_for_provider_at(
        &self,
        provider: Provider,
        generation: u64,
        models: Vec<ModelInfo>,
        etag: Option<String>,
    ) -> bool {
        let current_provider = self.current_provider.read().await;
        if *current_provider != provider || self.refresh_generation.load(Ordering::Acquire) != generation {
            debug!(
                requested_provider = %provider,
                current_provider = %*current_provider,
                "Discarding stale model metadata for a provider that is no longer active"
            );
            return false;
        }

        *self.remote_models.write().await = models;
        *self.etag.write().await = etag;
        true
    }

    /// Try to load from cache
    #[cfg(test)]
    async fn try_load_cache(&self) -> bool {
        let provider = *self.current_provider.read().await;
        self.try_load_cache_for(provider).await
    }

    #[cfg(test)]
    async fn try_load_cache_for(&self, provider: Provider) -> bool {
        let generation = self.refresh_generation.load(Ordering::Acquire);
        self.try_load_cache_for_at(provider, generation).await
    }

    async fn try_load_cache_for_at(&self, provider: Provider, generation: u64) -> bool {
        let Some(cache) = self.load_cache_for(provider).await else {
            return false;
        };
        if !cache.is_fresh(self.cache_ttl) {
            debug!("Cache is stale (age: {:?})", cache.age());
            return false;
        }
        self.apply_remote_state_for_provider_at(provider, generation, cache.models.into_iter().collect(), cache.etag)
            .await
    }

    async fn try_load_stale_cache_for_at(&self, provider: Provider, generation: u64) -> bool {
        let Some(cache) = self.load_cache_for(provider).await else {
            return false;
        };
        self.apply_remote_state_for_provider_at(provider, generation, cache.models.into_iter().collect(), cache.etag)
            .await
    }

    async fn load_cache_for(&self, provider: Provider) -> Option<ModelsCache> {
        let cache_path = self.cache_path_for(provider);
        load_cache_from_paths(&cache_path, &self.legacy_cache_paths(provider), provider).await
    }

    /// Persist cache to disk
    #[cfg(test)]
    async fn persist_cache(&self, models: &[ModelInfo], etag: Option<String>) {
        let provider = *self.current_provider.read().await;
        let generation = self.refresh_generation.load(Ordering::Acquire);
        self.persist_cache_for_at(provider, generation, models, etag).await;
    }

    #[cfg(test)]
    async fn persist_cache_for(&self, provider: Provider, models: &[ModelInfo], etag: Option<String>) {
        let cache = ModelsCache {
            fetched_at: Utc::now(),
            etag,
            provider: provider.to_string(),
            models: models.to_vec(),
        };
        let cache_path = self.cache_path_for(provider);
        cache::save_cache(&cache_path, &cache).await.expect("test cache write");
    }

    async fn persist_cache_for_at(
        &self,
        provider: Provider,
        generation: u64,
        models: &[ModelInfo],
        etag: Option<String>,
    ) {
        let _write_guard = self.cache_write_lock.lock().await;
        let current_provider = self.current_provider.read().await;
        if *current_provider != provider || self.refresh_generation.load(Ordering::Acquire) != generation {
            return;
        }
        let cache = ModelsCache {
            fetched_at: Utc::now(),
            etag,
            provider: provider.to_string(),
            models: models.to_vec(),
        };
        let cache_path = self.cache_path_for(provider);
        if let Err(err) = cache::save_cache(&cache_path, &cache).await {
            error!("Failed to write models cache: {err}");
        }
    }

    /// Build available models by merging remote and local presets
    fn build_available_models(&self, mut remote_models: Vec<ModelInfo>) -> Vec<ModelPreset> {
        // Sort by priority
        remote_models.sort_by_key(|a| a.priority);

        // Convert remote models to presets
        let remote_presets: Vec<ModelPreset> = remote_models.into_iter().map(Into::into).collect();
        let existing_presets = self.local_models.clone();
        let mut merged_presets = Self::merge_presets(remote_presets, existing_presets);
        merged_presets = self.filter_visible_models(merged_presets);

        // Ensure one default per provider
        self.ensure_defaults(&mut merged_presets);

        merged_presets
    }

    /// Filter to only visible models
    fn filter_visible_models(&self, models: Vec<ModelPreset>) -> Vec<ModelPreset> {
        models
            .into_iter()
            .filter(|model| model.show_in_picker && model.supported_in_api)
            .collect()
    }

    /// Merge remote and local presets, preferring remote when duplicates exist
    fn merge_presets(remote_presets: Vec<ModelPreset>, existing_presets: Vec<ModelPreset>) -> Vec<ModelPreset> {
        if remote_presets.is_empty() {
            return existing_presets;
        }

        let remote_slugs: HashSet<String> = remote_presets.iter().map(|preset| preset.model.clone()).collect();

        let mut merged_presets = remote_presets;
        for mut preset in existing_presets {
            if remote_slugs.contains(&preset.model) {
                continue;
            }
            preset.is_default = false;
            merged_presets.push(preset);
        }

        merged_presets
    }

    /// Ensure there's at least one default model
    fn ensure_defaults(&self, presets: &mut [ModelPreset]) {
        let has_default = presets.iter().any(|p| p.is_default);
        if !has_default && let Some(first) = presets.first_mut() {
            first.is_default = true;
        }
    }

    /// Get the provider-scoped cache file path.
    fn cache_path_for(&self, provider: Provider) -> PathBuf {
        self.vtcode_home.join(format!("models_cache_{provider}.json"))
    }

    fn legacy_cache_paths(&self, provider: Provider) -> Vec<PathBuf> {
        let provider_cache_file = format!("models_cache_{provider}.json");
        let mut candidates = if let Ok(paths) = VtCodePaths::resolve()
            && self.vtcode_home == paths.cache_dir()
        {
            vec![
                paths.legacy_dir().join(&provider_cache_file),
                paths.legacy_dir().join(LEGACY_MODEL_CACHE_FILE),
            ]
        } else {
            vec![
                self.vtcode_home.join(&provider_cache_file),
                self.vtcode_home.join(LEGACY_MODEL_CACHE_FILE),
            ]
        };
        candidates.dedup();
        candidates
    }

    /// Set the current provider
    pub async fn set_provider(&self, provider: Provider) {
        *self.current_provider.write().await = provider;
        self.refresh_generation.fetch_add(1, Ordering::AcqRel);
        self.remote_models.write().await.clear();
        *self.etag.write().await = None;
    }

    /// Get the current provider
    pub async fn get_provider(&self) -> Provider {
        *self.current_provider.read().await
    }

    /// Find a model preset by ID
    pub async fn find_model(&self, model_id: &str) -> Option<ModelPreset> {
        let models = self.list_models().await;
        models.into_iter().find(|m| m.model == model_id || m.id == model_id)
    }

    /// Check if a model exists
    pub async fn model_exists(&self, model_id: &str) -> bool {
        self.find_model(model_id).await.is_some()
    }

    /// Check if a model exists (sync, uses local presets only)
    ///
    /// This is a fast, non-blocking check that only looks at local presets.
    /// Use `model_exists` for the async version that includes remote models.
    pub fn model_exists_sync(&self, model_id: &str) -> bool {
        self.local_models.iter().any(|m| m.model == model_id || m.id == model_id)
    }

    /// Get all supported providers
    pub fn supported_providers() -> Vec<Provider> {
        Provider::all_providers()
    }

    /// Get version string for API requests
    pub fn client_version() -> String {
        format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        )
    }
}

async fn load_cache_from_paths(
    canonical_path: &Path,
    legacy_paths: &[PathBuf],
    provider: Provider,
) -> Option<ModelsCache> {
    let expected_provider = provider.to_string();
    let canonical_missing = match cache::load_cache(canonical_path).await {
        Ok(Some(cache)) => {
            if cache.provider == expected_provider {
                return Some(cache);
            }
            debug!(
                cached_provider = %cache.provider,
                requested_provider = %provider,
                "Ignoring model cache for a different provider"
            );
            false
        }
        Ok(None) => true,
        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
            debug!(path = %canonical_path.display(), "Ignoring malformed models cache");
            false
        }
        Err(err) => {
            error!(path = %canonical_path.display(), "Failed to load models cache: {err}");
            return None;
        }
    };

    for legacy_path in legacy_paths {
        if legacy_path == canonical_path {
            continue;
        }
        let cache = match cache::load_cache(legacy_path).await {
            Ok(Some(cache)) => cache,
            Ok(None) => continue,
            Err(err) => {
                debug!(path = %legacy_path.display(), "Ignoring unreadable legacy models cache: {err}");
                continue;
            }
        };
        if cache.provider != expected_provider {
            debug!(
                path = %legacy_path.display(),
                cached_provider = %cache.provider,
                requested_provider = %provider,
                "Ignoring legacy model cache for a different provider"
            );
            continue;
        }

        if canonical_missing {
            if let Err(err) = cache::save_cache_if_absent(canonical_path, &cache).await {
                debug!(path = %canonical_path.display(), "Failed to republish legacy models cache: {err}");
            }
        }
        return Some(cache);
    }

    None
}

/// Thread-safe reference-counted ModelsManager
pub type SharedModelsManager = Arc<ModelsManager>;

/// Create a new shared ModelsManager
pub fn new_shared_models_manager() -> SharedModelsManager {
    Arc::new(ModelsManager::new())
}

/// Create a new shared ModelsManager with specific provider
pub fn new_shared_models_manager_with_provider(provider: Provider) -> SharedModelsManager {
    Arc::new(ModelsManager::with_provider(provider))
}

#[cfg(test)]
mod tests;
