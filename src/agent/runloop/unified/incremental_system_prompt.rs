//! Incremental system prompt building to avoid redundant string cloning and processing
//!
//! This module provides a cached system prompt builder that only rebuilds the prompt
//! when the underlying configuration or context has changed.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

fn append_auto_mode_notice(prompt: &mut String) {
    if prompt.contains("## Auto Mode Active") {
        return;
    }
    prompt.push('\n');
    prompt.push_str(crate::agent::runloop::unified::auto_mode::system_prompt_addendum());
    prompt.push('\n');
}

fn append_plan_mode_notice(prompt: &mut String) {
    if prompt.contains(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_HEADER) {
        return;
    }
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_HEADER);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_NOTICE_LINE);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_EXIT_INSTRUCTION_LINE);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_PLAN_QUALITY_LINE);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_INTERVIEW_POLICY_LINE);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_NO_AUTO_EXIT_LINE);
    prompt.push('\n');
    prompt.push_str(vtcode_core::prompts::system::PLAN_MODE_TASK_TRACKER_LINE);
    prompt.push('\n');
}

fn append_full_auto_notice(prompt: &mut String, plan_mode: bool) {
    use std::fmt::Write;
    if plan_mode {
        let _ = writeln!(
            prompt,
            "\n# FULL-AUTO (PLAN MODE): Work autonomously within Plan Mode constraints."
        );
    } else {
        let _ = writeln!(
            prompt,
            "\n# FULL-AUTO: Complete task autonomously until done or blocked."
        );
    }
}

pub(crate) fn hash_base_system_prompt(base_prompt: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    base_prompt.hash(&mut hasher);
    hasher.finish()
}

/// Cached system prompt that avoids redundant rebuilding
#[derive(Clone)]
pub(crate) struct IncrementalSystemPrompt {
    /// The cached system prompt content
    cached_prompt: Arc<RwLock<CachedPrompt>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum PromptCacheShapingMode {
    #[default]
    Disabled,
    TrailingRuntimeContext,
    AnthropicBlockRuntimeContext,
}

impl PromptCacheShapingMode {
    pub(crate) fn is_enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

#[derive(Clone)]
struct CachedPrompt {
    /// The actual prompt content
    content: String,
    /// Hash of the base prompt that generated this prompt.
    base_prompt_hash: u64,
    /// Hash of the context that generated this prompt
    context_hash: u64,
}

impl IncrementalSystemPrompt {
    pub(crate) fn new() -> Self {
        Self {
            cached_prompt: Arc::new(RwLock::new(CachedPrompt {
                content: String::new(),
                base_prompt_hash: 0,
                context_hash: 0,
            })),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn get_system_prompt(
        &self,
        base_system_prompt: &str,
        base_prompt_hash: u64,
        context_hash: u64,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        let read_guard = self.cached_prompt.read().await;

        // Check if we can use the cached version
        if read_guard.base_prompt_hash == base_prompt_hash
            && read_guard.context_hash == context_hash
            && !read_guard.content.is_empty()
        {
            return read_guard.content.clone();
        }

        // Drop read lock before acquiring write lock
        drop(read_guard);

        // Rebuild the prompt
        self.rebuild_prompt(
            base_system_prompt,
            base_prompt_hash,
            context_hash,
            context,
            agent_config,
        )
        .await
    }

    /// Rebuild the prompt
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn rebuild_prompt(
        &self,
        base_system_prompt: &str,
        base_prompt_hash: u64,
        context_hash: u64,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        let mut write_guard = self.cached_prompt.write().await;

        // Double-check after acquiring write lock
        if write_guard.base_prompt_hash == base_prompt_hash
            && write_guard.context_hash == context_hash
            && !write_guard.content.is_empty()
        {
            return write_guard.content.clone();
        }

        // Build the new prompt
        let new_content = self
            .build_prompt_content(base_system_prompt, context, agent_config)
            .await;

        // Update cache
        write_guard.content = new_content.clone();
        write_guard.base_prompt_hash = base_prompt_hash;
        write_guard.context_hash = context_hash;

        new_content
    }

    /// Actually build the prompt content (this is where the logic goes)
    async fn build_prompt_content(
        &self,
        base_system_prompt: &str,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        use std::fmt::Write;
        use vtcode_core::project_doc::build_instruction_appendix_with_context;

        let mut prompt = String::with_capacity(base_system_prompt.len() + 1024);
        prompt.push_str(base_system_prompt);

        if let Some(cfg) = agent_config {
            if let Some(active_dir) = context.active_instruction_directory.as_deref()
                && let Some(unified) = build_instruction_appendix_with_context(
                    cfg,
                    active_dir,
                    &context.instruction_context_paths,
                )
                .await
            {
                let _ = writeln!(prompt, "\n# INSTRUCTIONS\n{}", unified);
            }
        } else if !context.discovered_skills.is_empty() {
            let _ = writeln!(
                prompt,
                "\n# SKILLS\nUse `list_skills` to see available capabilities."
            );
        }

        if context.full_auto {
            append_full_auto_notice(&mut prompt, context.plan_mode);
        }

        if context.auto_mode && !context.plan_mode {
            append_auto_mode_notice(&mut prompt);
        }

        if context.plan_mode {
            append_plan_mode_notice(&mut prompt);
        }

        prompt
    }

    /// Get cache statistics
    #[cfg(test)]
    async fn cache_stats(&self) -> (bool, usize) {
        let read_guard = self.cached_prompt.read().await;
        (!read_guard.content.is_empty(), read_guard.content.len())
    }
}

impl Default for IncrementalSystemPrompt {
    fn default() -> Self {
        Self::new()
    }
}

/// Context that affects system prompt building
#[derive(Debug, Clone)]
pub(crate) struct SystemPromptContext {
    pub(crate) full_auto: bool,
    pub(crate) auto_mode: bool,
    /// Plan mode: read-only mode for exploration and planning.
    pub(crate) plan_mode: bool,
    /// Discovered skills for immediate awareness
    pub(crate) discovered_skills: Vec<vtcode_core::skills::types::Skill>,
    /// Explicit scope root for AGENTS.md and instruction discovery.
    pub(crate) active_instruction_directory: Option<PathBuf>,
    /// Files and directories used to activate path-scoped instruction rules.
    pub(crate) instruction_context_paths: Vec<PathBuf>,
}

impl SystemPromptContext {
    pub(crate) fn hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.full_auto.hash(&mut hasher);
        self.auto_mode.hash(&mut hasher);
        self.plan_mode.hash(&mut hasher);
        self.active_instruction_directory.hash(&mut hasher);
        for path in &self.instruction_context_paths {
            path.hash(&mut hasher);
        }
        // Include the lean skill metadata that appears in the prompt.
        for skill in &self.discovered_skills {
            skill.name().hash(&mut hasher);
            skill.description().hash(&mut hasher);
            skill.path.hash(&mut hasher);
            skill.scope.hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
#[path = "incremental_system_prompt_tests.rs"]
mod tests;
