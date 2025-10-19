use std::sync::Arc;

use anyhow::Result;

use vtcode_core::core::context_curator::{ContextCurator, ToolDefinition as CuratorToolDefinition};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::llm::provider as uni;

use super::curator::{build_curated_sections, build_curator_messages};
use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    token_budget: Arc<TokenBudgetManager>,
    token_budget_enabled: bool,
    curator: ContextCurator,
    tool_catalog: Vec<CuratorToolDefinition>,
    base_system_prompt: String,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        trim_config: ContextTrimConfig,
        token_budget: Arc<TokenBudgetManager>,
        token_budget_enabled: bool,
        curator: ContextCurator,
        tool_catalog: Vec<CuratorToolDefinition>,
    ) -> Self {
        Self {
            trim_config,
            token_budget,
            token_budget_enabled,
            curator,
            tool_catalog,
            base_system_prompt,
        }
    }

    pub(crate) fn trim_config(&self) -> ContextTrimConfig {
        self.trim_config
    }

    pub(crate) fn token_budget(&self) -> Arc<TokenBudgetManager> {
        Arc::clone(&self.token_budget)
    }

    pub(crate) fn token_budget_enabled(&self) -> bool {
        self.token_budget_enabled
    }

    pub(crate) async fn reset_token_budget(&self) {
        if self.token_budget_enabled {
            self.token_budget.reset().await;
        }
    }

    pub(crate) fn prune_tool_responses(&self, history: &mut Vec<uni::Message>) -> usize {
        prune_unified_tool_responses(history, self.trim_config.preserve_recent_turns)
    }

    pub(crate) fn enforce_context_window(
        &self,
        history: &mut Vec<uni::Message>,
    ) -> ContextTrimOutcome {
        enforce_unified_context_window(history, self.trim_config)
    }

    pub(crate) fn aggressive_trim(&self, history: &mut Vec<uni::Message>) -> usize {
        apply_aggressive_trim_unified(history, self.trim_config)
    }

    pub(crate) fn clear_curator_state(&mut self) {
        self.curator.clear_active_files();
        self.curator.clear_errors();
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
    ) -> Result<String> {
        let curator_messages = build_curator_messages(
            attempt_history,
            &*self.token_budget,
            self.token_budget_enabled,
        )
        .await?;
        let curated_context = self
            .curator
            .curate_context(&curator_messages, &self.tool_catalog)
            .await?;
        let curated_sections = build_curated_sections(&curated_context);

        let mut system_prompt = self.base_system_prompt.clone();
        if self.token_budget_enabled {
            self.token_budget
                .count_tokens_for_component(
                    &system_prompt,
                    ContextComponent::SystemPrompt,
                    Some(&format!("base_system_{}", retry_attempts)),
                )
                .await?;
        }

        if !curated_sections.is_empty() {
            system_prompt.push_str("\n\n[Curated Context]\n");
            for (idx, section) in curated_sections.iter().enumerate() {
                let body = section.body.trim();
                if body.is_empty() {
                    continue;
                }
                system_prompt.push_str(section.heading);
                system_prompt.push('\n');
                system_prompt.push_str(section.body.trim_end());
                system_prompt.push('\n');
                if self.token_budget_enabled {
                    self.token_budget
                        .count_tokens_for_component(
                            body,
                            section.component,
                            Some(&format!("section_{}_{}", retry_attempts, idx)),
                        )
                        .await?;
                }
            }
        }

        Ok(system_prompt)
    }
}
