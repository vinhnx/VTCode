use super::AgentRunner;
use crate::config::VTCodeConfig;
use crate::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use crate::core::agent::task::ContextItem;
use crate::core::agent::task::Task;

impl AgentRunner {
    pub(super) fn is_simple_task(task: &Task, contexts: &[ContextItem]) -> bool {
        let title_chars = task.title.chars().count();
        let description_chars = task.description.chars().count();
        let instructions_chars = task
            .instructions
            .as_ref()
            .map(|text| text.chars().count())
            .unwrap_or(0);
        let total_chars = title_chars + description_chars + instructions_chars;

        let title_words = task.title.split_whitespace().count();
        let description_words = task.description.split_whitespace().count();
        let instructions_words = task
            .instructions
            .as_ref()
            .map(|text| text.split_whitespace().count())
            .unwrap_or(0);
        let total_words = title_words + description_words + instructions_words;

        let context_chars: usize = contexts.iter().map(|ctx| ctx.content.chars().count()).sum();

        total_chars <= 240 && total_words <= 40 && contexts.len() <= 1 && context_chars <= 800
    }

    pub(super) fn config(&self) -> &VTCodeConfig {
        self.config.as_ref()
    }

    #[allow(dead_code)]
    pub(super) fn core_agent_config(&self) -> CoreAgentConfig {
        let cfg = self.config();
        let checkpoint_dir = cfg
            .agent
            .checkpointing
            .storage_dir
            .as_ref()
            .map(|dir| self._workspace.join(dir));

        CoreAgentConfig {
            model: self.model.clone(),
            api_key: self._api_key.clone(),
            provider: cfg.agent.provider.clone(),
            api_key_env: cfg.agent.api_key_env.clone(),
            workspace: self._workspace.clone(),
            verbose: false,
            quiet: self.quiet,
            theme: cfg.agent.theme.clone(),
            reasoning_effort: self.reasoning_effort.unwrap_or(cfg.agent.reasoning_effort),
            ui_surface: cfg.agent.ui_surface,
            prompt_cache: cfg.prompt_cache.clone(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: cfg.agent.custom_api_keys.clone(),
            checkpointing_enabled: cfg.agent.checkpointing.enabled,
            checkpointing_storage_dir: checkpoint_dir,
            checkpointing_max_snapshots: cfg.agent.checkpointing.max_snapshots,
            checkpointing_max_age_days: cfg.agent.checkpointing.max_age_days,
            max_conversation_turns: cfg.agent.max_conversation_turns,
        }
    }
}
