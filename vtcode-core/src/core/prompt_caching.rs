use crate::config::constants::prompt_cache;
use crate::config::core::PromptCachingConfig;
use crate::llm::provider::{Message, MessageContent, MessageRole};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

/// Cached prompt entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPrompt {
    pub prompt_hash: String,
    pub original_prompt: String,
    pub optimized_prompt: String,
    pub model_used: String,
    pub tokens_saved: Option<u32>,
    pub quality_score: Option<f64>,
    pub created_at: u64,
    pub last_used: u64,
    pub usage_count: u32,
}

/// Prompt caching configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCacheConfig {
    pub enabled: bool,
    pub cache_dir: PathBuf,
    pub max_cache_size: usize,
    pub max_age_days: u64,
    pub enable_auto_cleanup: bool,
    pub min_quality_threshold: f64,
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        Self {
            enabled: prompt_cache::DEFAULT_ENABLED,
            cache_dir: default_cache_dir(),
            max_cache_size: prompt_cache::DEFAULT_MAX_ENTRIES,
            max_age_days: prompt_cache::DEFAULT_MAX_AGE_DAYS,
            enable_auto_cleanup: prompt_cache::DEFAULT_AUTO_CLEANUP,
            min_quality_threshold: prompt_cache::DEFAULT_MIN_QUALITY_THRESHOLD,
        }
    }
}

impl PromptCacheConfig {
    /// Build runtime configuration from high-level TOML settings
    pub fn from_settings(settings: &PromptCachingConfig, workspace_root: Option<&Path>) -> Self {
        Self {
            enabled: settings.enabled,
            cache_dir: settings.resolve_cache_dir(workspace_root),
            max_cache_size: settings.max_entries,
            max_age_days: settings.max_age_days,
            enable_auto_cleanup: settings.enable_auto_cleanup,
            min_quality_threshold: settings.min_quality_threshold,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

fn default_cache_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(prompt_cache::DEFAULT_CACHE_DIR);
    }
    PathBuf::from(prompt_cache::DEFAULT_CACHE_DIR)
}

/// Prompt caching system
pub struct PromptCache {
    config: PromptCacheConfig,
    cache: HashMap<String, CachedPrompt>,
    dirty: bool,
}

impl PromptCache {
    pub async fn new() -> Self {
        Self::with_config(PromptCacheConfig::default()).await
    }

    pub async fn with_config(config: PromptCacheConfig) -> Self {
        let mut cache = Self {
            config,
            cache: HashMap::new(),
            dirty: false,
        };

        // Load existing cache
        if cache.config.enabled {
            let _ = cache.load_cache().await;

            // Auto cleanup if enabled
            if cache.config.enable_auto_cleanup {
                let _ = cache.cleanup_expired();
            }
        }

        cache
    }

    /// Get cached optimized prompt
    pub fn get(&mut self, prompt_hash: &str) -> Option<&CachedPrompt> {
        if !self.config.enabled {
            return None;
        }
        if let Some(entry) = self.cache.get_mut(prompt_hash) {
            entry.last_used = Self::current_timestamp();
            entry.usage_count += 1;
            self.dirty = true;
            Some(entry)
        } else {
            None
        }
    }

    /// Store optimized prompt in cache
    pub fn put(&mut self, entry: CachedPrompt) -> Result<(), PromptCacheError> {
        if !self.config.enabled {
            return Ok(());
        }
        // Check quality threshold
        if let Some(quality) = entry.quality_score {
            if quality < self.config.min_quality_threshold {
                return Ok(()); // Don't cache low-quality entries
            }
        }

        // Check cache size limit
        if self.cache.len() >= self.config.max_cache_size {
            self.evict_oldest()?;
        }

        self.cache.insert(entry.prompt_hash.clone(), entry);
        self.dirty = true;

        Ok(())
    }

    /// Check if prompt is cached
    pub fn contains(&self, prompt_hash: &str) -> bool {
        self.config.enabled && self.cache.contains_key(prompt_hash)
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        if !self.config.enabled {
            return CacheStats::default();
        }
        let total_entries = self.cache.len();
        let total_usage = self.cache.values().map(|e| e.usage_count).sum::<u32>();
        let total_tokens_saved = self
            .cache
            .values()
            .filter_map(|e| e.tokens_saved)
            .sum::<u32>();
        let avg_quality = if !self.cache.is_empty() {
            self.cache
                .values()
                .filter_map(|e| e.quality_score)
                .sum::<f64>()
                / self.cache.len() as f64
        } else {
            0.0
        };

        CacheStats {
            total_entries,
            total_usage,
            total_tokens_saved,
            avg_quality,
        }
    }

    /// Clear all cache entries
    pub async fn clear(&mut self) -> Result<(), PromptCacheError> {
        if !self.config.enabled {
            return Ok(());
        }
        self.cache.clear();
        self.dirty = true;
        self.save_cache().await
    }

