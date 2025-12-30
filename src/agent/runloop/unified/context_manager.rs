use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, SystemPromptConfig, SystemPromptContext,
};

/// Statistics tracked incrementally to avoid re-scanning history
#[derive(Default, Clone)]
struct ContextStats {
    tool_usage_count: usize,
    error_count: usize,
    last_history_len: usize,
}

/// Simplified ContextManager without context trim and compaction functionality
pub(crate) struct ContextManager {
    base_system_prompt: String,
    incremental_prompt_builder: IncrementalSystemPrompt,
    /// Loaded skills for prompt injection
    loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    /// Incrementally tracked statistics
    cached_stats: ContextStats,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        _trim_config: (), // Removed trim config parameter
        loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    ) -> Self {
        Self {
            base_system_prompt: base_system_prompt.clone(),
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            loaded_skills,
            cached_stats: ContextStats::default(),
        }
    }

    /// Pre-request check that returns recommended action before making an LLM request.
    /// Token budgeting is disabled, always returns Proceed.
    #[allow(dead_code)]
    pub(crate) fn pre_request_check(&self, _history: &[uni::Message]) -> PreRequestAction {
        PreRequestAction::Proceed
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
            if msg.content.as_text().contains("error")
               || msg.content.as_text().contains("failed") {
                self.cached_stats.error_count += 1;
            }
        }
        self.cached_stats.last_history_len = new_len;
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
        full_auto: bool,
    ) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }
        
        // Update statistics incrementally
        self.update_stats(attempt_history);

        // Create configuration and context hashes for cache invalidation
        let config = SystemPromptConfig {
            base_prompt: self.base_system_prompt.clone(),
            enable_retry_context: retry_attempts > 0,
            enable_token_tracking: false,
            max_retry_attempts: 3, // This could be configurable
        };

        let context = SystemPromptContext {
            conversation_length: attempt_history.len(),
            tool_usage_count: self.cached_stats.tool_usage_count,
            error_count: self.cached_stats.error_count,
            token_usage_ratio: 0.0,
            full_auto,
            discovered_skills: self.loaded_skills.read().await.values().cloned().collect(),
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
            )
            .await;

        Ok(system_prompt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PreRequestAction {
    /// Normal operation, proceed with request
    Proceed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pre_request_check_returns_proceed() {
        let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())));

        let history = vec![uni::Message::user("hello".to_string())];
        assert_eq!(
            manager.pre_request_check(&history),
            super::PreRequestAction::Proceed
        );
    }

    #[tokio::test]
    async fn build_system_prompt_with_empty_base_prompt_fails() {
        let mut manager =
            ContextManager::new("".to_string(), (), Arc::new(RwLock::new(HashMap::new())));

        let result = manager.build_system_prompt(&[], 0, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }
}
