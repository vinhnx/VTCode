use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, warn};

use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};
use vtcode_core::constants::context as context_constants;
use vtcode_core::core::pruning_decisions::{PruningDecisionLedger, RetentionChoice};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::core::{ContextEfficiency, ContextPruner, MessageMetrics, RetentionDecision};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    token_budget: Arc<TokenBudgetManager>,
    token_budget_enabled: bool,
    base_system_prompt: String,
    #[allow(dead_code)]
    semantic_analyzer: Option<TreeSitterAnalyzer>,
    semantic_score_cache: Option<HashMap<u64, u8>>,
    context_pruner: ContextPruner,
    last_efficiency: Option<ContextEfficiency>,
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
            base_system_prompt,
            semantic_analyzer,
            semantic_score_cache,
            context_pruner,
            last_efficiency: None,
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

    /// Apply ContextPruner recommendations to remove low-priority messages with decision tracking
    pub(crate) fn prune_with_semantic_priority(
        &mut self,
        history: &mut Vec<uni::Message>,
        mut pruning_ledger: Option<&mut PruningDecisionLedger>,
        turn_number: usize,
    ) {
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
                    uni::MessageRole::System => "system",
                    uni::MessageRole::User => "user",
                    uni::MessageRole::Assistant => "assistant",
                    uni::MessageRole::Tool => "tool",
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
                    message_type: message_type.to_string(),
                };

                metrics_list.push((idx, metrics));
            }

            // Get decisions from ContextPruner in batch
            let metrics_only: Vec<_> = metrics_list.iter().map(|(_, m)| m.clone()).collect();
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
                                / context_constants::SEMANTIC_SCORE_SCALING_FACTOR as u32)
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
        }
    }

    #[allow(dead_code)]
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

        let avg_semantic = if history.len() > 0 {
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
                    uni::MessageRole::System => "system",
                    uni::MessageRole::User => "user",
                    uni::MessageRole::Assistant => "assistant",
                    uni::MessageRole::Tool => "tool",
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
                    message_type: message_type.to_string(),
                }
            })
            .collect()
    }
}
