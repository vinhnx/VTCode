use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use vtcode_config::constants::context::{
    TOKEN_BUDGET_CRITICAL_THRESHOLD, TOKEN_BUDGET_HIGH_THRESHOLD, TOKEN_BUDGET_WARNING_THRESHOLD,
};
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
    total_token_usage: usize,
}

/// Token budget status for proactive context management
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenBudgetStatus {
    /// Below 70% - normal operation
    Normal,
    /// 70-85% - start preparing for context handoff
    Warning,
    /// 85-90% - active context management needed
    High,
    /// Above 90% - immediate action required
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
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        _trim_config: (), // Removed trim config parameter
        loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
        agent_config: Option<vtcode_config::core::AgentConfig>,
    ) -> Self {
        Self {
            base_system_prompt: base_system_prompt.clone(),
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            loaded_skills,
            cached_stats: ContextStats::default(),
            agent_config,
        }
    }

    /// Pre-request check that returns recommended action before making an LLM request.
    /// Checks session boundaries to correct runaway sessions.
    pub(crate) fn pre_request_check(&self, history: &[uni::Message]) -> PreRequestAction {
        let hard_limit = self
            .agent_config
            .as_ref()
            .map(|c| c.max_conversation_turns)
            .unwrap_or(150);
        let soft_limit = hard_limit.saturating_sub(30).max(10);

        let msg_count = history.len();

        if msg_count > hard_limit {
            return PreRequestAction::Stop(format!(
                "Session limit reached ({} messages). Please update artifacts (task.md/docs) to persist progress, then start a new session.",
                msg_count
            ));
        }

        if msg_count > soft_limit {
            return PreRequestAction::Warn(format!(
                "Session is getting long ({} messages). Consider updating key artifacts (task.md/docs) to persist context soon.",
                msg_count
            ));
        }

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
            if msg.content.as_text().contains("error") || msg.content.as_text().contains("failed") {
                self.cached_stats.error_count += 1;
            }
        }
        self.cached_stats.last_history_len = new_len;
    }

    /// Update token usage from LLM response
    /// Uses completion_tokens (new output) to track growth rate
    /// since prompt_tokens includes all history and would double-count
    pub(crate) fn update_token_usage(&mut self, usage: &Option<uni::Usage>) {
        if let Some(usage) = usage {
            self.cached_stats.total_token_usage += usage.completion_tokens as usize;
        }
    }

    /// Get guidance message based on token budget status
    /// Returns actionable guidance for context management
    pub(crate) fn get_token_budget_guidance(&self, context_window_size: usize) -> &'static str {
        if context_window_size == 0 {
            return "";
        }

        let usage_ratio = self.cached_stats.total_token_usage as f64 / context_window_size as f64;

        if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
            "CRITICAL: Update artifacts (task.md/docs) and consider starting a new session."
        } else if usage_ratio >= TOKEN_BUDGET_HIGH_THRESHOLD {
            "HIGH: Start summarizing key findings and preparing for context handoff."
        } else if usage_ratio >= TOKEN_BUDGET_WARNING_THRESHOLD {
            "WARNING: Consider updating progress docs to preserve important context."
        } else {
            ""
        }
    }

    /// Get token budget status and guidance together
    #[allow(dead_code)]
    pub(crate) fn get_token_budget_status_and_guidance(
        &self,
        context_window_size: usize,
    ) -> (TokenBudgetStatus, &'static str) {
        if context_window_size == 0 {
            return (TokenBudgetStatus::Normal, "");
        }

        let usage_ratio = self.cached_stats.total_token_usage as f64 / context_window_size as f64;

        if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
            (
                TokenBudgetStatus::Critical,
                "CRITICAL: Update artifacts (task.md/docs) and consider starting a new session.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_HIGH_THRESHOLD {
            (
                TokenBudgetStatus::High,
                "HIGH: Start summarizing key findings and preparing for context handoff.",
            )
        } else if usage_ratio >= TOKEN_BUDGET_WARNING_THRESHOLD {
            (
                TokenBudgetStatus::Warning,
                "WARNING: Consider updating progress docs to preserve important context.",
            )
        } else {
            (TokenBudgetStatus::Normal, "")
        }
    }

    /// Get current token budget status based on usage ratio
    /// Uses thresholds from Anthropic context window documentation:
    /// - 70%: Warning - prepare for handoff
    /// - 85%: High - active management needed
    /// - 90%: Critical - immediate action required
    #[allow(dead_code)]
    pub(crate) fn get_token_budget_status(&self, context_window_size: usize) -> TokenBudgetStatus {
        if context_window_size == 0 {
            return TokenBudgetStatus::Normal;
        }

        let usage_ratio = self.cached_stats.total_token_usage as f64 / context_window_size as f64;

        if usage_ratio >= TOKEN_BUDGET_CRITICAL_THRESHOLD {
            TokenBudgetStatus::Critical
        } else if usage_ratio >= TOKEN_BUDGET_HIGH_THRESHOLD {
            TokenBudgetStatus::High
        } else if usage_ratio >= TOKEN_BUDGET_WARNING_THRESHOLD {
            TokenBudgetStatus::Warning
        } else {
            TokenBudgetStatus::Normal
        }
    }

    /// Get current token usage
    #[allow(dead_code)]
    pub(crate) fn current_token_usage(&self) -> usize {
        self.cached_stats.total_token_usage
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
        full_auto: bool,
        plan_mode: bool,
        context_window_size: Option<usize>,
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

        // Determine if model supports context awareness (Claude 4.5+)
        let supports_context_awareness = context_window_size.is_some();

        // Get token budget guidance if context awareness is supported
        let token_budget_guidance = if supports_context_awareness {
            self.get_token_budget_guidance(context_window_size.unwrap_or(0))
        } else {
            ""
        };

        let context = SystemPromptContext {
            conversation_length: attempt_history.len(),
            tool_usage_count: self.cached_stats.tool_usage_count,
            error_count: self.cached_stats.error_count,
            token_usage_ratio: 0.0,
            full_auto,
            plan_mode,
            discovered_skills: self.loaded_skills.read().await.values().cloned().collect(),
            context_window_size,
            current_token_usage: if supports_context_awareness {
                Some(self.cached_stats.total_token_usage)
            } else {
                None
            },
            supports_context_awareness,
            token_budget_guidance,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PreRequestAction {
    /// Normal operation, proceed with request
    Proceed,
    /// Proceed but inject a warning/reminder to the agent
    Warn(String),
    /// Stop execution and force user intervention or summary
    Stop(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pre_request_check_returns_proceed() {
        let manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        let history = vec![uni::Message::user("hello".to_string())];
        assert_eq!(
            manager.pre_request_check(&history),
            super::PreRequestAction::Proceed
        );
    }

    #[test]
    fn test_pre_request_check_limits() {
        let manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        let mut history = Vec::new();
        for _ in 0..60 {
            history.push(uni::Message::user("test".to_string()));
        }

        assert!(matches!(
            manager.pre_request_check(&history),
            super::PreRequestAction::Warn(_)
        ));
    }

    #[test]
    fn test_token_budget_status_thresholds() {
        let manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Context window of 200K tokens
        let context_size = 200_000;

        // Zero usage should be Normal
        assert_eq!(
            manager.get_token_budget_status(context_size),
            TokenBudgetStatus::Normal
        );
    }

    #[test]
    fn test_token_budget_status_with_zero_context() {
        let manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Zero context window should return Normal (avoid division by zero)
        assert_eq!(
            manager.get_token_budget_status(0),
            TokenBudgetStatus::Normal
        );
    }

    #[tokio::test]
    async fn build_system_prompt_with_empty_base_prompt_fails() {
        let mut manager = ContextManager::new(
            "".to_string(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        let result = manager
            .build_system_prompt(&[], 0, false, false, None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_token_budget_status_warning_threshold() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Set token usage to 70% (140000/200000)
        manager.cached_stats.total_token_usage = 140_000;

        assert_eq!(
            manager.get_token_budget_status(200_000),
            TokenBudgetStatus::Warning
        );
        assert_eq!(
            manager.get_token_budget_guidance(200_000),
            "WARNING: Consider updating progress docs to preserve important context."
        );
    }

    #[test]
    fn test_token_budget_status_high_threshold() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Set token usage to 85% (170000/200000)
        manager.cached_stats.total_token_usage = 170_000;

        assert_eq!(
            manager.get_token_budget_status(200_000),
            TokenBudgetStatus::High
        );
        assert_eq!(
            manager.get_token_budget_guidance(200_000),
            "HIGH: Start summarizing key findings and preparing for context handoff."
        );
    }

    #[test]
    fn test_token_budget_status_critical_threshold() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Set token usage to 90% (180000/200000)
        manager.cached_stats.total_token_usage = 180_000;

        assert_eq!(
            manager.get_token_budget_status(200_000),
            TokenBudgetStatus::Critical
        );
        assert_eq!(
            manager.get_token_budget_guidance(200_000),
            "CRITICAL: Update artifacts (task.md/docs) and consider starting a new session."
        );
    }

    #[test]
    fn test_token_budget_status_normal() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Set token usage to 50% (100000/200000)
        manager.cached_stats.total_token_usage = 100_000;

        assert_eq!(
            manager.get_token_budget_status(200_000),
            TokenBudgetStatus::Normal
        );
        assert_eq!(manager.get_token_budget_guidance(200_000), "");
    }

    #[test]
    fn test_token_budget_status_and_guidance_together() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Test critical threshold
        manager.cached_stats.total_token_usage = 185_000;
        let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
        assert_eq!(status, TokenBudgetStatus::Critical);
        assert!(guidance.contains("CRITICAL"));

        // Test high threshold
        manager.cached_stats.total_token_usage = 175_000;
        let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
        assert_eq!(status, TokenBudgetStatus::High);
        assert!(guidance.contains("HIGH"));

        // Test warning threshold
        manager.cached_stats.total_token_usage = 145_000;
        let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
        assert_eq!(status, TokenBudgetStatus::Warning);
        assert!(guidance.contains("WARNING"));

        // Test normal
        manager.cached_stats.total_token_usage = 50_000;
        let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
        assert_eq!(status, TokenBudgetStatus::Normal);
        assert!(guidance.is_empty());
    }

    #[test]
    fn test_update_token_usage_with_completion_tokens() {
        let mut manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        // Initial state
        assert_eq!(manager.current_token_usage(), 0);

        // Update with first response (500 completion tokens)
        manager.update_token_usage(&Some(uni::Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }));
        assert_eq!(manager.current_token_usage(), 500);

        // Update with second response (800 completion tokens)
        manager.update_token_usage(&Some(uni::Usage {
            prompt_tokens: 2500,
            completion_tokens: 800,
            total_tokens: 3300,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }));
        assert_eq!(manager.current_token_usage(), 1300);
    }
}