    /// Generate hash for prompt
    pub fn hash_prompt(prompt: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Save cache to disk
    pub async fn save_cache(&self) -> Result<(), PromptCacheError> {
        if !self.config.enabled || !self.dirty {
            return Ok(());
        }

        // Ensure cache directory exists
        fs::create_dir_all(&self.config.cache_dir)
            .await
            .map_err(|e| PromptCacheError::Io(e))?;

        let cache_path = self.config.cache_dir.join("prompt_cache.json");
        let data = serde_json::to_string_pretty(&self.cache)
            .map_err(|e| PromptCacheError::Serialization(e))?;

        fs::write(cache_path, data)
            .await
            .map_err(|e| PromptCacheError::Io(e))?;

        Ok(())
    }

    /// Load cache from disk
    async fn load_cache(&mut self) -> Result<(), PromptCacheError> {
        if !self.config.enabled {
            return Ok(());
        }
        let cache_path = self.config.cache_dir.join("prompt_cache.json");

        if !fs::try_exists(&cache_path).await.unwrap_or(false) {
            return Ok(());
        }

        let data = fs::read_to_string(cache_path)
            .await
            .map_err(|e| PromptCacheError::Io(e))?;

        self.cache = serde_json::from_str(&data).map_err(|e| PromptCacheError::Serialization(e))?;

        Ok(())
    }

    /// Clean up expired cache entries
    fn cleanup_expired(&mut self) -> Result<(), PromptCacheError> {
        if !self.config.enabled {
            return Ok(());
        }
        let now = Self::current_timestamp();
        let max_age_seconds = self.config.max_age_days * 24 * 60 * 60;

        self.cache
            .retain(|_, entry| now - entry.created_at < max_age_seconds);

        self.dirty = true;
        Ok(())
    }

    /// Evict oldest cache entries when cache is full
    fn evict_oldest(&mut self) -> Result<(), PromptCacheError> {
        if !self.config.enabled {
            return Ok(());
        }
        if self.cache.is_empty() {
            return Ok(());
        }

        // Find the oldest entry
        let oldest_key = self
            .cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
            .unwrap();

        self.cache.remove(&oldest_key);
        self.dirty = true;

        Ok(())
    }

    /// Get current timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

// Note: Drop trait cannot be async, so we remove automatic save on drop.
// Users must explicitly call save_cache() or use a wrapper that handles this.

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_usage: u32,
    pub total_tokens_saved: u32,
    pub avg_quality: f64,
}

impl Default for CacheStats {
    fn default() -> Self {
        Self {
            total_entries: 0,
            total_usage: 0,
            total_tokens_saved: 0,
            avg_quality: 0.0,
        }
    }
}

/// Prompt cache errors
#[derive(Debug, thiserror::Error)]
pub enum PromptCacheError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Cache full")]
    CacheFull,
}

/// Prompt optimizer that uses caching
pub struct PromptOptimizer {
    cache: PromptCache,
    llm_provider: Box<dyn crate::llm::provider::LLMProvider>,
}

impl PromptOptimizer {
    pub async fn new(llm_provider: Box<dyn crate::llm::provider::LLMProvider>) -> Self {
        Self {
            cache: PromptCache::new().await,
            llm_provider,
        }
    }

    pub fn with_cache(mut self, cache: PromptCache) -> Self {
        self.cache = cache;
        self
    }

    /// Explicitly save the cache to disk
    pub async fn save_cache(&self) -> Result<(), PromptCacheError> {
        self.cache.save_cache().await
    }

    /// Optimize a prompt using caching
    pub async fn optimize_prompt(
        &mut self,
        original_prompt: &str,
        target_model: &str,
        context: Option<&str>,
    ) -> Result<String, PromptOptimizationError> {
        let prompt_hash = PromptCache::hash_prompt(original_prompt);

        // Check cache first
        if let Some(cached) = self.cache.get(&prompt_hash) {
            return Ok(cached.optimized_prompt.clone());
        }

        // Generate optimized prompt
        let optimized = self
            .generate_optimized_prompt(original_prompt, target_model, context)
            .await?;

        // Calculate tokens saved (rough estimate)
        let original_tokens = Self::estimate_tokens(original_prompt);
        let optimized_tokens = Self::estimate_tokens(&optimized);
        let tokens_saved = original_tokens.saturating_sub(optimized_tokens);

        // Create cache entry
        let entry = CachedPrompt {
            prompt_hash: prompt_hash.clone(),
            original_prompt: original_prompt.to_string(),
            optimized_prompt: optimized.clone(),
            model_used: target_model.to_string(),
            tokens_saved: Some(tokens_saved),
            quality_score: Some(0.8), // Placeholder quality score
            created_at: PromptCache::current_timestamp(),
            last_used: PromptCache::current_timestamp(),
            usage_count: 1,
        };

        // Store in cache
        self.cache.put(entry)?;

        Ok(optimized)
    }

