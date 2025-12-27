use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Result, bail};
use tracing::{debug, warn};

use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};
use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, SystemPromptConfig, SystemPromptContext,
};
use vtcode_core::constants::context as context_constants;
use vtcode_core::core::pruning_decisions::{PruningDecisionLedger, RetentionChoice};
use vtcode_core::core::{
    ContextEfficiency, ContextPruner, MessageMetrics, MessageType, RetentionDecision,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreRequestAction {
    /// Normal operation, proceed with request
    Proceed,
    /// Light trimming recommended (at WARNING threshold)
    TrimLight,
    /// Aggressive trimming required (at ALERT threshold)
    TrimAggressive,

    /// Context overflow, unsafe to proceed
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetStatus {
    Safe,
    Warning(f64),
    Critical(f64),
    Overflow { estimated: usize, limit: usize },
}

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    base_system_prompt: String,
    incremental_prompt_builder: IncrementalSystemPrompt,
    #[allow(dead_code)]
    semantic_analyzer: Option<TreeSitterAnalyzer>,
    semantic_score_cache: Option<HashMap<u64, u8>>,
    context_pruner: ContextPruner,
    last_efficiency: Option<ContextEfficiency>,
    /// Loaded skills for prompt injection
    #[allow(dead_code)]
    loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
}

impl ContextManager {
    pub(crate) fn new(
        base_system_prompt: String,
        trim_config: ContextTrimConfig,
        loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
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

        let context_pruner = ContextPruner::new(trim_config.max_tokens);

        Self {
            trim_config,
            base_system_prompt: base_system_prompt.clone(),
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            semantic_analyzer,
            semantic_score_cache,
            context_pruner,
            last_efficiency: None,
            loaded_skills,
        }
    }

    pub(crate) fn trim_config(&self) -> ContextTrimConfig {
        self.trim_config
    }

    /// Estimate the total tokens that would be used for the current conversation state
    /// Includes history + estimated overhead for the next model response
    pub(crate) fn estimate_request_tokens(&self, history: &[uni::Message]) -> usize {
        let history_tokens: usize = history
            .iter()
            .map(|msg| match &msg.content {
                uni::MessageContent::Text(text) => (text.len()
                    / context_constants::CHAR_PER_TOKEN_APPROXIMATION)
                    .max(context_constants::MIN_TOKEN_COUNT),
                uni::MessageContent::Parts(_) => context_constants::DEFAULT_TOKENS_FOR_PARTS,
            })
            .sum();

        // Add estimated overhead for model response (typ. ~2000 tokens avg)
        const MODEL_RESPONSE_OVERHEAD: usize = 2000;

        history_tokens + MODEL_RESPONSE_OVERHEAD
    }

    /// Detailed check of budget status
    /// Token budgeting is disabled, always returns Safe
    pub(crate) fn check_budget_violation(&self, _history: &[uni::Message]) -> BudgetStatus {
        BudgetStatus::Safe
    }

    /// Check if the estimated token usage would exceed a given threshold
    /// Returns true if preemptive action (trimming) is recommended
    #[allow(dead_code)]
    pub(crate) fn will_exceed_threshold(&self, history: &[uni::Message], threshold: f64) -> bool {
        match self.check_budget_violation(history) {
            BudgetStatus::Safe => false,
            BudgetStatus::Warning(r) | BudgetStatus::Critical(r) => r >= threshold,
            BudgetStatus::Overflow { .. } => true,
        }
    }

    /// Pre-request check that returns recommended action before making an LLM request.
    /// Token budgeting is disabled, always returns Proceed.
    pub(crate) fn pre_request_check(&self, _history: &[uni::Message]) -> PreRequestAction {
        PreRequestAction::Proceed
    }

    #[allow(dead_code)]
    pub(crate) fn prune_tool_responses(&self, history: &mut Vec<uni::Message>) -> usize {
        prune_unified_tool_responses(history, &self.trim_config)
    }

    #[allow(dead_code)]
    pub(crate) fn enforce_context_window(
        &mut self,
        history: &mut Vec<uni::Message>,
    ) -> ContextTrimOutcome {
        let outcome = enforce_unified_context_window(
            history,
            self.trim_config,
            self.semantic_analyzer.as_mut(),
            self.semantic_score_cache.as_mut(),
        );

        // Record efficiency metrics from the trimming operation
        self.record_efficiency_after_trim(history, &outcome);

        outcome
    }

    /// HP-9: Check if context window enforcement is needed
    ///
    /// Returns true if history should be trimmed based on heuristics:
    /// - Message count exceeds 75% of estimated max (assuming ~100 tokens/msg)
    /// - OR always enforce if semantic compression is enabled (for quality)
    pub(crate) fn should_enforce_context(&self, history: &[uni::Message]) -> bool {
        // Always enforce if semantic compression is enabled for quality
        if self.trim_config.semantic_compression {
            return true;
        }

        // Estimate: ~100 tokens per message on average
        const ESTIMATED_TOKENS_PER_MESSAGE: usize = 100;
        let estimated_max_messages = self.trim_config.max_tokens / ESTIMATED_TOKENS_PER_MESSAGE;

        // Enforce when we exceed 75% of estimated capacity
        let threshold = (estimated_max_messages * 3) / 4;
        history.len() > threshold
    }

    /// Apply ContextPruner recommendations to remove low-priority messages with decision tracking
    pub(crate) fn prune_with_semantic_priority(
        &mut self,
        history: &mut Vec<uni::Message>,
        mut pruning_ledger: Option<&mut PruningDecisionLedger>,
        turn_number: usize,
    ) -> (usize, bool) {
        let before_len = history.len();
        if self.trim_config.semantic_compression {
            // Build metrics for all messages
            let mut metrics_list = Vec::new();

            for (idx, msg) in history.iter().enumerate() {
                // Preserve system message
                if matches!(msg.role, uni::MessageRole::System) {
                    continue;
                }

                // Use ContextPruner to get retention decision
                let semantic_score = if let Some(cache) = &self.semantic_score_cache {
                    cache
                        .get(&(idx as u64))
                        .copied()
                        .unwrap_or(context_constants::DEFAULT_SEMANTIC_CACHE_SCORE)
                        as u32
                        * context_constants::SEMANTIC_SCORE_SCALING_FACTOR
                } else {
                    context_constants::DEFAULT_SEMANTIC_SCORE
                };

                let message_type = match &msg.role {
                    uni::MessageRole::System => MessageType::System,
                    uni::MessageRole::User => MessageType::User,
                    uni::MessageRole::Assistant => MessageType::Assistant,
                    uni::MessageRole::Tool => MessageType::Tool,
                };

                let token_count = match &msg.content {
                    uni::MessageContent::Text(text) => (text.len()
                        / context_constants::CHAR_PER_TOKEN_APPROXIMATION)
                        .max(context_constants::MIN_TOKEN_COUNT),
                    uni::MessageContent::Parts(_) => context_constants::DEFAULT_TOKENS_FOR_PARTS,
                };

                let metrics = MessageMetrics {
                    index: idx,
                    token_count,
                    semantic_score,
                    age_in_turns: (history.len().saturating_sub(idx + 1)) as u32,
                    message_type,
                };

                metrics_list.push((idx, metrics));
            }

            // Get decisions from ContextPruner in batch
            let metrics_only: Vec<_> = metrics_list.iter().map(|(_, m)| m).cloned().collect();
            let decisions = self.context_pruner.prune_messages(&metrics_only);

            // Record decisions and collect indices to remove
            let mut indices_to_remove = Vec::new();
            for (idx, metrics) in &metrics_list {
                if let Some(decision) = decisions.get(idx) {
                    // Record the decision in the ledger if available
                    if let Some(ledger) = pruning_ledger.as_mut() {
                        let retention_choice = match decision {
                            RetentionDecision::Keep => RetentionChoice::Keep,
                            RetentionDecision::Remove => RetentionChoice::Remove,
                            RetentionDecision::Summarizable => RetentionChoice::Summarize,
                        };

                        let reason = format!(
                            "score={}, age={}",
                            metrics.semantic_score, metrics.age_in_turns
                        );

                        ledger.record_decision(
                            turn_number,
                            *idx,
                            (metrics.semantic_score
                                / context_constants::SEMANTIC_SCORE_SCALING_FACTOR)
                                as u16,
                            metrics.token_count,
                            metrics.age_in_turns as usize,
                            retention_choice,
                            &reason,
                        );
                    }

                    if matches!(decision, RetentionDecision::Remove) {
                        indices_to_remove.push(*idx);
                    }
                }
            }

            // Remove in reverse order to preserve indices
            for idx in indices_to_remove.iter().rev() {
                history.remove(*idx);
                debug!("Pruned message at index {} for low semantic priority", idx);
            }

            // Record pruning round completion
            if let Some(ledger) = pruning_ledger.as_mut() {
                ledger.record_pruning_round();
            }

            let total_summarizable = metrics_list
                .iter()
                .filter(|(idx, _)| {
                    matches!(decisions.get(idx), Some(RetentionDecision::Summarizable))
                })
                .count();

            // Return removed count and whether summarization is recommended
            // Threshold: if > 10% of messages are summarizable or > 5 messages
            let summarization_recommended = total_summarizable > 5
                || (total_summarizable > 0 && total_summarizable >= history.len() / 10);

            (
                before_len.saturating_sub(history.len()),
                summarization_recommended,
            )
        } else {
            (0, false)
        }
    }

    #[allow(dead_code)]
    pub(crate) fn aggressive_trim(&self, history: &mut Vec<uni::Message>) -> usize {
        apply_aggressive_trim_unified(history, self.trim_config)
    }

    /// Adaptive trim based on budget thresholds; returns structured outcome
    pub(crate) async fn adaptive_trim(
        &mut self,
        _history: &mut Vec<uni::Message>,
        _pruning_ledger: Option<&mut PruningDecisionLedger>,
        _turn_number: usize,
    ) -> Result<ContextTrimOutcome> {
        // Token budgeting is disabled, no trimming needed
        Ok(ContextTrimOutcome::default())
    }

    pub(crate) async fn build_system_prompt(
        &mut self,
        attempt_history: &[uni::Message],
        retry_attempts: usize,
        current_plan: Option<vtcode_core::tools::TaskPlan>,
        full_auto: bool,
    ) -> Result<String> {
        if self.base_system_prompt.trim().is_empty() {
            bail!("Base system prompt is empty; cannot build prompt");
        }
        // Create configuration and context hashes for cache invalidation
        let config = SystemPromptConfig {
            base_prompt: self.base_system_prompt.clone(),
            enable_retry_context: retry_attempts > 0,
            enable_token_tracking: false,
            max_retry_attempts: 3, // This could be configurable
        };

        let context = SystemPromptContext {
            conversation_length: attempt_history.len(),
            tool_usage_count: attempt_history
                .iter()
                .filter(|msg| msg.tool_calls.is_some() || msg.tool_call_id.is_some())
                .count(),
            error_count: attempt_history
                .iter()
                .filter(|msg| {
                    msg.content.as_text().contains("error")
                        || msg.content.as_text().contains("failed")
                })
                .count(),
            token_usage_ratio: 0.0,
            current_plan,
            full_auto,
            discovered_skills: self
                .loaded_skills
                .read()
                .await
                .values()
                .cloned()
                .collect(),
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

    pub(crate) fn last_efficiency(&self) -> Option<&ContextEfficiency> {
        self.last_efficiency.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn log_efficiency_metrics(&self) {
        if let Some(efficiency) = &self.last_efficiency {
            debug!(
                "Context efficiency: {:.1}% utilization ({} tokens), {} messages, {:.2} semantic value/token",
                efficiency.context_utilization_percent,
                efficiency.total_tokens,
                efficiency.total_messages,
                efficiency.semantic_value_per_token
            );
        }
    }

    /// Record efficiency metrics after trimming operation
    #[allow(dead_code)]
    fn record_efficiency_after_trim(
        &mut self,
        history: &[uni::Message],
        _outcome: &ContextTrimOutcome,
    ) {
        let total_tokens = history
            .iter()
            .map(|msg| match &msg.content {
                uni::MessageContent::Text(text) => (text.len()
                    / context_constants::CHAR_PER_TOKEN_APPROXIMATION)
                    .max(context_constants::MIN_TOKEN_COUNT),
                uni::MessageContent::Parts(_) => context_constants::DEFAULT_TOKENS_FOR_PARTS,
            })
            .sum::<usize>();

        let total_semantic_value: u32 = if let Some(cache) = &self.semantic_score_cache {
            cache
                .values()
                .map(|&v| v as u32 * context_constants::SEMANTIC_SCORE_SCALING_FACTOR)
                .sum()
        } else {
            history
                .iter()
                .map(|msg| {
                    if matches!(msg.role, uni::MessageRole::System) {
                        context_constants::SYSTEM_MESSAGE_SEMANTIC_SCORE
                    } else if matches!(msg.role, uni::MessageRole::User) {
                        context_constants::USER_MESSAGE_SEMANTIC_SCORE
                    } else {
                        context_constants::DEFAULT_SEMANTIC_SCORE
                    }
                })
                .sum()
        };

        let context_utilization = if self.trim_config.max_tokens > 0 {
            (total_tokens as f64 / self.trim_config.max_tokens as f64)
                * context_constants::PERCENTAGE_CONVERSION_FACTOR
        } else {
            0.0
        };

        let semantic_per_token = if total_tokens > 0 {
            total_semantic_value as f64 / total_tokens as f64
        } else {
            0.0
        };

        let avg_semantic = if !history.is_empty() {
            total_semantic_value / history.len() as u32
        } else {
            0
        };

        self.last_efficiency = Some(ContextEfficiency {
            context_utilization_percent: context_utilization,
            total_tokens,
            total_messages: history.len(),
            semantic_value_per_token: semantic_per_token,
            avg_semantic_score: avg_semantic,
        });
    }

    pub(crate) fn get_summarizable_indices(&mut self, history: &[uni::Message]) -> Vec<usize> {
        if !self.trim_config.semantic_compression {
            return Vec::new();
        }

        let mut metrics_list = Vec::new();
        for (idx, msg) in history.iter().enumerate() {
            if matches!(msg.role, uni::MessageRole::System) {
                continue;
            }

            let semantic_score = if let Some(cache) = &self.semantic_score_cache {
                cache
                    .get(&(idx as u64))
                    .copied()
                    .unwrap_or(context_constants::DEFAULT_SEMANTIC_CACHE_SCORE)
                    as u32
                    * context_constants::SEMANTIC_SCORE_SCALING_FACTOR
            } else {
                context_constants::DEFAULT_SEMANTIC_SCORE
            };

            let message_type = match &msg.role {
                uni::MessageRole::System => MessageType::System,
                uni::MessageRole::User => MessageType::User,
                uni::MessageRole::Assistant => MessageType::Assistant,
                uni::MessageRole::Tool => MessageType::Tool,
            };

            let token_count = match &msg.content {
                uni::MessageContent::Text(text) => (text.len()
                    / context_constants::CHAR_PER_TOKEN_APPROXIMATION)
                    .max(context_constants::MIN_TOKEN_COUNT),
                uni::MessageContent::Parts(_) => context_constants::DEFAULT_TOKENS_FOR_PARTS,
            };

            metrics_list.push(MessageMetrics {
                index: idx,
                token_count,
                semantic_score,
                age_in_turns: (history.len().saturating_sub(idx + 1)) as u32,
                message_type,
            });
        }

        let decisions = self.context_pruner.prune_messages(&metrics_list);

        metrics_list
            .iter()
            .filter_map(|metrics| {
                if matches!(
                    decisions.get(&metrics.index),
                    Some(RetentionDecision::Summarizable)
                ) {
                    Some(metrics.index)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Convert LLM messages to MessageMetrics for ContextPruner analysis
    #[allow(dead_code)]
    fn convert_to_message_metrics(
        messages: &[uni::Message],
        semantic_scores: &[u8],
    ) -> Vec<MessageMetrics> {
        let now = messages.len();

        messages
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let message_type = match &msg.role {
                    uni::MessageRole::System => MessageType::System,
                    uni::MessageRole::User => MessageType::User,
                    uni::MessageRole::Assistant => MessageType::Assistant,
                    uni::MessageRole::Tool => MessageType::Tool,
                };

                let semantic_score = if idx < semantic_scores.len() {
                    semantic_scores[idx] as u32 * context_constants::SEMANTIC_SCORE_SCALING_FACTOR // Scale from 0-255 to 0-1000
                } else {
                    context_constants::DEFAULT_SEMANTIC_SCORE
                };

                let age_in_turns = (now.saturating_sub(idx + 1)) as u32;

                // Approximate tokens: ~4 characters per token
                let content_len = match &msg.content {
                    uni::MessageContent::Text(text) => text.len(),
                    uni::MessageContent::Parts(_) => context_constants::DEFAULT_TOKENS_FOR_PARTS, // Rough estimate for parts
                };
                let token_count = (content_len / context_constants::CHAR_PER_TOKEN_APPROXIMATION)
                    .max(context_constants::MIN_TOKEN_COUNT);

                MessageMetrics {
                    index: idx,
                    token_count,
                    semantic_score,
                    age_in_turns,
                    message_type,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_history(len: usize) -> Vec<uni::Message> {
        let mut history = Vec::new();
        for idx in 0..len {
            if idx < 3 {
                history.push(uni::Message::assistant(format!("tool-{idx}")));
            } else if idx % 2 == 0 {
                history.push(uni::Message::assistant("assistant".to_string()));
            } else {
                history.push(uni::Message::user("user".to_string()));
            }
        }
        history
    }

    #[tokio::test]
    async fn adaptive_trim_prunes_tools_at_warning_threshold() {
        let trim_config = ContextTrimConfig {
            max_tokens: 100,
            ..ContextTrimConfig::default()
        };
        let mut manager = ContextManager::new(
            "sys".into(),
            trim_config,
            Arc::new(RwLock::new(HashMap::new())),
        );

        // Build a history that would need trimming
        let mut history: Vec<uni::Message> = (0..50)
            .map(|_| uni::Message::assistant("x".repeat(200)))
            .collect();

        let outcome = manager
            .adaptive_trim(&mut history, None, 1)
            .await
            .expect("adaptive trim should succeed");

        // Token budgeting is disabled, so no trimming should occur
        assert_eq!(outcome.phase, TrimPhase::None);
        assert_eq!(outcome.removed_messages, 0);
    }

    #[tokio::test]
    async fn adaptive_trim_compacts_at_alert_threshold() {
        let trim_config = ContextTrimConfig {
            max_tokens: 40,
            ..ContextTrimConfig::default()
        };
        let mut manager = ContextManager::new(
            "sys".into(),
            trim_config,
            Arc::new(RwLock::new(HashMap::new())),
        );

        // Build a history that exceeds the context window
        let mut history: Vec<uni::Message> = (0..10)
            .map(|_| uni::Message::assistant("x".repeat(200)))
            .collect();

        let outcome = manager
            .adaptive_trim(&mut history, None, 2)
            .await
            .expect("adaptive trim should succeed");

        // Token budgeting is disabled, so no trimming should occur
        assert_eq!(outcome.phase, TrimPhase::None);
        assert_eq!(outcome.removed_messages, 0);
    }

    #[test]
    fn pre_request_check_returns_proceed_when_budget_disabled() {
        let trim_config = ContextTrimConfig {
            max_tokens: 100_000,
            ..ContextTrimConfig::default()
        };
        // Token budgeting is disabled - should always return Proceed
        let manager = ContextManager::new(
            "sys".into(),
            trim_config,
            Arc::new(RwLock::new(HashMap::new())),
        );

        let history = vec![uni::Message::user("hello".to_string())];
        assert_eq!(
            manager.pre_request_check(&history),
            super::PreRequestAction::Proceed
        );
    }

    #[test]
    fn pre_request_check_returns_proceed_when_budget_disabled_with_large_history() {
        let trim_config = ContextTrimConfig {
            max_tokens: 10_000, // Small window for easy testing
            ..ContextTrimConfig::default()
        };
        // Token budgeting is disabled, so should always return Proceed
        let manager = ContextManager::new(
            "sys".into(),
            trim_config,
            Arc::new(RwLock::new(HashMap::new())),
        );

        // Create history that's ~80% of max tokens
        // At 4 chars/token, 8000 tokens = 32000 chars, minus 2000 overhead = 30000 chars
        let history = vec![uni::Message::user("x".repeat(30000))];
        let action = manager.pre_request_check(&history);
        
        // With token budgeting disabled, should return Proceed even with large history
        assert_eq!(action, super::PreRequestAction::Proceed);
    }

    #[test]
    fn will_exceed_threshold_returns_false_when_under() {
        let trim_config = ContextTrimConfig {
            max_tokens: 100_000,
            ..ContextTrimConfig::default()
        };
        let manager = ContextManager::new(
            "sys".into(),
            trim_config,
            Arc::new(RwLock::new(HashMap::new())),
        );

        // Small message should be under any threshold
        let history = vec![uni::Message::user("hi".to_string())];
        assert!(!manager.will_exceed_threshold(&history, 0.75));
    }
}
