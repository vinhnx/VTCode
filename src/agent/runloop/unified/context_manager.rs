use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{Result, bail};
use tracing::{debug, warn};

use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, TrimPhase, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};
use crate::agent::runloop::unified::incremental_system_prompt::{
    IncrementalSystemPrompt, SystemPromptConfig, SystemPromptContext,
};
use vtcode_core::constants::context as context_constants;
use vtcode_core::core::pruning_decisions::{PruningDecisionLedger, RetentionChoice};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::core::{
    ContextEfficiency, ContextPruner, MessageMetrics, MessageType, RetentionDecision,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

/// Action to take before making an LLM request based on proactive token budget analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreRequestAction {
    /// Normal operation, proceed with request
    Proceed,
    /// Light trimming recommended (at WARNING threshold)
    TrimLight,
    /// Aggressive trimming required (at ALERT threshold)
    TrimAggressive,
    /// Create checkpoint and potentially reset context (at CHECKPOINT threshold)
    Checkpoint,
}

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    token_budget: Arc<TokenBudgetManager>,
    token_budget_enabled: bool,
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

        let context_pruner = ContextPruner::new(trim_config.max_tokens);

        Self {
            trim_config,
            token_budget,
            token_budget_enabled,
            base_system_prompt: base_system_prompt.clone(),
            incremental_prompt_builder: IncrementalSystemPrompt::new(),
            semantic_analyzer,
            semantic_score_cache,
            context_pruner,
            last_efficiency: None,
            loaded_skills: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) fn trim_config(&self) -> ContextTrimConfig {
        self.trim_config
    }

    pub(crate) fn token_budget(&self) -> Arc<TokenBudgetManager> {
        Arc::clone(&self.token_budget)
    }

    #[allow(dead_code)]
    pub(crate) fn token_budget_enabled(&self) -> bool {
        self.token_budget_enabled
    }

    pub(crate) async fn reset_token_budget(&self) {
        if self.token_budget_enabled {
            self.token_budget.reset().await;
        }
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

    /// Check if the estimated token usage would exceed a given threshold
    /// Returns true if preemptive action (trimming) is recommended
    #[allow(dead_code)]
    pub(crate) fn will_exceed_threshold(&self, history: &[uni::Message], threshold: f64) -> bool {
        if !self.token_budget_enabled {
            return false;
        }

        let estimated_tokens = self.estimate_request_tokens(history);
        let max_tokens = self.trim_config.max_tokens;

        if max_tokens == 0 {
            return false;
        }

        let estimated_ratio = estimated_tokens as f64 / max_tokens as f64;
        estimated_ratio >= threshold
    }

    /// Pre-request check that returns recommended action before making an LLM request.
    /// This enables proactive trimming BEFORE tokens are consumed rather than reacting afterwards.
    pub(crate) fn pre_request_check(&self, history: &[uni::Message]) -> PreRequestAction {
        use vtcode_core::core::token_constants::{
            THRESHOLD_ALERT, THRESHOLD_CHECKPOINT, THRESHOLD_WARNING,
        };

        if !self.token_budget_enabled {
            return PreRequestAction::Proceed;
        }

        let estimated_tokens = self.estimate_request_tokens(history);
        let max_tokens = self.trim_config.max_tokens;

        if max_tokens == 0 {
            return PreRequestAction::Proceed;
        }

        let estimated_ratio = estimated_tokens as f64 / max_tokens as f64;

        if estimated_ratio >= THRESHOLD_CHECKPOINT {
            PreRequestAction::Checkpoint
        } else if estimated_ratio >= THRESHOLD_ALERT {
            PreRequestAction::TrimAggressive
        } else if estimated_ratio >= THRESHOLD_WARNING {
            PreRequestAction::TrimLight
        } else {
            PreRequestAction::Proceed
        }
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
    ) -> usize {
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
                            RetentionDecision::Summarizable => RetentionChoice::Keep, // Keep for now
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

            before_len.saturating_sub(history.len())
        } else {
            0
        }
    }

    #[allow(dead_code)]
    pub(crate) fn aggressive_trim(&self, history: &mut Vec<uni::Message>) -> usize {
        apply_aggressive_trim_unified(history, self.trim_config)
    }

    /// Adaptive trim based on budget thresholds; returns structured outcome
    pub(crate) async fn adaptive_trim(
        &mut self,
        history: &mut Vec<uni::Message>,
        pruning_ledger: Option<&mut PruningDecisionLedger>,
        turn_number: usize,
    ) -> Result<ContextTrimOutcome> {
        if !self.token_budget_enabled {
            return Ok(ContextTrimOutcome::default());
        }

        let mut pruning_ledger = pruning_ledger;
        let usage = self.token_budget.usage_ratio().await;
        let mut outcome = ContextTrimOutcome::default();

        // Alert/compact: semantic prune first, then enforce window
        if usage >= vtcode_core::core::token_constants::THRESHOLD_ALERT {
            let before_len = history.len();
            self.prune_with_semantic_priority(history, pruning_ledger.as_deref_mut(), turn_number);
            let window_outcome = self.enforce_context_window(history);
            let after_semantic = before_len.saturating_sub(history.len());
            let total_removed = after_semantic.saturating_add(window_outcome.removed_messages);
            outcome.removed_messages = total_removed;
            outcome.phase = if total_removed > 0 {
                TrimPhase::AlertSemantic
            } else {
                window_outcome.phase
            };
        } else if usage >= vtcode_core::core::token_constants::THRESHOLD_WARNING {
            // Warning: light prune of tool responses
            let removed = self.prune_tool_responses(history);
            outcome.removed_messages = removed;
            if removed > 0 {
                outcome.phase = TrimPhase::WarningToolPrune;
            }
        }

        // Track efficiency after any action
        if outcome.is_trimmed() {
            self.record_efficiency_after_trim(history, &outcome);
            if let Some(ledger) = pruning_ledger {
                ledger.record_pruning_round();
            }
        }

        Ok(outcome)
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
            enable_token_tracking: self.token_budget_enabled,
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
            token_usage_ratio: if self.token_budget_enabled {
                self.token_budget.usage_ratio().await
            } else {
                0.0
            },
            current_plan,
            full_auto,
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
    use vtcode_core::core::token_budget::TokenBudgetConfig;
    use vtcode_core::core::token_constants::{THRESHOLD_ALERT, THRESHOLD_WARNING};

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
        let budget_cfg = TokenBudgetConfig::for_model("test", trim_config.max_tokens);
        let budget = Arc::new(TokenBudgetManager::new(budget_cfg));
        let mut manager = ContextManager::new("sys".into(), trim_config, Arc::clone(&budget), true);

        let mut history = make_tool_history(8);
        let mut stats = vtcode_core::core::token_budget::TokenUsageStats::new();
        stats.total_tokens =
            (THRESHOLD_WARNING * trim_config.max_tokens as f64).ceil() as usize + 5;
        budget.restore_stats(stats).await;

        let outcome = manager
            .adaptive_trim(&mut history, None, 1)
            .await
            .expect("adaptive trim should succeed");

        assert_eq!(outcome.phase, TrimPhase::WarningToolPrune);
        assert!(outcome.removed_messages > 0);
    }

    #[tokio::test]
    async fn adaptive_trim_compacts_at_alert_threshold() {
        let trim_config = ContextTrimConfig {
            max_tokens: 40,
            ..ContextTrimConfig::default()
        };
        let budget_cfg = TokenBudgetConfig::for_model("test", trim_config.max_tokens);
        let budget = Arc::new(TokenBudgetManager::new(budget_cfg));
        let mut manager = ContextManager::new("sys".into(), trim_config, Arc::clone(&budget), true);

        // Build a history that exceeds the context window
        let mut history: Vec<uni::Message> = (0..10)
            .map(|_| uni::Message::assistant("x".repeat(200)))
            .collect();

        let mut stats = vtcode_core::core::token_budget::TokenUsageStats::new();
        stats.total_tokens = (THRESHOLD_ALERT * trim_config.max_tokens as f64).ceil() as usize + 10;
        budget.restore_stats(stats).await;

        let outcome = manager
            .adaptive_trim(&mut history, None, 2)
            .await
            .expect("adaptive trim should succeed");

        assert!(outcome.is_trimmed());
        assert!(
            matches!(
                outcome.phase,
                TrimPhase::AlertSemantic | TrimPhase::WindowEnforced
            ),
            "unexpected phase: {:?}",
            outcome.phase
        );
    }

    #[test]
    fn pre_request_check_returns_proceed_when_budget_disabled() {
        let trim_config = ContextTrimConfig {
            max_tokens: 100_000,
            ..ContextTrimConfig::default()
        };
        let budget_cfg = TokenBudgetConfig::for_model("test", trim_config.max_tokens);
        let budget = Arc::new(TokenBudgetManager::new(budget_cfg));
        // Note: token_budget_enabled = false
        let manager = ContextManager::new("sys".into(), trim_config, budget, false);

        let history = vec![uni::Message::user("hello".to_string())];
        assert_eq!(
            manager.pre_request_check(&history),
            super::PreRequestAction::Proceed
        );
    }

    #[test]
    fn pre_request_check_returns_trim_at_warning_threshold() {
        let trim_config = ContextTrimConfig {
            max_tokens: 10_000, // Small window for easy testing
            ..ContextTrimConfig::default()
        };
        let budget_cfg = TokenBudgetConfig::for_model("test", trim_config.max_tokens);
        let budget = Arc::new(TokenBudgetManager::new(budget_cfg));
        let manager = ContextManager::new("sys".into(), trim_config, budget, true);

        // Create history that's ~80% of max tokens (above WARNING threshold of 0.75)
        // At 4 chars/token, 8000 tokens = 32000 chars, minus 2000 overhead = 30000 chars
        let history = vec![uni::Message::user("x".repeat(30000))];
        let action = manager.pre_request_check(&history);
        assert!(matches!(
            action,
            super::PreRequestAction::TrimLight | super::PreRequestAction::TrimAggressive
        ));
    }

    #[test]
    fn will_exceed_threshold_returns_false_when_under() {
        let trim_config = ContextTrimConfig {
            max_tokens: 100_000,
            ..ContextTrimConfig::default()
        };
        let budget_cfg = TokenBudgetConfig::for_model("test", trim_config.max_tokens);
        let budget = Arc::new(TokenBudgetManager::new(budget_cfg));
        let manager = ContextManager::new("sys".into(), trim_config, budget, true);

        // Small message should be under any threshold
        let history = vec![uni::Message::user("hi".to_string())];
        assert!(!manager.will_exceed_threshold(&history, 0.75));
    }
}
