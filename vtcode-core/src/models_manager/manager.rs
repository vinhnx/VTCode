//! Models Manager - Coordinates model discovery, caching, and selection.
//!
//! This module provides the main `ModelsManager` struct that coordinates:
//! - Local model presets (built-in configurations)
//! - Remote model discovery (fetching from provider APIs)
//! - Disk caching with TTL
//! - Model family resolution

use chrono::Utc;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use super::cache::{self, ModelsCache};
use super::model_family::{ModelFamily, find_family_for_model};
use super::model_presets::{ModelInfo, ModelPreset, builtin_model_presets, presets_for_provider};
use crate::config::models::Provider;

/// Cache file name
const MODEL_CACHE_FILE: &str = "models_cache.json";

/// Default cache TTL (5 minutes)
const DEFAULT_MODEL_CACHE_TTL: Duration = Duration::from_secs(300);

/// Default model for Gemini provider
const GEMINI_DEFAULT_MODEL: &str = "gemini-3-flash-preview";

/// Default model for OpenAI provider
const OPENAI_DEFAULT_MODEL: &str = "gpt-5";

/// Default model for Anthropic provider
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-4.5";

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
    /// Whether remote model fetching is enabled
    remote_models_enabled: bool,
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
            remote_models_enabled: true,
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
            remote_models_enabled: true,
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
            remote_models_enabled: true,
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
            remote_models_enabled: true,
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
        dirs::home_dir()
            .map(|h| h.join(".vtcode"))
            .unwrap_or_else(|| PathBuf::from(".vtcode"))
    }

    /// Refresh available models, using cache if fresh
    pub async fn refresh_available_models(&self) -> anyhow::Result<()> {
        if !self.remote_models_enabled {
            debug!("Remote model fetching is disabled");
            return Ok(());
        }

        // Try to load from cache first
        if self.try_load_cache().await {
            debug!("Using cached models");
            return Ok(());
        }

        let provider = *self.current_provider.read().await;

        match provider {
            Provider::Ollama => {
                debug!("Fetching remote models for Ollama...");
                match self.fetch_ollama_models().await {
                    Ok(models) => {
                        info!("Fetched {} models from Ollama", models.len());
                        self.apply_remote_models(models.clone()).await;
                        self.persist_cache(&models, None).await;
                        Ok(())
                    }
                    Err(e) => {
                        error!("Failed to fetch Ollama models: {e}");
                        // Fall back to local presets if fetch fails
                        Ok(())
                    }
                }
            }
            _ => {
                // For other providers, we don't have remote discovery yet
                info!(
                    "Remote model discovery for {:?} not implemented, using local presets",
                    provider
                );
                Ok(())
            }
        }
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
                        display_name: format!("{} (Ollama)", name),
                        description: format!("Ollama model: {}", name),
                        provider: Provider::Ollama,
                        default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
                        supported_reasoning_levels: vec![],
                        context_window: Some(32_000), // Default for most Ollama models
                        supports_tool_use: true,
                        supports_streaming: true,
                        supports_reasoning: false,
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
        all_models
            .into_iter()
            .filter(|m| m.provider == provider)
            .collect()
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
        if let Some(preset) = self
            .local_models
            .iter()
            .find(|p| p.provider == provider && p.is_default)
        {
            return preset.model.clone();
        }

        // Fall back to hardcoded defaults
        match provider {
            Provider::Gemini => GEMINI_DEFAULT_MODEL.to_string(),
            Provider::OpenAI => OPENAI_DEFAULT_MODEL.to_string(),
            Provider::Anthropic => ANTHROPIC_DEFAULT_MODEL.to_string(),
            Provider::DeepSeek => "deepseek-reasoner".to_string(),
            Provider::XAI => "grok-4".to_string(),
            Provider::ZAI => "glm-5".to_string(),
            Provider::Minimax => "MiniMax-M2.5".to_string(),
            Provider::OpenRouter => "deepseek/deepseek-chat".to_string(),
            Provider::Ollama => "gpt-oss:20b".to_string(),
            Provider::Moonshot => "qwen3-coder-next".to_string(),
            Provider::HuggingFace => "deepseek-ai/DeepSeek-V3-0324".to_string(),
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
    async fn apply_remote_models(&self, models: Vec<ModelInfo>) {
        *self.remote_models.write().await = models;
    }

    /// Try to load from cache
    async fn try_load_cache(&self) -> bool {
        let cache_path = self.cache_path();
        let cache = match cache::load_cache(&cache_path).await {
            Ok(Some(cache)) => cache,
            Ok(None) => {
                debug!("No cache file found at {:?}", cache_path);
                return false;
            }
            Err(err) => {
                error!("Failed to load models cache: {err}");
                return false;
            }
        };

        if !cache.is_fresh(self.cache_ttl) {
            debug!("Cache is stale (age: {:?})", cache.age());
            return false;
        }

        let models: Vec<ModelInfo> = cache.models.into_iter().collect();

        *self.etag.write().await = cache.etag;
        self.apply_remote_models(models).await;
        true
    }

    /// Persist cache to disk
    #[allow(dead_code)] // Will be used when remote model fetching is implemented
    async fn persist_cache(&self, models: &[ModelInfo], etag: Option<String>) {
        let provider = *self.current_provider.read().await;
        let cache = ModelsCache {
            fetched_at: Utc::now(),
            etag,
            provider: provider.to_string(),
            models: models.to_vec(),
        };
        let cache_path = self.cache_path();
        if let Err(err) = cache::save_cache(&cache_path, &cache).await {
            error!("Failed to write models cache: {err}");
        }
    }

    /// Build available models by merging remote and local presets
    fn build_available_models(&self, mut remote_models: Vec<ModelInfo>) -> Vec<ModelPreset> {
        // Sort by priority
        remote_models.sort_by(|a, b| a.priority.cmp(&b.priority));

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
    fn merge_presets(
        remote_presets: Vec<ModelPreset>,
        existing_presets: Vec<ModelPreset>,
    ) -> Vec<ModelPreset> {
        if remote_presets.is_empty() {
            return existing_presets;
        }

        let remote_slugs: HashSet<String> = remote_presets
            .iter()
            .map(|preset| preset.model.clone())
            .collect();

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

    /// Get the cache file path
    fn cache_path(&self) -> PathBuf {
        self.vtcode_home.join(MODEL_CACHE_FILE)
    }

    /// Set the current provider
    pub async fn set_provider(&self, provider: Provider) {
        *self.current_provider.write().await = provider;
    }

    /// Get the current provider
    pub async fn get_provider(&self) -> Provider {
        *self.current_provider.read().await
    }

    /// Find a model preset by ID
    pub async fn find_model(&self, model_id: &str) -> Option<ModelPreset> {
        let models = self.list_models().await;
        models
            .into_iter()
            .find(|m| m.model == model_id || m.id == model_id)
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
        self.local_models
            .iter()
            .any(|m| m.model == model_id || m.id == model_id)
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
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_new_manager() {
        let manager = ModelsManager::new();
        assert!(!manager.local_models.is_empty());
    }

    #[tokio::test]
    async fn test_list_models() {
        let manager = ModelsManager::new();
        let models = manager.list_models().await;
        assert!(!models.is_empty());
    }

    #[tokio::test]
    async fn test_list_models_for_provider() {
        let manager = ModelsManager::new();
        let gemini_models = manager.list_models_for_provider(Provider::Gemini).await;
        assert!(!gemini_models.is_empty());
        assert!(gemini_models.iter().all(|m| m.provider == Provider::Gemini));
    }

    #[tokio::test]
    async fn test_get_model_with_default() {
        let manager = ModelsManager::with_provider(Provider::Gemini);
        let model = manager.get_model(None).await;
        assert!(!model.is_empty());
    }

    #[tokio::test]
    async fn test_get_model_with_explicit() {
        let manager = ModelsManager::new();
        let model = manager.get_model(Some("custom-model")).await;
        assert_eq!(model, "custom-model");
    }

    #[tokio::test]
    async fn test_construct_model_family() {
        let manager = ModelsManager::new();
        let family = manager
            .construct_model_family("gemini-3-flash-preview")
            .await;
        assert_eq!(family.family, "gemini-3");
        assert_eq!(family.provider, Provider::Gemini);
    }

    #[tokio::test]
    async fn test_find_model() {
        let manager = ModelsManager::new();
        let model = manager.find_model("gemini-3-flash-preview").await;
        assert!(model.is_some());
    }

    #[tokio::test]
    async fn test_model_exists() {
        let manager = ModelsManager::new();
        assert!(manager.model_exists("gemini-3-flash-preview").await);
        assert!(!manager.model_exists("nonexistent-model").await);
    }

    #[tokio::test]
    async fn test_set_provider() {
        let manager = ModelsManager::new();
        manager.set_provider(Provider::Anthropic).await;
        assert_eq!(manager.get_provider().await, Provider::Anthropic);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let dir = tempdir().expect("create temp dir");
        let manager = ModelsManager::with_home(dir.path().to_path_buf());

        // Initially no cache
        let cached = manager.try_load_cache().await;
        assert!(!cached);

        // Persist some models
        let models = vec![ModelInfo {
            slug: "test-model".to_string(),
            display_name: "Test Model".to_string(),
            description: "A test".to_string(),
            provider: Provider::Gemini,
            default_reasoning_level: crate::config::types::ReasoningEffortLevel::Medium,
            supported_reasoning_levels: vec![],
            context_window: Some(128_000),
            supports_tool_use: true,
            supports_streaming: true,
            supports_reasoning: false,
            priority: 0,
            visibility: "list".to_string(),
            supported_in_api: true,
            upgrade: None,
        }];
        manager.persist_cache(&models, None).await;

        // Now cache should load
        let cached = manager.try_load_cache().await;
        assert!(cached);
    }

    #[test]
    fn test_client_version() {
        let version = ModelsManager::client_version();
        assert!(!version.is_empty());
        assert!(version.contains('.'));
    }

    #[test]
    fn test_supported_providers() {
        let providers = ModelsManager::supported_providers();
        assert!(!providers.is_empty());
        assert!(providers.contains(&Provider::Gemini));
        assert!(providers.contains(&Provider::OpenAI));
    }
}