    /// Generate optimized prompt using LLM
    async fn generate_optimized_prompt(
        &self,
        original_prompt: &str,
        target_model: &str,
        context: Option<&str>,
    ) -> Result<String, PromptOptimizationError> {
        let system_prompt = format!(
            "You are an expert prompt engineer. Your task is to optimize prompts for {} \
             to make them more effective, clearer, and more likely to produce high-quality responses. \
             Focus on improving clarity, specificity, structure, and effectiveness while preserving \
             the original intent and requirements.",
            target_model
        );

        let mut user_prompt = format!(
            "Please optimize the following prompt for {}:\n\nORIGINAL PROMPT:\n{}\n\n",
            target_model, original_prompt
        );

        if let Some(ctx) = context {
            user_prompt.push_str(&format!("CONTEXT:\n{}\n\n", ctx));
        }

        user_prompt.push_str(
            "OPTIMIZATION REQUIREMENTS:\n\
             1. Make the prompt clearer and more specific\n\
             2. Improve structure and formatting\n\
             3. Add relevant context or examples if helpful\n\
             4. Ensure the prompt is appropriate for the target model\n\
             5. Maintain the original intent and requirements\n\
             6. Keep the optimized prompt concise but comprehensive\n\n\
             Provide only the optimized prompt without any explanation or additional text.",
        );

        let request = crate::llm::provider::LLMRequest {
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text(system_prompt),
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: None,
                    origin_tool: None,
                },
                Message {
                    role: MessageRole::User,
                    content: MessageContent::Text(user_prompt),
                    reasoning: None,
                    reasoning_details: None,
                    tool_calls: None,
                    tool_call_id: None,
                    origin_tool: None,
                },
            ],
            system_prompt: None,
            tools: None,
            model: target_model.to_string(),
            max_tokens: Some(2000),
            temperature: Some(0.3),
            stream: false,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
        };

        let response = self
            .llm_provider
            .generate(request)
            .await
            .map_err(|e| PromptOptimizationError::LLMError(e.to_string()))?;

        Ok(response
            .content
            .unwrap_or_else(|| original_prompt.to_string()))
    }

    /// Estimate token count (rough approximation)
    fn estimate_tokens(text: &str) -> u32 {
        // Rough approximation: 1 token ≈ 4 characters for English text
        (text.len() / 4) as u32
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// Clear cache
    pub async fn clear_cache(&mut self) -> Result<(), PromptCacheError> {
        self.cache.clear().await
    }
}

/// Prompt optimization errors
#[derive(Debug, thiserror::Error)]
pub enum PromptOptimizationError {
    #[error("LLM error: {0}")]
    LLMError(String),

    #[error("Cache error: {0}")]
    CacheError(#[from] PromptCacheError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_hash() {
        let prompt = "Test prompt";
        let hash1 = PromptCache::hash_prompt(prompt);
        let hash2 = PromptCache::hash_prompt(prompt);
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let mut cache = PromptCache::new().await;

        let entry = CachedPrompt {
            prompt_hash: "test_hash".to_string(),
            original_prompt: "original".to_string(),
            optimized_prompt: "optimized".to_string(),
            model_used: crate::config::constants::models::GEMINI_2_5_FLASH.to_string(),
            tokens_saved: Some(100),
            quality_score: Some(0.9),
            created_at: 1000,
            last_used: 1000,
            usage_count: 0,
        };

        cache.put(entry).unwrap();
        assert!(cache.contains("test_hash"));

        let retrieved = cache.get("test_hash");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().usage_count, 1);
    }

    #[tokio::test]
    async fn disabled_cache_config_is_no_op() {
        let settings = PromptCachingConfig {
            enabled: false,
            cache_dir: "relative/cache".to_string(),
            ..PromptCachingConfig::default()
        };
        let cfg = PromptCacheConfig::from_settings(&settings, None);
        assert!(!cfg.is_enabled());

        let mut cache = PromptCache::with_config(cfg).await;
        assert!(!cache.contains("missing"));
        assert_eq!(cache.stats().total_entries, 0);

        let entry = CachedPrompt {
            prompt_hash: "noop".to_string(),
            original_prompt: "original".to_string(),
            optimized_prompt: "optimized".to_string(),
            model_used: crate::config::constants::models::GEMINI_2_5_FLASH.to_string(),
            tokens_saved: Some(10),
            quality_score: Some(0.9),
            created_at: 1,
            last_used: 1,
            usage_count: 0,
        };

        cache.put(entry).unwrap();
        assert!(!cache.contains("noop"));
        assert_eq!(cache.stats().total_entries, 0);
    }
}
