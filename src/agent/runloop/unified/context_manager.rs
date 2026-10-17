use hashbrown::HashMap;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, SystemPromptContext, hash_base_system_prompt,
};

/// Parameters for building system prompts
#[derive(Clone)]
pub(crate) struct SystemPromptParams {
    pub full_auto: bool,
    pub planning_active: bool,
    pub request_user_input_enabled: bool,
}

/// Context state tracked outside the prompt builder.
#[derive(Default, Clone)]
struct ContextStats {
    /// Current prompt-side token pressure used for compaction checks.
    total_token_usage: usize,
    /// Set when pressure crosses the soft compaction threshold. The flag is
    /// intentionally separate from the current estimate so a boundary can
    /// consume it explicitly before the next request records fresh pressure.
    compaction_pending: bool,
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
    /// Workspace root used for request-time instruction path rendering.
    workspace_root: Option<PathBuf>,
    /// Files observed through tool activity for instruction matching.
    instruction_activity_paths: BTreeSet<PathBuf>,
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
            instruction_activity_paths: BTreeSet::new(),
        }
    }

    pub(crate) fn set_workspace_root(&mut self, workspace_root: &Path) {
        self.workspace_root = Some(workspace_root.to_path_buf());
    }

    /// Replace the composed base system prompt after deferred session hydration.
    pub(crate) fn set_base_system_prompt(&mut self, base_system_prompt: String) {
        self.base_system_prompt = base_system_prompt;
    }

    #[cfg(test)]
    pub(crate) fn default_for_test() -> Self {
        Self::new(String::new(), (), Arc::new(RwLock::new(HashMap::new())), None)
    }

    pub(crate) fn record_instruction_activity_paths<I>(&mut self, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        self.instruction_activity_paths.extend(paths);
    }

    pub(crate) fn tracked_instruction_activity_paths(&self) -> Vec<PathBuf> {
        self.instruction_activity_paths.iter().cloned().collect()
    }

    pub(crate) fn instruction_context_paths_snapshot(&self) -> Vec<PathBuf> {
        self.instruction_context_paths()
    }

    pub(crate) fn active_instruction_directory_snapshot(&self) -> Option<PathBuf> {
        self.active_instruction_directory()
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
                // Neither prompt_tokens nor (total - completion) is available;
                // can't estimate prompt pressure. Preserve previous reading.
                self.cached_stats.total_token_usage
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

    /// Get current token usage
    pub(crate) fn current_token_usage(&self) -> usize {
        self.cached_stats.total_token_usage
    }

    /// Record a lower-bound estimate for the assembled wire prompt before the
    /// provider call. Provider usage remains authoritative when available,
    /// but this estimate keeps newly rendered instructions, tool schemas, and
    /// history growth in the next compaction decision when a request fails
    /// before returning usage.
    pub(crate) fn record_prompt_estimate(&mut self, estimated_tokens: usize) {
        self.cached_stats.total_token_usage = self.cached_stats.total_token_usage.max(estimated_tokens);
    }

    /// Derive the internal soft threshold from the effective hard threshold.
    /// This is deliberately not configurable: it only schedules work at a
    /// safe boundary and must not change the request/cache shape.
    pub(crate) fn compaction_soft_threshold(hard_threshold: Option<usize>) -> Option<usize> {
        hard_threshold.map(|threshold| threshold.saturating_sub(threshold / 10).max(1))
    }

    pub(crate) fn mark_compaction_pending_at_soft_threshold(&mut self, hard_threshold: Option<usize>) -> bool {
        let Some(soft_threshold) = Self::compaction_soft_threshold(hard_threshold) else {
            return false;
        };
        if self.current_token_usage() < soft_threshold {
            return false;
        }
        self.cached_stats.compaction_pending = true;
        true
    }

    pub(crate) fn compaction_pending(&self) -> bool {
        self.cached_stats.compaction_pending
    }

    pub(crate) fn take_compaction_pending(&mut self) -> bool {
        std::mem::take(&mut self.cached_stats.compaction_pending)
    }

    pub(crate) fn context_usage_percent(&self, max_context_tokens: usize) -> u8 {
        if max_context_tokens == 0 {
            return 0;
        }
        self.current_token_usage()
            .saturating_mul(100)
            .checked_div(max_context_tokens)
            .unwrap_or(0)
            .min(100) as u8
    }

    pub(crate) fn reset_for_fresh_execution(&mut self) {
        self.cached_stats = ContextStats::default();
        self.incremental_prompt_builder = IncrementalSystemPrompt::new();
    }

    pub(crate) async fn active_loaded_skill_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.loaded_skills.read().await.keys().cloned().collect();
        names.sort_unstable();
        names
    }

    /// Reset prompt-pressure tracking after history compaction.
    ///
    /// The next request builder records a fresh estimate from the compacted
    /// wire request. Retaining the pre-compaction threshold here would keep
    /// pressure artificially high and can trigger the same compaction again
    /// before the provider has a chance to report authoritative usage.
    pub(crate) fn reset_token_pressure_after_compaction(&mut self) {
        self.cached_stats.compaction_pending = false;
        self.cached_stats.total_token_usage = 0;
    }

    pub(crate) async fn build_system_prompt(&mut self, params: SystemPromptParams) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }

        let context = SystemPromptContext {
            full_auto: params.full_auto,
            planning_active: params.planning_active,
            request_user_input_enabled: params.request_user_input_enabled,
            discovered_skills: self.loaded_skills.read().await.values().cloned().collect(),
            active_instruction_directory: self.active_instruction_directory(),
            instruction_context_paths: self.instruction_context_paths(),
        };

        // Use incremental builder to avoid redundant cloning and processing
        let system_prompt = self
            .incremental_prompt_builder
            .get_system_prompt(
                &self.base_system_prompt,
                hash_base_system_prompt(&self.base_system_prompt),
                context.hash(),
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
    pub(crate) fn normalize_history_for_request<'a>(&self, history: &'a [uni::Message]) -> Cow<'a, [uni::Message]> {
        let request_history_needs_repair =
            vtcode_core::core::agent::state::request_history_needs_normalization(history);
        if !request_history_needs_repair && !history_needs_message_cleanup(history) {
            return Cow::Borrowed(history);
        }

        let mut normalized = if request_history_needs_repair {
            vtcode_core::core::agent::state::normalize_history_for_request(history)
        } else {
            history.to_vec()
        };

        // Filter and merge in-place to avoid a second full clone.
        let mut write = 0;
        for read in 0..normalized.len() {
            if is_empty_context_message(&normalized[read]) {
                continue;
            }

            if write > 0 && can_merge_consecutive_assistant_text(&normalized[write - 1], &normalized[read]) {
                // Clone only the current message for merging (much cheaper than cloning all).
                let current = normalized[read].clone();
                append_assistant_text(&mut normalized[write - 1], &current);
                continue;
            }

            if write != read {
                normalized.swap(write, read);
            }
            write += 1;
        }

        normalized.truncate(write);
        Cow::Owned(normalized)
    }

    fn active_instruction_directory(&self) -> Option<PathBuf> {
        let workspace = self.workspace_root.as_ref()?;

        self.instruction_activity_paths
            .iter()
            .find(|path| path.starts_with(workspace))
            .and_then(|path| path.parent().filter(|p| p.starts_with(workspace)).map(Path::to_path_buf))
            .or_else(|| Some(workspace.clone()))
    }

    fn instruction_context_paths(&self) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();

        if let Some(active_dir) = self.active_instruction_directory() {
            paths.insert(active_dir);
        }

        paths.extend(self.instruction_activity_paths.iter().cloned());
        paths.into_iter().collect()
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

    matches!(previous.content, uni::MessageContent::Text(_)) && matches!(current.content, uni::MessageContent::Text(_))
}

fn history_needs_message_cleanup(history: &[uni::Message]) -> bool {
    let mut previous = None;
    for message in history {
        if is_empty_context_message(message) {
            return true;
        }

        if previous.is_some_and(|previous| can_merge_consecutive_assistant_text(previous, message)) {
            return true;
        }
        previous = Some(message);
    }

    false
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
