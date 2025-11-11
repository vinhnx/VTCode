use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, warn};

use crate::agent::runloop::context::{
    ContextTrimConfig, ContextTrimOutcome, apply_aggressive_trim_unified,
    enforce_unified_context_window, prune_unified_tool_responses,
};
use vtcode_core::core::token_budget::{ContextComponent, TokenBudgetManager};
use vtcode_core::core::{ContextPruner, ContextEfficiency, MessageMetrics, RetentionDecision};
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tree_sitter::TreeSitterAnalyzer;

pub(crate) struct ContextManager {
    trim_config: ContextTrimConfig,
    token_budget: Arc<TokenBudgetManager>,
    token_budget_enabled: bool,
    base_system_prompt: String,
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

    pub(crate) fn prune_tool_responses(&self, history: &mut Vec<uni::Message>) -> usize {
        prune_unified_tool_responses(history, &self.trim_config)
    }

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

    /// Apply ContextPruner recommendations to remove low-priority messages
    pub(crate) fn prune_with_semantic_priority(&mut self, history: &mut Vec<uni::Message>) {
        if self.trim_config.semantic_compression {
            let mut indices_to_remove = Vec::new();

            for (idx, msg) in history.iter().enumerate() {
                // Preserve system message
                if matches!(msg.role, uni::MessageRole::System) {
                    continue;
                }

                // Use ContextPruner to get retention decision
                let semantic_score = if let Some(cache) = &self.semantic_score_cache {
                    cache.get(&(idx as u64)).copied().unwrap_or(500) as u32 * 4
                } else {
                    500
                };

                let message_type = match &msg.role {
                    uni::MessageRole::System => "system",
                    uni::MessageRole::User => "user",
                    uni::MessageRole::Assistant => "assistant",
                    uni::MessageRole::Tool => "tool",
                };

                let token_count = match &msg.content {
                    uni::MessageContent::Text(text) => (text.len() / 4).max(1),
                    uni::MessageContent::Parts(_) => 256,
                };

                let metrics = MessageMetrics {
                    index: idx,
                    token_count,
                    semantic_score,
                    age_in_turns: (history.len().saturating_sub(idx + 1)) as u32,
                    message_type: message_type.to_string(),
                };

                let decision = self.context_pruner.should_keep_message(&metrics);
                if matches!(decision, RetentionDecision::Remove) {
                    indices_to_remove.push(idx);
                }
            }

            // Remove in reverse order to preserve indices
            for idx in indices_to_remove.iter().rev() {
                history.remove(*idx);
                debug!("Pruned message at index {} for low semantic priority", idx);
            }
        }
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

    pub(crate) fn last_efficiency(&self) -> Option<&ContextEfficiency> {
        self.last_efficiency.as_ref()
    }

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
    fn record_efficiency_after_trim(&mut self, history: &[uni::Message], outcome: &ContextTrimOutcome) {
        let total_tokens = history.iter()
            .map(|msg| {
                match &msg.content {
                    uni::MessageContent::Text(text) => (text.len() / 4).max(1),
                    uni::MessageContent::Parts(_) => 256,
                }
            })
            .sum::<usize>();

        let total_semantic_value: u32 = if let Some(cache) = &self.semantic_score_cache {
            cache.values().map(|&v| v as u32 * 4).sum()
        } else {
            history.iter()
                .map(|msg| {
                    if matches!(msg.role, uni::MessageRole::System) { 950 }
                    else if matches!(msg.role, uni::MessageRole::User) { 850 }
                    else { 500 }
                })
                .sum()
        };

        let context_utilization = if self.trim_config.max_tokens > 0 {
            (total_tokens as f64 / self.trim_config.max_tokens as f64) * 100.0
        } else {
            0.0
        };

        let semantic_per_token = if total_tokens > 0 {
            total_semantic_value as f64 / total_tokens as f64
        } else {
            0.0
        };

        self.last_efficiency = Some(ContextEfficiency {
            context_utilization_percent: context_utilization,
            total_tokens,
            total_messages: history.len(),
            semantic_value_per_token: semantic_per_token,
            messages_removed: outcome.messages_removed,
            tokens_recovered: outcome.tokens_freed,
        });
    }

    /// Convert LLM messages to MessageMetrics for ContextPruner analysis
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
                    semantic_scores[idx] as u32 * 4 // Scale from 0-255 to 0-1000
                } else {
                    500
                };

                let age_in_turns = (now.saturating_sub(idx + 1)) as u32;

                // Approximate tokens: ~4 characters per token
                let content_len = match &msg.content {
                    uni::MessageContent::Text(text) => text.len(),
                    uni::MessageContent::Parts(_) => 256, // Rough estimate for parts
                };
                let token_count = (content_len / 4).max(1);

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
