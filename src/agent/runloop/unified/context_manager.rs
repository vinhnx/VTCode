use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_config::IdeContextConfig;
use vtcode_config::constants::context::{
    TOKEN_BUDGET_CRITICAL_THRESHOLD, TOKEN_BUDGET_HIGH_THRESHOLD, TOKEN_BUDGET_WARNING_THRESHOLD,
};
use vtcode_core::EditorContextSnapshot;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, PromptCacheShapingMode, SystemPromptConfig, SystemPromptContext,
};

/// Parameters for building system prompts
#[derive(Clone)]
pub(crate) struct SystemPromptParams {
    pub full_auto: bool,
    pub plan_mode: bool,
    pub supports_context_awareness: bool,
    pub context_window_size: Option<usize>,
    pub prompt_cache_shaping_mode: PromptCacheShapingMode,
}

/// Statistics tracked incrementally to avoid re-scanning history
#[derive(Default, Clone)]
struct ContextStats {
    tool_usage_count: usize,
    error_count: usize,
    last_history_len: usize,
    /// Current prompt-side token pressure used for compaction checks.
    total_token_usage: usize,
}

/// Token budget status for proactive context management
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenBudgetStatus {
    /// Below 70% - normal operation
    Normal,
    /// 70-90% - start preparing for context handoff
    Warning,
    /// 90-95% - active context management needed
    High,
    /// Above 95% - immediate action required
    Critical,
}

