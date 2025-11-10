use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    token_budget: Arc<TokenBudgetManager>,
    token_budget_enabled: bool,
    base_system_prompt: String,
    semantic_analyzer: Option<TreeSitterAnalyzer>,
    semantic_score_cache: Option<HashMap<u64, u8>>,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        trim_config: ContextTrimConfig,
        token_budget: Arc<TokenBudgetManager>,
        token_budget_enabled: bool,
    ) -> Self {
        let (semantic_analyzer, semantic_score_cache) = if trim_config.semantic_compression {
            match TreeSitterAnalyzer::new() {
                Ok(analyzer) => (Some(analyzer), Some(HashMap::new())),
                Err(error) => {
                    warn!(
                        error = %error,
                        "Failed to initialize TreeSitterAnalyzer; disabling semantic compression"
                    );
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Self {
            trim_config,
            token_budget,
            token_budget_enabled,
            base_system_prompt,
            semantic_analyzer,
            semantic_score_cache,
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
        prune_unified_tool_responses(history, &self.trim_config)
    }

    pub(crate) fn enforce_context_window(
        &mut self,
        history: &mut Vec<uni::Message>,
    ) -> ContextTrimOutcome {
        enforce_unified_context_window(
            history,
            self.trim_config,
            self.semantic_analyzer.as_mut(),
            self.semantic_score_cache.as_mut(),
        )
    }

    pub(crate) fn aggressive_trim(&self, history: &mut Vec<uni::Message>) -> usize {
        apply_aggressive_trim_unified(history, self.trim_config)
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        _attempt_history: &[uni::Message],
        retry_attempts: usize,
    ) -> Result<String> {
        let system_prompt = self.base_system_prompt.clone();
        if self.token_budget_enabled {
            self.token_budget
                .count_tokens_for_component(
                    &system_prompt,
                    ContextComponent::SystemPrompt,
                    Some(&format!("base_system_{}", retry_attempts)),
                )
                .await?;
        }

        Ok(system_prompt)
    }
}
