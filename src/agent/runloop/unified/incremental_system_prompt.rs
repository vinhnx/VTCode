//! Incremental system prompt building to avoid redundant string cloning and processing
//!
//! This module provides a cached system prompt builder that only rebuilds the prompt
//! when the underlying configuration or context has changed.

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
}

/// Cached system prompt that avoids redundant rebuilding
#[derive(Clone)]
pub struct IncrementalSystemPrompt {
    /// The cached system prompt content
    cached_prompt: Arc<RwLock<CachedPrompt>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PromptAssemblyMode {
    #[default]
    AppendInstructions,
    BaseIncludesInstructions,
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
    /// Assembly mode for this prompt
    assembly_mode: PromptAssemblyMode,
}

impl IncrementalSystemPrompt {
    pub fn new() -> Self {
        Self {
            cached_prompt: Arc::new(RwLock::new(CachedPrompt {
                content: String::new(),
                config_hash: 0,
                context_hash: 0,
                retry_attempts: usize::MAX, // Force rebuild on first use
                assembly_mode: PromptAssemblyMode::AppendInstructions,
            })),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_system_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
        prompt_assembly_mode: PromptAssemblyMode,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        let read_guard = self.cached_prompt.read().await;

        // Check if we can use the cached version
        if read_guard.config_hash == config_hash
            && read_guard.context_hash == context_hash
            && read_guard.retry_attempts == retry_attempts
            && read_guard.assembly_mode == prompt_assembly_mode
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
            prompt_assembly_mode,
            context,
            agent_config,
        )
        .await
    }

    /// Rebuild the prompt
    #[allow(clippy::too_many_arguments)]
    pub async fn rebuild_prompt(
        &self,
        base_system_prompt: &str,
        config_hash: u64,
        context_hash: u64,
        retry_attempts: usize,
        prompt_assembly_mode: PromptAssemblyMode,
        context: &SystemPromptContext,
        agent_config: Option<&vtcode_config::core::AgentConfig>,
    ) -> String {
        let mut write_guard = self.cached_prompt.write().await;

        // Double-check after acquiring write lock
        if write_guard.config_hash == config_hash
            && write_guard.context_hash == context_hash
            && write_guard.retry_attempts == retry_attempts
            && write_guard.assembly_mode == prompt_assembly_mode
            && !write_guard.content.is_empty()
        {
            return write_guard.content.clone();
        }

        // Build the new prompt
        let new_content = self
            .build_prompt_content(
                base_system_prompt,
                retry_attempts,
                prompt_assembly_mode,
                context,
                agent_config,
            )
            .await;

        // Update cache
        write_guard.content = new_content.clone();
        write_guard.config_hash = config_hash;
        write_guard.context_hash = context_hash;
        write_guard.retry_attempts = retry_attempts;
        write_guard.assembly_mode = prompt_assembly_mode;

        new_content
    }

    /// Actually build the prompt content (this is where the logic goes)
    async fn build_prompt_content(
        &self,
        base_system_prompt: &str,
        retry_attempts: usize,
        prompt_assembly_mode: PromptAssemblyMode,
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
                "\n# Retry #{}: Try a different strategy, not the same steps.",
                retry_attempts
            );
            let _ = writeln!(
                prompt,
                "# Re-plan now: use `task_tracker` (or `plan_task_tracker` in Plan Mode) to define composable slices (files + outcome + verify) before more mutating edits."
            );
        }
        if context.error_count > 0 {
            let _ = writeln!(
                prompt,
                "\n# {} errors: Check file paths, permissions, and tool args, then continue with smaller verified slices.",
                context.error_count
            );
        }

        let has_context = context.conversation_length > 0
            || context.tool_usage_count > 0
            || context.error_count > 0
            || context.token_usage_ratio > 0.0
            || context.full_auto
            || context.plan_mode
            || context.active_agent_prompt.is_some();

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

            if context.full_auto {
                if context.plan_mode {
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

            // Inject active agent's system prompt (replaces hardcoded plan mode injection)
            // This supports the planner/coder subagent architecture
            if let Some(ref agent_prompt) = context.active_agent_prompt {
                // Use the subagent's system prompt directly
                let _ = writeln!(prompt, "\n{}", agent_prompt);
            }

            // Always append runtime plan-mode guardrails when plan mode is active,
            // even when subagent prompts are injected.
            if context.plan_mode {
                append_plan_mode_notice(&mut prompt);
            }
        }

        // Unified Instructions (Project Docs, User Inst, Skills)
        if prompt_assembly_mode == PromptAssemblyMode::AppendInstructions {
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
                if let Some(unified) =
                    get_user_instructions(cfg, &cwd, Some(&skill_metadata[..])).await
                {
                    let _ = writeln!(prompt, "\n# INSTRUCTIONS\n{}", unified);
                }
            } else if !context.discovered_skills.is_empty() {
                // Fallback if config is missing (basic skills rendering)
                let _ = writeln!(
                    prompt,
                    "\n# SKILLS\nUse `list_skills` to see available capabilities."
                );
            }
        } else if !context.discovered_skills.is_empty() {
            // Base prompt already includes instruction hierarchy; append only active skill names.
            let mut skill_names = context
                .discovered_skills
                .iter()
                .map(|s| s.name().to_string())
                .collect::<Vec<_>>();
            skill_names.sort();

            let preview = skill_names.iter().take(12).cloned().collect::<Vec<_>>();
            let _ = writeln!(prompt, "\n# ACTIVE SKILLS\n{}", preview.join(", "));
            if skill_names.len() > preview.len() {
                let _ = writeln!(
                    prompt,
                    "(+{} more active skills)",
                    skill_names.len() - preview.len()
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
        write_guard.assembly_mode = PromptAssemblyMode::AppendInstructions;
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
    pub prompt_assembly_mode: PromptAssemblyMode,
}

impl SystemPromptConfig {
    /// Create a new config with a pre-computed hash of the base prompt
    pub fn new(
        base_prompt: &str,
        enable_retry_context: bool,
        enable_token_tracking: bool,
        max_retry_attempts: usize,
        prompt_assembly_mode: PromptAssemblyMode,
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
            prompt_assembly_mode,
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
        self.prompt_assembly_mode.hash(&mut hasher);
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
#[path = "incremental_system_prompt_tests.rs"]
mod tests;
