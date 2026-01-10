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

    pub async fn get_system_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
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
            context,
            agent_config,
        )
        .await
    }

    /// Rebuild the prompt
    pub async fn rebuild_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
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
        let new_content = self
            .build_prompt_content(base_system_prompt, retry_attempts, context, agent_config)
            .await;

        // Update cache
        write_guard.content = new_content.clone();
        write_guard.config_hash = config_hash;
        write_guard.context_hash = context_hash;
        write_guard.retry_attempts = retry_attempts;

        new_content
    }

    /// Actually build the prompt content (this is where the logic goes)
    async fn build_prompt_content(
        &self,
        base_system_prompt: &str,
        retry_attempts: usize,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        use std::fmt::Write;
        use vtcode_core::project_doc::get_user_instructions;
        use vtcode_core::skills::types::SkillMetadata;

        let mut prompt = String::with_capacity(base_system_prompt.len() + 1024);
        prompt.push_str(base_system_prompt);

        // ...

        // Concise retry/error context
        if retry_attempts > 0 {
            let _ = writeln!(
                prompt,
                "\n# Retry #{}: Try different approaches, not same steps.",
                retry_attempts
            );
        }
        if context.error_count > 0 {
            let _ = writeln!(
                prompt,
                "\n# {} errors: Check file paths, permissions, tool args.",
                context.error_count
            );
        }

        let has_context = context.conversation_length > 0
            || context.tool_usage_count > 0
            || context.error_count > 0
            || context.token_usage_ratio > 0.0;

        if has_context {
            let _ = writeln!(prompt, "\n[Context]");
            let _ = writeln!(prompt, "- turns: {}", context.conversation_length);
            let _ = writeln!(prompt, "- tool_calls: {}", context.tool_usage_count);
            let _ = writeln!(prompt, "- errors: {}", context.error_count);
            let _ = writeln!(
                prompt,
                "- token_usage: {:.2}%",
                context.token_usage_ratio * 100.0
            );
            let _ = writeln!(prompt, "- full_auto: {}", context.full_auto);

            if context.full_auto {
                let _ = writeln!(
                    prompt,
                    "\n# FULL-AUTO: Complete task autonomously until done or blocked."
                );
            }

            if context.plan_mode {
                let _ = writeln!(prompt, "\n# PLAN MODE (READ-ONLY)");
                let _ = writeln!(
                    prompt,
                    "You are in Plan Mode. In this mode:"
                );
                let _ = writeln!(
                    prompt,
                    "- You may freely read files, list files, search, and use code intelligence tools."
                );
                let _ = writeln!(
                    prompt,
                    "- You MUST NOT edit files, apply patches, run shell commands, or execute tests."
                );
                let _ = writeln!(
                    prompt,
                    "- Instead, produce a clear, step-by-step implementation plan."
                );
                let _ = writeln!(
                    prompt,
                    "- Plans should be in structured Markdown (numbered steps) and mention relevant files, functions, and tests."
                );
                let _ = writeln!(
                    prompt,
                    "- Ask clarifying questions if requirements are ambiguous."
                );
                let _ = writeln!(
                    prompt,
                    "- The user can exit Plan Mode with /plan off to enable mutating tools."
                );
            }
        }

        // Unified Instructions (Project Docs, User Inst, Skills)
        if let Some(cfg) = agent_config {
            let skill_metadata: Vec<SkillMetadata> = context
                .discovered_skills
                .iter()
                .map(|s| SkillMetadata {
                    name: s.manifest.name.clone(),
                    description: s.manifest.description.clone(),
                    short_description: s.manifest.when_to_use.clone(),
                    path: s.path.clone(),
                    scope: s.scope,
                    manifest: Some(s.manifest.clone()),
                })
                .collect();
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            if let Some(unified) = get_user_instructions(cfg, &cwd, Some(&skill_metadata[..])).await
            {
                let _ = writeln!(prompt, "\n# INSTRUCTIONS\n{}", unified);
            }
        } else {
            // Fallback if config is missing (basic skills rendering)
            if !context.discovered_skills.is_empty() {
                let _ = writeln!(
                    prompt,
                    "\n# SKILLS\nUse `list_skills` to see available capabilities."
                );
            }
        }

        prompt
    }

    /// Clear the cached prompt (useful when configuration changes)
    #[allow(dead_code)]
    pub async fn clear_cache(&self) {
        let mut write_guard = self.cached_prompt.write().await;
        write_guard.content.clear();
        write_guard.config_hash = 0;
        write_guard.context_hash = 0;
        write_guard.retry_attempts = usize::MAX;
    }

    /// Get cache statistics
    #[allow(dead_code)]
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
    pub full_auto: bool,
    /// Plan mode: read-only mode for exploration and planning
    pub plan_mode: bool,
    /// Discovered skills for immediate awareness
    pub discovered_skills: Vec<vtcode_core::skills::types::Skill>,
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
        self.full_auto.hash(&mut hasher);
        self.plan_mode.hash(&mut hasher);
        // We use skill names and versions for hashing
        for skill in &self.discovered_skills {
            skill.name().hash(&mut hasher);
            skill.manifest.version.hash(&mut hasher);
        }
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
        let context = SystemPromptContext {
            conversation_length: 2,
            tool_usage_count: 1,
            error_count: 0,
            token_usage_ratio: 0.0,
            full_auto: false,
            plan_mode: false,
            discovered_skills: Vec::new(),
        };

        // First call - should build from scratch
        let prompt1 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;
        assert_eq!(prompt1, base_prompt);

        // Second call with same parameters - should use cache
        let prompt2 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;
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
        let context = SystemPromptContext {
            conversation_length: 1,
            tool_usage_count: 0,
            error_count: 0,
            token_usage_ratio: 0.0,
            full_auto: false,
            plan_mode: false,
            discovered_skills: Vec::new(),
        };
        // Build initial prompt
        let _ = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        // Rebuild with different retry attempts
        let prompt = prompt_builder
            .rebuild_prompt(base_prompt, 1, 1, 1, &context, None)
            .await;

        assert!(prompt.contains("Retry #1"));
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
