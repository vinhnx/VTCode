//! Incremental system prompt building to avoid redundant string cloning and processing
//!
//! This module provides a cached system prompt builder that only rebuilds the prompt
//! when the underlying configuration or context has changed.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Cached system prompt that avoids redundant rebuilding
#[derive(Clone)]
pub struct IncrementalSystemPrompt {
    /// The cached system prompt content
    cached_prompt: Arc<RwLock<CachedPrompt>>,
}

#[derive(Clone)]
struct CachedPrompt {
    /// The actual prompt content
    content: String,
    /// Hash of the configuration that generated this prompt
    config_hash: u64,
    /// Hash of the context that generated this prompt
    context_hash: u64,
    /// Number of retry attempts this prompt was built for
    retry_attempts: usize,
}

impl IncrementalSystemPrompt {
    pub fn new() -> Self {
        Self {
            cached_prompt: Arc::new(RwLock::new(CachedPrompt {
                content: String::new(),
                config_hash: 0,
                context_hash: 0,
                retry_attempts: usize::MAX, // Force rebuild on first use
            })),
        }
    }

    /// Get the current system prompt, rebuilding only if necessary
    pub async fn get_system_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
    ) -> String {
        let read_guard = self.cached_prompt.read().await;

        // Check if we can use the cached version
        if read_guard.config_hash == config_hash
            && read_guard.context_hash == context_hash
            && read_guard.retry_attempts == retry_attempts
            && !read_guard.content.is_empty()
        {
            return read_guard.content.clone();
        }

        // Drop read lock before acquiring write lock
        drop(read_guard);

        // Rebuild the prompt
        self.rebuild_prompt(
            base_system_prompt,
            config_hash,
            context_hash,
            retry_attempts,
        )
        .await
    }

    /// Force rebuild of the system prompt
    pub async fn rebuild_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
    ) -> String {
        let mut write_guard = self.cached_prompt.write().await;

        // Double-check after acquiring write lock
        if write_guard.config_hash == config_hash
            && write_guard.context_hash == context_hash
            && write_guard.retry_attempts == retry_attempts
            && !write_guard.content.is_empty()
        {
            return write_guard.content.clone();
        }

        // Build the new prompt
        let new_content = self.build_prompt_content(base_system_prompt, retry_attempts);

        // Update cache
        write_guard.content = new_content.clone();
        write_guard.config_hash = config_hash;
        write_guard.context_hash = context_hash;
        write_guard.retry_attempts = retry_attempts;

        new_content
    }

    /// Actually build the prompt content (this is where the logic goes)
    fn build_prompt_content(&self, base_system_prompt: &str, retry_attempts: usize) -> String {
        // For now, just return the base prompt, but this is where we could add
        // retry-specific modifications, context-aware adjustments, etc.
        if retry_attempts > 0 {
            format!(
                "{}\n\nNote: This is attempt #{} at completing the task.",
                base_system_prompt, retry_attempts
            )
        } else {
            base_system_prompt.to_string()
        }
    }

    /// Clear the cached prompt (useful when configuration changes)
    pub async fn clear_cache(&self) {
        let mut write_guard = self.cached_prompt.write().await;
        write_guard.content.clear();
        write_guard.config_hash = 0;
        write_guard.context_hash = 0;
        write_guard.retry_attempts = usize::MAX;
    }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> (bool, usize) {
        let read_guard = self.cached_prompt.read().await;
        (!read_guard.content.is_empty(), read_guard.content.len())
    }
}

impl Default for IncrementalSystemPrompt {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration that affects system prompt building
#[derive(Debug, Clone, Hash)]
pub struct SystemPromptConfig {
    pub base_prompt: String,
    pub enable_retry_context: bool,
    pub enable_token_tracking: bool,
    pub max_retry_attempts: usize,
}

impl SystemPromptConfig {
    pub fn hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.base_prompt.hash(&mut hasher);
        self.enable_retry_context.hash(&mut hasher);
        self.enable_token_tracking.hash(&mut hasher);
        self.max_retry_attempts.hash(&mut hasher);
        hasher.finish()
    }
}

/// Context that affects system prompt building
#[derive(Debug, Clone)]
pub struct SystemPromptContext {
    pub conversation_length: usize,
    pub tool_usage_count: usize,
    pub error_count: usize,
    pub token_usage_ratio: f64,
}

impl SystemPromptContext {
    pub fn hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.conversation_length.hash(&mut hasher);
        self.tool_usage_count.hash(&mut hasher);
        self.error_count.hash(&mut hasher);
        ((self.token_usage_ratio * 1000.0) as usize).hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_incremental_prompt_caching() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "Test system prompt";

        // First call - should build from scratch
        let prompt1 = prompt_builder.get_system_prompt(base_prompt, 1, 1, 0).await;
        assert_eq!(prompt1, base_prompt);

        // Second call with same parameters - should use cache
        let prompt2 = prompt_builder.get_system_prompt(base_prompt, 1, 1, 0).await;
        assert_eq!(prompt2, base_prompt);

        // Verify cache stats
        let (is_cached, size) = prompt_builder.cache_stats().await;
        assert!(is_cached);
        assert_eq!(size, base_prompt.len());
    }

    #[tokio::test]
    async fn test_incremental_prompt_rebuild() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "Test system prompt";

        // Build initial prompt
        let _ = prompt_builder.get_system_prompt(base_prompt, 1, 1, 0).await;

        // Rebuild with different retry attempts
        let prompt = prompt_builder.rebuild_prompt(base_prompt, 1, 1, 1).await;

        assert!(prompt.contains("attempt #1"));
    }

    #[tokio::test]
    async fn test_prompt_config_hash() {
        let config1 = SystemPromptConfig {
            base_prompt: "Test".to_string(),
            enable_retry_context: true,
            enable_token_tracking: false,
            max_retry_attempts: 3,
        };

        let config2 = SystemPromptConfig {
            base_prompt: "Test".to_string(),
            enable_retry_context: true,
            enable_token_tracking: false,
            max_retry_attempts: 3,
        };

        assert_eq!(config1.hash(), config2.hash());
    }
}
