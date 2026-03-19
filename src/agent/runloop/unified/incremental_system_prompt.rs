//! Incremental system prompt building to avoid redundant string cloning and processing
//!
//! This module provides a cached system prompt builder that only rebuilds the prompt
//! when the underlying configuration or context has changed.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

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

fn append_token_warning_if_any(prompt: &mut String, context: &SystemPromptContext) {
    use std::fmt::Write;
    if context.supports_context_awareness
        && let (Some(context_size), Some(used)) =
            (context.context_window_size, context.current_token_usage)
    {
        let remaining = context_size.saturating_sub(used);
        let guidance = context.token_budget_guidance;
        if guidance.is_empty() {
            let _ = writeln!(
                prompt,
                "<system_warning>Token usage: {}/{}; {} remaining.</system_warning>",
                used, context_size, remaining
            );
        } else {
            let _ = writeln!(
                prompt,
                "<system_warning>Token usage: {}/{}; {} remaining. {}</system_warning>",
                used, context_size, remaining, guidance
            );
        }
    }
}

fn append_context_metrics(prompt: &mut String, context: &SystemPromptContext) {
    use std::fmt::Write;
    let _ = writeln!(prompt, "- turns: {}", context.conversation_length);
    let _ = writeln!(prompt, "- tool_calls: {}", context.tool_usage_count);
    let _ = writeln!(prompt, "- errors: {}", context.error_count);
    let _ = writeln!(
        prompt,
        "- token_usage: {:.2}%",
        context.token_usage_ratio * 100.0
    );
    let _ = writeln!(prompt, "- full_auto: {}", context.full_auto);
    append_token_warning_if_any(prompt, context);
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
    /// Hash of the configuration that generated this prompt
    config_hash: u64,
    /// Hash of the context that generated this prompt
    context_hash: u64,
    /// Number of retry attempts this prompt was built for
    retry_attempts: usize,
}