/// Simplified ContextManager without context trim and compaction functionality
pub(crate) struct ContextManager {
    base_system_prompt: String,
    incremental_prompt_builder: IncrementalSystemPrompt,
    /// Loaded skills for prompt injection
    loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    /// Incrementally tracked statistics
    cached_stats: ContextStats,
    /// Agent configuration
    agent_config: Option<vtcode_config::core::AgentConfig>,
    /// Workspace root used for request-time editor context rendering.
    workspace_root: Option<PathBuf>,
    /// Normalized editor snapshot used for request-time prompt injection.
    editor_context_snapshot: Option<EditorContextSnapshot>,
    /// Prompt/TUI behavior for IDE context injection.
    ide_context_config: IdeContextConfig,
    /// Session-local override for IDE context enablement.
    session_ide_context_enabled_override: Option<bool>,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        _trim_config: (), // Removed trim config parameter
        loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
        agent_config: Option<vtcode_config::core::AgentConfig>,
    ) -> Self {
        Self {
            base_system_prompt,
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            loaded_skills,
            cached_stats: ContextStats::default(),
            agent_config,
            workspace_root: None,
            editor_context_snapshot: None,
            ide_context_config: IdeContextConfig::default(),
            session_ide_context_enabled_override: None,
        }
    }

    pub(crate) fn set_workspace_root(&mut self, workspace_root: &Path) {
        self.workspace_root = Some(workspace_root.to_path_buf());
    }

    pub(crate) fn set_editor_context_snapshot(
        &mut self,
        snapshot: Option<EditorContextSnapshot>,
        ide_context_config: Option<&IdeContextConfig>,
    ) {
        self.editor_context_snapshot = snapshot;
        if let Some(config) = ide_context_config {
            self.ide_context_config = config.clone();
        }
    }

    fn with_session_ide_context_override(&self, mut config: IdeContextConfig) -> IdeContextConfig {
        if let Some(enabled) = self.session_ide_context_enabled_override {
            config.enabled = enabled;
        }
        config
    }

    pub(crate) fn effective_ide_context_config(&self) -> IdeContextConfig {
        self.with_session_ide_context_override(self.ide_context_config.clone())
    }

    pub(crate) fn effective_ide_context_config_with_base(
        &self,
        ide_context_config: Option<&IdeContextConfig>,
    ) -> IdeContextConfig {
        self.with_session_ide_context_override(
            ide_context_config
                .cloned()
                .unwrap_or_else(|| self.ide_context_config.clone()),
        )
    }

    pub(crate) fn toggle_session_ide_context(&mut self) -> bool {
        let enabled = !self.effective_ide_context_config().enabled;
        self.session_ide_context_enabled_override = Some(enabled);
        enabled
    }

    fn update_stats(&mut self, history: &[uni::Message]) {
        let new_len = history.len();
        if new_len < self.cached_stats.last_history_len {
            // History was truncated or reset, full rescan
            self.cached_stats = ContextStats::default();
        } else if new_len == self.cached_stats.last_history_len {
            return;
        }

        // Only scan new messages
        for msg in &history[self.cached_stats.last_history_len..] {
            if msg.tool_calls.is_some() || msg.tool_call_id.is_some() {
                self.cached_stats.tool_usage_count += 1;
            }
            if msg.content.as_text().contains("error") || msg.content.as_text().contains("failed") {
                self.cached_stats.error_count += 1;
            }
        }
        self.cached_stats.last_history_len = new_len;
    }

    /// Update token usage from the latest LLM response.
    ///
    /// We prioritize prompt-side pressure as the compaction signal:
    /// - `prompt_tokens` when available
    /// - fallback to `total_tokens - completion_tokens`
    pub(crate) fn update_token_usage(&mut self, usage: &Option<uni::Usage>) {
        if let Some(usage) = usage {
            let prompt_tokens = usage.prompt_tokens as usize;
            let completion_tokens = usage.completion_tokens as usize;
            let total_tokens = usage.total_tokens as usize;

            let estimated_prompt_pressure = if prompt_tokens > 0 {
                prompt_tokens
            } else if total_tokens > completion_tokens {
                total_tokens.saturating_sub(completion_tokens)
            } else {
                self.cached_stats
                    .total_token_usage
                    .saturating_add(completion_tokens)
            };

            self.cached_stats.total_token_usage = estimated_prompt_pressure;
        }
    }

    /// Validate that ContextManager token tracking matches provider-reported usage
    /// Logs a warning if delta > 5% to catch tracking inconsistencies
    #[cfg(debug_assertions)]
    pub(crate) fn validate_token_tracking(&self, provider_usage: &Option<uni::Usage>) {
        if let Some(usage) = provider_usage {
            let provider_prompt = if usage.prompt_tokens > 0 {
                usage.prompt_tokens as usize
            } else {
                (usage.total_tokens as usize).saturating_sub(usage.completion_tokens as usize)
            };
            let manager_total = self.cached_stats.total_token_usage;

            if provider_prompt > 0 {
                let delta = if provider_prompt > manager_total {
                    (provider_prompt - manager_total) as f64 / provider_prompt as f64
                } else {
                    (manager_total - provider_prompt) as f64 / provider_prompt as f64
                };

                if delta > 0.05 {
                    tracing::warn!(
                        provider_prompt_tokens = provider_prompt,
                        manager_tokens = manager_total,
                        delta_percent = delta * 100.0,
                        "Prompt-token tracking divergence detected between ContextManager and provider usage"
                    );
                }
            }
        }
    }

    /// Compute usage ratio once, avoiding repeated division
    #[inline]
    fn usage_ratio(&self, context_window_size: usize) -> f64 {
        if context_window_size == 0 {
            0.0
        } else {
            self.cached_stats.total_token_usage as f64 / context_window_size as f64
        }
    }

    /// Get token budget status and guidance together (single computation)
    /// Uses thresholds from Anthropic context window documentation:
    /// - 70%: Warning - prepare for handoff
    /// - 90%: High - active management needed
    /// - 95%: Critical - immediate action required
    pub(crate) fn get_token_budget_status_and_guidance(
        &self,
        context_window_size: usize,
    ) -> (TokenBudgetStatus, &'static str) {
        let usage_ratio = self.usage_ratio(context_window_size);

        if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
            (
                TokenBudgetStatus::Critical,
                "CRITICAL: Update artifacts and consider a new session.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_HIGH_THRESHOLD {
            (
                TokenBudgetStatus::High,
                "HIGH: Summarize key findings and prepare a handoff.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_WARNING_THRESHOLD {
            (
                TokenBudgetStatus::Warning,
                "WARNING: Update progress docs to preserve context.",
            )
        } else {
            (TokenBudgetStatus::Normal, "")
        }
    }

    /// Get guidance message based on token budget status
    /// Returns actionable guidance for context management
    pub(crate) fn get_token_budget_guidance(&self, context_window_size: usize) -> &'static str {
        self.get_token_budget_status_and_guidance(context_window_size)
            .1
    }

    /// Get current token budget status based on usage ratio
    #[cfg(test)]
    pub(crate) fn get_token_budget_status(&self, context_window_size: usize) -> TokenBudgetStatus {
        self.get_token_budget_status_and_guidance(context_window_size)
            .0
    }

    /// Get current token usage
    pub(crate) fn current_token_usage(&self) -> usize {
        self.cached_stats.total_token_usage
    }

    /// Cap prompt-pressure tracking after local history compaction.
    pub(crate) fn cap_token_usage_after_compaction(&mut self, threshold: Option<usize>) {
        self.cached_stats.total_token_usage = match threshold {
            Some(limit) if limit > 0 => self.cached_stats.total_token_usage.min(limit - 1),
            Some(_) | None => 0,
        };
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
        params: SystemPromptParams,
    ) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }

        // Update statistics incrementally
        self.update_stats(attempt_history);

        // Create configuration with pre-computed hash (avoids cloning base_prompt)
        let config = SystemPromptConfig::new(
            &self.base_system_prompt,
            retry_attempts > 0,
            false,
            3, // This could be configurable
        );

        // Determine if the provider/model exposes native context-awareness signals.
        let supports_context_awareness = params.supports_context_awareness;

        // Get token budget guidance if context awareness is supported
        let token_budget_guidance = if supports_context_awareness {
            params
                .context_window_size
                .map(|context_size| self.get_token_budget_guidance(context_size))
                .unwrap_or("")
        } else {
            ""
        };

        // Compute token usage ratio from ContextManager's cached stats (single source of truth)
        let token_usage_ratio = if let Some(context_size) = params.context_window_size {
            self.usage_ratio(context_size)
        } else {
            0.0
        };

        let context = SystemPromptContext {
            conversation_length: attempt_history.len(),
            tool_usage_count: self.cached_stats.tool_usage_count,
            error_count: self.cached_stats.error_count,
            token_usage_ratio,
            full_auto: params.full_auto,
            plan_mode: params.plan_mode,
            discovered_skills: self.loaded_skills.read().await.values().cloned().collect(),
            context_window_size: params.context_window_size,
            current_token_usage: if supports_context_awareness {
                Some(self.cached_stats.total_token_usage)
            } else {
                None
            },
            supports_context_awareness,
            token_budget_guidance,
            prompt_cache_shaping_mode: params.prompt_cache_shaping_mode,
            editor_context_block: self.editor_context_prompt_block(),
            active_instruction_directory: self.workspace_root.clone(),
        };

        // Use incremental builder to avoid redundant cloning and processing
        let system_prompt = self
            .incremental_prompt_builder
            .get_system_prompt(
                &self.base_system_prompt,
                config.hash(),
                context.hash(),
                retry_attempts,
                &context,
                self.agent_config.as_ref(),
            )
            .await;

        Ok(system_prompt)
    }

    /// Build a normalized, request-scoped message view without mutating session history.
    ///
    /// This keeps request assembly deterministic and trims no-op artifacts while preserving
    /// tool-calling semantics.
    pub(crate) fn normalize_history_for_request(
        &self,
        history: &[uni::Message],
    ) -> Vec<uni::Message> {
        if history.is_empty() {
            return Vec::new();
        }

        let mut normalized = Vec::with_capacity(history.len());
        for message in history {
            if is_empty_context_message(message) {
                continue;
            }

            if let Some(last) = normalized.last_mut()
                && can_merge_consecutive_assistant_text(last, message)
            {
                append_assistant_text(last, message);
                continue;
            }

            normalized.push(message.clone());
        }

        if normalized.is_empty() {
            history.to_vec()
        } else {
            normalized
        }
    }

    fn editor_context_prompt_block(&self) -> Option<String> {
        let ide_context_config = self.effective_ide_context_config();
        if !ide_context_config.enabled || !ide_context_config.inject_into_prompt {
            return None;
        }

        let workspace = self.workspace_root.as_deref()?;
        self.editor_context_snapshot
            .as_ref()
            .filter(|snapshot| ide_context_config.allows_provider_family(snapshot.provider_family))
            .and_then(|snapshot| {
                snapshot.prompt_block(workspace, ide_context_config.include_selection_text)
            })
    }
}

