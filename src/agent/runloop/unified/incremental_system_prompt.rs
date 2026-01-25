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

        // Inject context budget for models with context awareness (Claude 4.5+)
        if context.supports_context_awareness
            && let Some(context_size) = context.context_window_size
        {
            let _ = writeln!(
                prompt,
                "\n<budget:token_budget>{}</budget:token_budget>",
                context_size
            );
        }

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

            // Add system warning for context awareness models with token tracking
            if context.supports_context_awareness
                && let (Some(context_size), Some(used)) =
                    (context.context_window_size, context.current_token_usage)
            {
                let remaining = context_size.saturating_sub(used);
                let guidance = context.token_budget_guidance;

                // Context Anxiety Counter-prompting
                // Reassure the model to prevent rushing when approaching limits
                let anxiety_reassurance =
                    if remaining < context_size / 2 && remaining > context_size / 4 {
                        " You have sufficient context—focus on quality over speed."
                    } else {
                        ""
                    };

                if guidance.is_empty() {
                    let _ = writeln!(
                        prompt,
                        "<system_warning>Token usage: {}/{}; {} remaining{}</system_warning>",
                        used, context_size, remaining, anxiety_reassurance
                    );
                } else {
                    let _ = writeln!(
                        prompt,
                        "<system_warning>Token usage: {}/{}; {} remaining. {}{}</system_warning>",
                        used, context_size, remaining, guidance, anxiety_reassurance
                    );
                }
            }

            if context.full_auto {
                let _ = writeln!(
                    prompt,
                    "\n# FULL-AUTO: Complete task autonomously until done or blocked."
                );
            }

            // Inject active agent's system prompt (replaces hardcoded plan mode injection)
            // This supports the planner/coder subagent architecture
            if let Some(ref agent_prompt) = context.active_agent_prompt {
                // Use the subagent's system prompt directly
                let _ = writeln!(prompt, "\n{}", agent_prompt);
            } else if context.plan_mode {
                // Legacy fallback: if no active agent prompt but plan_mode is set,
                // inject minimal plan mode notice (backward compatibility)
                let _ = writeln!(prompt, "\n# PLAN MODE (READ-ONLY)");
                let _ = writeln!(
                    prompt,
                    "Plan Mode is active. Mutating tools are blocked except for `.vtcode/plans/` directory."
                );
                let _ = writeln!(
                    prompt,
                    "Call `exit_plan_mode` when ready to transition to implementation."
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
/// Uses pre-computed hash of base_prompt to avoid cloning the full string
#[derive(Debug, Clone, Hash)]
pub struct SystemPromptConfig {
    /// Pre-computed hash of the base prompt (avoids storing full string)
    pub base_prompt_hash: u64,
    pub enable_retry_context: bool,
    pub enable_token_tracking: bool,
    pub max_retry_attempts: usize,
}

impl SystemPromptConfig {
    /// Create a new config with a pre-computed hash of the base prompt
    pub fn new(
        base_prompt: &str,
        enable_retry_context: bool,
        enable_token_tracking: bool,
        max_retry_attempts: usize,
    ) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        base_prompt.hash(&mut hasher);

        Self {
            base_prompt_hash: hasher.finish(),
            enable_retry_context,
            enable_token_tracking,
            max_retry_attempts,
        }
    }

    pub fn hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.base_prompt_hash.hash(&mut hasher);
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
    /// Plan mode: read-only mode for exploration and planning (legacy, for backward compatibility)
    pub plan_mode: bool,
    /// Active agent profile name (e.g., "planner", "coder")
    /// This determines which subagent's system prompt is used
    pub active_agent_name: String,
    /// Active agent's system prompt (from SubagentConfig)
    /// If set, this will be appended to the base system prompt
    pub active_agent_prompt: Option<String>,
    /// Discovered skills for immediate awareness
    pub discovered_skills: Vec<vtcode_core::skills::types::Skill>,
    /// Total context window size for the current model (e.g., 200000, 1000000)
    pub context_window_size: Option<usize>,
    /// Current tokens used in the conversation
    pub current_token_usage: Option<usize>,
    /// Whether the model supports context awareness (Claude 4.5+)
    pub supports_context_awareness: bool,
    /// Actionable guidance based on token budget status
    pub token_budget_guidance: &'static str,
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
        self.active_agent_name.hash(&mut hasher);
        self.active_agent_prompt.hash(&mut hasher);
        self.context_window_size.hash(&mut hasher);
        self.current_token_usage.hash(&mut hasher);
        self.supports_context_awareness.hash(&mut hasher);
        self.token_budget_guidance.hash(&mut hasher);
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
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: None,
            current_token_usage: None,
            supports_context_awareness: false,
            token_budget_guidance: "",
        };

        // First call - should build from scratch (includes context section)
        let prompt1 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;
        assert!(prompt1.contains("Test system prompt"));
        assert!(prompt1.contains("[Context]"));

        // Second call with same parameters - should use cache
        let prompt2 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;
        assert_eq!(prompt1, prompt2);

        // Verify cache stats
        let (is_cached, size) = prompt_builder.cache_stats().await;
        assert!(is_cached);
        assert!(size > base_prompt.len());
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
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: None,
            current_token_usage: None,
            supports_context_awareness: false,
            token_budget_guidance: "",
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
        let config1 = SystemPromptConfig::new("Test", true, false, 3);
        let config2 = SystemPromptConfig::new("Test", true, false, 3);

        assert_eq!(config1.hash(), config2.hash());
    }

    #[tokio::test]
    async fn test_context_awareness_token_budget_warning() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "You are a helpful assistant.";
        let context = SystemPromptContext {
            conversation_length: 50,
            tool_usage_count: 20,
            error_count: 1,
            token_usage_ratio: 0.65,
            full_auto: false,
            plan_mode: false,
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: Some(200_000),
            current_token_usage: Some(130_000),
            supports_context_awareness: true,
            token_budget_guidance: "WARNING: Consider updating progress docs to preserve important context.",
        };

        let prompt = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
        assert!(prompt.contains("Token usage: 130000/200000; 70000 remaining"));
        assert!(prompt.contains("WARNING: Consider updating progress docs"));
    }

    #[tokio::test]
    async fn test_context_awareness_token_budget_high() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "You are a helpful assistant.";
        let context = SystemPromptContext {
            conversation_length: 80,
            tool_usage_count: 35,
            error_count: 2,
            token_usage_ratio: 0.88,
            full_auto: true,
            plan_mode: false,
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: Some(200_000),
            current_token_usage: Some(176_000),
            supports_context_awareness: true,
            token_budget_guidance: "HIGH: Start summarizing key findings and preparing for context handoff.",
        };

        let prompt = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
        assert!(prompt.contains("Token usage: 176000/200000; 24000 remaining"));
        assert!(prompt.contains("HIGH: Start summarizing key findings"));
    }

    #[tokio::test]
    async fn test_context_awareness_token_budget_critical() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "You are a helpful assistant.";
        let context = SystemPromptContext {
            conversation_length: 120,
            tool_usage_count: 50,
            error_count: 3,
            token_usage_ratio: 0.95,
            full_auto: false,
            plan_mode: false,
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: Some(200_000),
            current_token_usage: Some(190_000),
            supports_context_awareness: true,
            token_budget_guidance: "CRITICAL: Update artifacts (task.md/docs) and consider starting a new session.",
        };

        let prompt = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
        assert!(prompt.contains("Token usage: 190000/200000; 10000 remaining"));
        assert!(prompt.contains("CRITICAL: Update artifacts"));
    }

    #[tokio::test]
    async fn test_context_awareness_normal_no_guidance() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "You are a helpful assistant.";
        let context = SystemPromptContext {
            conversation_length: 10,
            tool_usage_count: 5,
            error_count: 0,
            token_usage_ratio: 0.10,
            full_auto: false,
            plan_mode: false,
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: Some(200_000),
            current_token_usage: Some(20_000),
            supports_context_awareness: true,
            token_budget_guidance: "",
        };

        let prompt = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
        assert!(prompt.contains("Token usage: 20000/200000; 180000 remaining"));
        assert!(
            !prompt.contains("WARNING:")
                && !prompt.contains("HIGH:")
                && !prompt.contains("CRITICAL:")
        );
    }

    #[tokio::test]
    async fn test_non_context_aware_model_no_budget_tags() {
        let prompt_builder = IncrementalSystemPrompt::new();
        let base_prompt = "You are a helpful assistant.";
        let context = SystemPromptContext {
            conversation_length: 10,
            tool_usage_count: 5,
            error_count: 0,
            token_usage_ratio: 0.10,
            full_auto: false,
            plan_mode: false,
            active_agent_name: String::new(),
            active_agent_prompt: None,
            discovered_skills: Vec::new(),
            context_window_size: None,
            current_token_usage: None,
            supports_context_awareness: false,
            token_budget_guidance: "",
        };

        let prompt = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
            .await;

        assert!(!prompt.contains("<budget:token_budget>"));
        assert!(!prompt.contains("<system_warning>"));
    }
}