impl IncrementalSystemPrompt {
    pub(crate) fn new() -> Self {
        Self {
            cached_prompt: Arc::new(RwLock::new(CachedPrompt {
                content: String::new(),
                config_hash: 0,
                context_hash: 0,
                retry_attempts: usize::MAX, // Force rebuild on first use
            })),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn get_system_prompt(
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn rebuild_prompt(
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
        use vtcode_core::project_doc::build_instruction_appendix;

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

        if let Some(editor_context_block) = context.editor_context_block.as_deref() {
            let _ = writeln!(prompt, "\n{}", editor_context_block);
        }

        let cache_friendly_mode = context.prompt_cache_shaping_mode.is_enabled();
        let mut runtime_tail = String::new();
        if cache_friendly_mode {
            let has_runtime_context = retry_attempts > 0
                || context.error_count > 0
                || context.conversation_length > 0
                || context.tool_usage_count > 0
                || context.token_usage_ratio > 0.0;
            if has_runtime_context || context.full_auto || context.plan_mode {
                let _ = writeln!(runtime_tail, "\n[Runtime Context]");
                if retry_attempts > 0 {
                    let _ = writeln!(
                        runtime_tail,
                        "# Retry #{}: Try a different strategy, not the same steps.",
                        retry_attempts
                    );
                    let _ = writeln!(
                        runtime_tail,
                        "# Re-plan now: use `task_tracker` to define composable slices (files + outcome + verify) before more mutating edits."
                    );
                }
                if context.error_count > 0 {
                    let _ = writeln!(
                        runtime_tail,
                        "# {} errors: Check file paths, permissions, and tool args, then continue with smaller verified slices.",
                        context.error_count
                    );
                }
                if has_runtime_context {
                    append_context_metrics(&mut runtime_tail, context);
                }
                if context.full_auto {
                    append_full_auto_notice(&mut runtime_tail, context.plan_mode);
                }
                if context.plan_mode {
                    append_plan_mode_notice(&mut runtime_tail);
                }
            }
        } else {
            if retry_attempts > 0 {
                let _ = writeln!(
                    runtime_tail,
                    "\n# Retry #{}: Try a different strategy, not the same steps.",
                    retry_attempts
                );
                let _ = writeln!(
                    runtime_tail,
                    "# Re-plan now: use `task_tracker` to define composable slices (files + outcome + verify) before more mutating edits."
                );
            }
            if context.error_count > 0 {
                let _ = writeln!(
                    runtime_tail,
                    "\n# {} errors: Check file paths, permissions, and tool args, then continue with smaller verified slices.",
                    context.error_count
                );
            }

            let has_context = context.conversation_length > 0
                || context.tool_usage_count > 0
                || context.error_count > 0
                || context.token_usage_ratio > 0.0
                || context.full_auto
                || context.plan_mode;

            if has_context {
                let _ = writeln!(runtime_tail, "\n[Context]");
                append_context_metrics(&mut runtime_tail, context);

                if context.full_auto {
                    append_full_auto_notice(&mut runtime_tail, context.plan_mode);
                }

                if context.plan_mode {
                    append_plan_mode_notice(&mut runtime_tail);
                }
            }
        }

        if let Some(cfg) = agent_config {
            if let Some(active_dir) = context.active_instruction_directory.as_deref()
                && let Some(unified) = build_instruction_appendix(cfg, active_dir).await
            {
                let _ = writeln!(prompt, "\n# INSTRUCTIONS\n{}", unified);
            }
        } else if !context.discovered_skills.is_empty() {
            let _ = writeln!(
                prompt,
                "\n# SKILLS\nUse `list_skills` to see available capabilities."
            );
        }

        if !runtime_tail.trim().is_empty() {
            let _ = writeln!(prompt, "\n{}", runtime_tail.trim_start_matches('\n'));
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

/// Configuration that affects system prompt building
/// Uses pre-computed hash of base_prompt to avoid cloning the full string
#[derive(Debug, Clone, Hash)]
pub(crate) struct SystemPromptConfig {
    /// Pre-computed hash of the base prompt (avoids storing full string)
    pub(crate) base_prompt_hash: u64,
    pub(crate) enable_retry_context: bool,
    pub(crate) enable_token_tracking: bool,
    pub(crate) max_retry_attempts: usize,
}

impl SystemPromptConfig {
    /// Create a new config with a pre-computed hash of the base prompt
    pub(crate) fn new(
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

    pub(crate) fn hash(&self) -> u64 {
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
pub(crate) struct SystemPromptContext {
    pub(crate) conversation_length: usize,
    pub(crate) tool_usage_count: usize,
    pub(crate) error_count: usize,
    pub(crate) token_usage_ratio: f64,
    pub(crate) full_auto: bool,
    /// Plan mode: read-only mode for exploration and planning.
    pub(crate) plan_mode: bool,
    /// Discovered skills for immediate awareness
    pub(crate) discovered_skills: Vec<vtcode_core::skills::types::Skill>,
    /// Total context window size for the current model (e.g., 200000, 1000000)
    pub(crate) context_window_size: Option<usize>,
    /// Current tokens used in the conversation
    pub(crate) current_token_usage: Option<usize>,
    /// Whether the model supports context awareness (Claude 4.5+)
    pub(crate) supports_context_awareness: bool,
    /// Actionable guidance based on token budget status
    pub(crate) token_budget_guidance: &'static str,
    /// Runtime context shaping strategy used to improve prompt prefix cache locality.
    pub(crate) prompt_cache_shaping_mode: PromptCacheShapingMode,
    /// Structured active editor context injected at request time.
    pub(crate) editor_context_block: Option<String>,
    /// Explicit scope root for AGENTS.md and instruction discovery.
    pub(crate) active_instruction_directory: Option<PathBuf>,
}

impl SystemPromptContext {
    pub(crate) fn hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.conversation_length.hash(&mut hasher);
        self.tool_usage_count.hash(&mut hasher);
        self.error_count.hash(&mut hasher);
        ((self.token_usage_ratio * 1000.0) as usize).hash(&mut hasher);
        self.full_auto.hash(&mut hasher);
        self.plan_mode.hash(&mut hasher);
        self.context_window_size.hash(&mut hasher);
        self.current_token_usage.hash(&mut hasher);
        self.supports_context_awareness.hash(&mut hasher);
        self.token_budget_guidance.hash(&mut hasher);
        self.prompt_cache_shaping_mode.hash(&mut hasher);
        self.editor_context_block.hash(&mut hasher);
        self.active_instruction_directory.hash(&mut hasher);
        // We use skill names and versions for hashing
        for skill in &self.discovered_skills {
            skill.name().hash(&mut hasher);
            skill.manifest.version.hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
#[path = "incremental_system_prompt_tests.rs"]
mod tests;