fn is_empty_context_message(message: &uni::Message) -> bool {
    message.tool_calls.is_none()
        && message.tool_call_id.is_none()
        && message.reasoning.is_none()
        && message.reasoning_details.is_none()
        && message.content.trim().is_empty()
}

fn can_merge_consecutive_assistant_text(previous: &uni::Message, current: &uni::Message) -> bool {
    if previous.role != uni::MessageRole::Assistant || current.role != uni::MessageRole::Assistant {
        return false;
    }

    if previous.phase != current.phase {
        return false;
    }

    if previous.tool_calls.is_some()
        || previous.tool_call_id.is_some()
        || previous.reasoning.is_some()
        || previous.reasoning_details.is_some()
        || previous.origin_tool.is_some()
        || current.tool_calls.is_some()
        || current.tool_call_id.is_some()
        || current.reasoning.is_some()
        || current.reasoning_details.is_some()
        || current.origin_tool.is_some()
    {
        return false;
    }

    matches!(previous.content, uni::MessageContent::Text(_))
        && matches!(current.content, uni::MessageContent::Text(_))
}

fn append_assistant_text(previous: &mut uni::Message, current: &uni::Message) {
    let uni::MessageContent::Text(previous_text) = &mut previous.content else {
        return;
    };
    let uni::MessageContent::Text(current_text) = &current.content else {
        return;
    };

    if !previous_text.is_empty() && !current_text.is_empty() {
        previous_text.push('\n');
    }
    previous_text.push_str(current_text);
}

#[cfg(test)]
#[path = "context_manager_tests.rs"]
mod tests;
