use hashbrown::HashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_config::IdeContextConfig;
use vtcode_core::EditorContextSnapshot;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, SystemPromptContext, hash_base_system_prompt,
};

/// Parameters for building system prompts
#[derive(Clone)]
pub(crate) struct SystemPromptParams {
    pub full_auto: bool,
    pub auto_mode: bool,
    pub plan_mode: bool,
    pub request_user_input_enabled: bool,
}

/// Context state tracked outside the prompt builder.
#[derive(Default, Clone)]
struct ContextStats {
    /// Current prompt-side token pressure used for compaction checks.
    total_token_usage: usize,
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
    /// Files observed through editor context or tool activity for instruction matching.
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
            editor_context_snapshot: None,
            ide_context_config: IdeContextConfig::default(),
            session_ide_context_enabled_override: None,
            instruction_activity_paths: BTreeSet::new(),
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
        params: SystemPromptParams,
    ) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }

        let context = SystemPromptContext {
            full_auto: params.full_auto,
            auto_mode: params.auto_mode,
            plan_mode: params.plan_mode,
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
    pub(crate) fn normalize_history_for_request(
        &self,
        history: &[uni::Message],
    ) -> Vec<uni::Message> {
        if history.is_empty() {
            return Vec::new();
        }

        let mut normalized_history = history.to_vec();
        vtcode_core::core::agent::state::normalize_history(&mut normalized_history);

        let mut normalized = Vec::with_capacity(normalized_history.len());
        for message in &normalized_history {
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
            normalized_history
        } else {
            normalized
        }
    }

    pub(crate) fn request_editor_context_message(&self) -> Option<uni::Message> {
        let ide_context_config = self.effective_ide_context_config();
        if !ide_context_config.enabled || !ide_context_config.inject_into_prompt {
            return None;
        }

        let workspace = self.workspace_root.as_deref()?;
        let block = self
            .editor_context_snapshot
            .as_ref()
            .filter(|snapshot| ide_context_config.allows_provider_family(snapshot.provider_family))
            .and_then(|snapshot| {
                snapshot.prompt_block(workspace, ide_context_config.include_selection_text)
            })?;

        Some(uni::Message::user(block))
    }

    fn active_instruction_directory(&self) -> Option<PathBuf> {
        let workspace = self.workspace_root.as_ref()?;
        if let Some(snapshot) = self.editor_context_snapshot.as_ref()
            && let Some(active_file) = snapshot.active_file.as_ref()
            && let Some(path) = self.resolve_editor_context_path(active_file.path.as_str())
            && path.starts_with(workspace)
        {
            return path
                .parent()
                .map(Path::to_path_buf)
                .or_else(|| Some(workspace.clone()));
        }

        self.instruction_activity_paths
            .iter()
            .find(|path| path.starts_with(workspace))
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| Some(workspace.clone()))
    }

    fn instruction_context_paths(&self) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();

        if let Some(snapshot) = self.editor_context_snapshot.as_ref() {
            if let Some(active_file) = snapshot.active_file.as_ref()
                && let Some(path) = self.resolve_editor_context_path(active_file.path.as_str())
            {
                paths.insert(path);
            }

            for editor in &snapshot.visible_editors {
                if let Some(path) = self.resolve_editor_context_path(editor.path.as_str()) {
                    paths.insert(path);
                }
            }
        }

        if let Some(active_dir) = self.active_instruction_directory() {
            paths.insert(active_dir);
        }

        paths.extend(self.instruction_activity_paths.iter().cloned());
        paths.into_iter().collect()
    }

    fn resolve_editor_context_path(&self, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.contains("://") || trimmed.starts_with("untitled:") {
            return None;
        }

        let candidate = Path::new(trimmed);
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.workspace_root.as_ref()?.join(candidate)
        };

        std::fs::canonicalize(&path).ok().or(Some(path))
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
