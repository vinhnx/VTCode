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
        context: &SystemPromptContext,
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
        context: &SystemPromptContext,
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
        let new_content = self.build_prompt_content(base_system_prompt, retry_attempts, context);

        // Update cache
        write_guard.content = new_content.clone();
        write_guard.config_hash = config_hash;
        write_guard.context_hash = context_hash;
        write_guard.retry_attempts = retry_attempts;

        new_content
    }

    /// Actually build the prompt content (this is where the logic goes)
    fn build_prompt_content(
        &self,
        base_system_prompt: &str,
        retry_attempts: usize,
        context: &SystemPromptContext,
    ) -> String {
        use std::fmt::Write;

        let mut prompt = String::with_capacity(base_system_prompt.len() + 512);
        prompt.push_str(base_system_prompt);

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

            if let Some(plan) = &context.current_plan {
                let _ = writeln!(
                    prompt,
                    "\n## PLAN (v{}) - {}/{} done",
                    plan.version, plan.summary.completed_steps, plan.summary.total_steps
                );
                if let Some(explanation) = &plan.explanation {
                    let _ = writeln!(prompt, "Goal: {}", explanation);
                }
                for (i, step) in plan.steps.iter().enumerate() {
                    let mark = match step.status {
                        vtcode_core::tools::StepStatus::Completed => "[x]",
                        vtcode_core::tools::StepStatus::InProgress => "[>]",
                        vtcode_core::tools::StepStatus::Pending => "[ ]",
                    };
                    let _ = writeln!(prompt, "{}. {} {}", i + 1, mark, step.step);
                }
                let _ = writeln!(prompt, "→ Continue to next step.");
            }
        }
        
        // Skill System Guide (Architectural context for the agent)
        let _ = writeln!(prompt, "\n# HOW TO USE SKILLS
VTCode uses a tiered skill system to optimize context window usage.
1. **Discovery**: Use `list_skills` (with optional `query` or `variety`) to find capabilities.
2. **Activation**: Use `load_skill(name=\"...\")` to activate a skill. This:
   - Registers associated tools into your available toolset.
   - Provides detailed `Instructions` (SKILL.md) for specialized workflows.
3. **Execution**: Avoid manual low-level commands if an `AgentSkill` exists for the task.
4. **Resources**: If a skill references additional files (scripts/ or references/), use `load_skill_resource` to read them on-demand.");

        // Skills Section
        let agent_skills: Vec<_> = context
            .discovered_skills
            .iter()
            .filter(|s| matches!(s.variety, vtcode_core::skills::types::SkillVariety::AgentSkill))
            .collect();

        if !agent_skills.is_empty() {
            let _ = writeln!(prompt, "\n# AVAILABLE SKILLS");
            for skill in agent_skills {
                let status = if !skill.instructions.is_empty() {
                    " [ACTIVE]"
                } else {
                    ""
                };
                let _ = writeln!(prompt, "## {}{}", skill.name(), status);
                let _ = writeln!(prompt, "{}", skill.description());
                if skill.instructions.is_empty() {
                    let _ = writeln!(prompt, "Use `load_skill(\"{}\")` to see full instructions.", skill.name());
                }
            }
        }

        let system_utils: Vec<_> = context
            .discovered_skills
            .iter()
            .filter(|s| {
                matches!(
                    s.variety,
                    vtcode_core::skills::types::SkillVariety::SystemUtility
                )
            })
            .collect();

        if !system_utils.is_empty() {
            let count = system_utils.len();
            // Get a few examples to show the agent what kind of tools are available
            let examples: Vec<_> = system_utils
                .iter()
                .take(5)
                .map(|s| s.name())
                .collect();
            let examples_str = if examples.len() < count {
                format!("{}, ...", examples.join(", "))
            } else {
                examples.join(", ")
            };

            let _ = writeln!(
                prompt,
                "\n# SYSTEM UTILITIES\n- {} tools available ({}) via `list_skills` and `load_skill`.",
                count,
                examples_str
            );
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
    pub current_plan: Option<vtcode_core::tools::TaskPlan>,
    pub full_auto: bool,
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
        if let Some(plan) = &self.current_plan {
            plan.version.hash(&mut hasher);
            plan.summary.completed_steps.hash(&mut hasher);
        }
        self.full_auto.hash(&mut hasher);
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
            token_usage_ratio: 0.1,
            current_plan: None,
            full_auto: false,
            discovered_skills: Vec::new(),
        };

        // First call - should build from scratch
        let prompt1 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context)
            .await;
        assert_eq!(prompt1, base_prompt);

        // Second call with same parameters - should use cache
        let prompt2 = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context)
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
            current_plan: None,
            full_auto: false,
            discovered_skills: Vec::new(),
        };

        // Build initial prompt
        let _ = prompt_builder
            .get_system_prompt(base_prompt, 1, 1, 0, &context)
            .await;

        // Rebuild with different retry attempts
        let prompt = prompt_builder
            .rebuild_prompt(base_prompt, 1, 1, 1, &context)
            .await;

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
