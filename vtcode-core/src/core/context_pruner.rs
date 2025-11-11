/// Advanced context pruning based on token cost and semantic importance
///
/// This module implements intelligent message retention within context window
/// by combining token cost analysis with semantic importance scoring.
/// Messages are evaluated on both their token expense and semantic value,
/// allowing preservation of high-semantic-value messages even if older.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Semantic importance score for a message (0-1000 scale)
/// Higher values indicate more important for context retention
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticScore(u32);

impl SemanticScore {
    /// Create score from 0-1000 range
    pub fn new(value: u32) -> Self {
        SemanticScore(value.min(1000))
    }

    /// Get score as f64 in 0-1 range
    pub fn as_ratio(&self) -> f64 {
        self.0 as f64 / 1000.0
    }

    /// System message (high importance)
    pub fn system_message() -> Self {
        SemanticScore(950)
    }

    /// User query (high importance)
    pub fn user_query() -> Self {
        SemanticScore(850)
    }

    /// Tool response (medium importance, depends on freshness)
    pub fn tool_response() -> Self {
        SemanticScore(600)
    }

    /// Assistant response (medium importance)
    pub fn assistant_response() -> Self {
        SemanticScore(500)
    }

    /// Context/filler message (lower importance)
    pub fn context_message() -> Self {
        SemanticScore(300)
    }
}

/// Message retention metrics for pruning decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetrics {
    /// Message index in conversation
    pub index: usize,
    /// Total tokens in this message
    pub token_count: usize,
    /// Semantic importance (0-1000)
    pub semantic_score: u32,
    /// Age in turns (0 = most recent)
    pub age_in_turns: u32,
    /// Message type: "system", "user", "assistant", "tool"
    pub message_type: String,
}

/// Retention decision for a message
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionDecision {
    /// Always keep (system prompt, recent important messages)
    Keep,
    /// Remove to make room
    Remove,
    /// Keep but could be summarized
    Summarizable,
}

/// Token-aware context pruning strategy
#[derive(Debug, Clone)]
pub struct ContextPruner {
    /// Maximum tokens to keep in context
    pub max_tokens: usize,
    /// Threshold for semantic importance (0-1000)
    pub semantic_threshold: u32,
    /// Bonus for recent messages (turns)
    pub recency_bonus_per_turn: u32,
    /// Minimum semantic value to always keep
    pub min_keep_semantic: u32,
}

impl ContextPruner {
    /// Create new context pruner
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            semantic_threshold: 300,
            recency_bonus_per_turn: 5,
            min_keep_semantic: 400,
        }
    }

    /// Decide which messages to keep based on token budget and semantic value
    pub fn prune_messages(&self, messages: &[MessageMetrics]) -> HashMap<usize, RetentionDecision> {
        let mut decisions = HashMap::new();
        let mut total_tokens = 0;

        // Calculate adjusted semantic scores with recency bonus
        let mut scored_messages: Vec<_> = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let recency_bonus =
                    msg.age_in_turns.saturating_mul(self.recency_bonus_per_turn) as u32;
                let adjusted_score = (msg.semantic_score + recency_bonus).min(1000);
                (i, msg, adjusted_score)
            })
            .collect();

        // Always keep system and very recent important messages
        for (i, msg, score) in &scored_messages {
            if msg.message_type == "system" || *score >= self.min_keep_semantic {
                decisions.insert(*i, RetentionDecision::Keep);
                total_tokens += msg.token_count;
            }
        }

        if total_tokens >= self.max_tokens {
            return decisions; // Already at limit with required messages
        }

        // Sort remaining by score (descending) for greedy selection
        scored_messages.sort_by(|a, b| {
            let a_score = a.2;
            let b_score = b.2;
            b_score.cmp(&a_score)
        });

        // Greedily add messages by semantic value
        for (i, msg, _score) in &scored_messages {
            if decisions.contains_key(i) {
                continue; // Already decided
            }

            if total_tokens + msg.token_count <= self.max_tokens {
                decisions.insert(*i, RetentionDecision::Keep);
                total_tokens += msg.token_count;
            } else {
                decisions.insert(*i, RetentionDecision::Remove);
            }
        }

        // Fill any remaining space with low-value messages (for context)
        for (i, msg, _score) in &scored_messages {
            if !decisions.contains_key(i) && total_tokens + msg.token_count <= self.max_tokens {
                decisions.insert(*i, RetentionDecision::Summarizable);
                total_tokens += msg.token_count;
            }
        }

        decisions
    }

    /// Calculate priority score for message retention
    pub fn calculate_priority(
        &self,
        semantic_score: u32,
        token_count: usize,
        age_in_turns: u32,
    ) -> f64 {
        // Token efficiency: lower tokens = higher priority
        let token_efficiency = 1.0 / (1.0 + (token_count as f64 / 500.0));

        // Semantic value: normalized to 0-1
        let semantic_value = (semantic_score as f64) / 1000.0;

        // Recency: boost for recent messages
        let recency_bonus = 1.0 / (1.0 + (age_in_turns as f64 / 5.0));

        // Combined score: prioritize semantic value and efficiency
        (semantic_value * 0.6 + token_efficiency * 0.3 + recency_bonus * 0.1).min(1.0)
    }

    /// Analyze context window efficiency
    pub fn analyze_efficiency(&self, messages: &[MessageMetrics]) -> ContextEfficiency {
        let total_tokens: usize = messages.iter().map(|m| m.token_count).sum();
        let total_semantic: u32 = messages.iter().map(|m| m.semantic_score).sum();
        let avg_semantic = if messages.is_empty() {
            0
        } else {
            total_semantic / messages.len() as u32
        };

        let semantic_value_per_token = if total_tokens > 0 {
            (total_semantic as f64) / (total_tokens as f64) * 1000.0
        } else {
            0.0
        };

        let utilization = ((total_tokens as f64) / (self.max_tokens as f64)) * 100.0;

        ContextEfficiency {
            total_tokens,
            total_messages: messages.len(),
            avg_semantic_score: avg_semantic,
            semantic_value_per_token,
            context_utilization_percent: utilization.min(100.0),
        }
    }

    /// Format efficiency report
    pub fn format_efficiency_report(&self, messages: &[MessageMetrics]) -> String {
        let efficiency = self.analyze_efficiency(messages);
        let mut report = String::new();
        report.push_str("📊 Context Window Efficiency\n");
        report.push_str(&format!(
            "  Tokens Used: {}/{} ({:.1}%)\n",
            efficiency.total_tokens, self.max_tokens, efficiency.context_utilization_percent
        ));
        report.push_str(&format!(
            "  Messages: {} total\n",
            efficiency.total_messages
        ));
        report.push_str(&format!(
            "  Avg Semantic Score: {}/1000\n",
            efficiency.avg_semantic_score
        ));
        report.push_str(&format!(
            "  Semantic Value/Token: {:.2}\n",
            efficiency.semantic_value_per_token
        ));

        report
    }
}

/// Context efficiency metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEfficiency {
    /// Total tokens used
    pub total_tokens: usize,
    /// Number of messages
    pub total_messages: usize,
    /// Average semantic score
    pub avg_semantic_score: u32,
    /// Semantic value per token ratio
    pub semantic_value_per_token: f64,
    /// Context utilization percentage
    pub context_utilization_percent: f64,
}

impl Default for ContextPruner {
    fn default() -> Self {
        Self::new(8192)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_messages() -> Vec<MessageMetrics> {
        vec![
            MessageMetrics {
                index: 0,
                token_count: 100,
                semantic_score: 950,
                age_in_turns: 10,
                message_type: "system".to_string(),
            },
            MessageMetrics {
                index: 1,
                token_count: 500,
                semantic_score: 850,
                age_in_turns: 9,
                message_type: "user".to_string(),
            },
            MessageMetrics {
                index: 2,
                token_count: 200,
                semantic_score: 500,
                age_in_turns: 8,
                message_type: "assistant".to_string(),
            },
            MessageMetrics {
                index: 3,
                token_count: 300,
                semantic_score: 600,
                age_in_turns: 2,
                message_type: "tool".to_string(),
            },
            MessageMetrics {
                index: 4,
                token_count: 150,
                semantic_score: 800,
                age_in_turns: 0,
                message_type: "user".to_string(),
            },
        ]
    }

    #[test]
    fn test_creates_pruner() {
        let pruner = ContextPruner::new(4096);
        assert_eq!(pruner.max_tokens, 4096);
    }

    #[test]
    fn test_keeps_system_messages() {
        let pruner = ContextPruner::new(1000);
        let messages = create_test_messages();

        let decisions = pruner.prune_messages(&messages);
        // System message should always be kept
        assert_eq!(decisions[&0], RetentionDecision::Keep);
    }

    #[test]
    fn test_respects_token_budget() {
        let pruner = ContextPruner::new(500);
        let messages = create_test_messages();

        let decisions = pruner.prune_messages(&messages);
        let kept_tokens: usize = messages
            .iter()
            .enumerate()
            .filter(|(i, _)| decisions.get(i) == Some(&RetentionDecision::Keep))
            .map(|(_, m)| m.token_count)
            .sum();

        assert!(kept_tokens <= pruner.max_tokens);
    }

    #[test]
    fn test_prioritizes_semantic_value() {
        let pruner = ContextPruner::new(2000);
        let messages = create_test_messages();

        let decisions = pruner.prune_messages(&messages);
        // High semantic score messages should be kept
        assert_eq!(decisions[&1], RetentionDecision::Keep); // user query (850)
    }

    #[test]
    fn test_calculates_priority() {
        let pruner = ContextPruner::new(4096);
        let priority_high = pruner.calculate_priority(900, 100, 0);
        let priority_low = pruner.calculate_priority(200, 1000, 10);

        assert!(priority_high > priority_low);
    }

    #[test]
    fn test_analyzes_efficiency() {
        let pruner = ContextPruner::new(8192);
        let messages = create_test_messages();

        let efficiency = pruner.analyze_efficiency(&messages);
        assert!(efficiency.total_tokens > 0);
        assert_eq!(efficiency.total_messages, 5);
    }

    #[test]
    fn test_semantic_score_bounds() {
        let score = SemanticScore::new(2000);
        assert_eq!(score.0, 1000); // Clamped to max
    }

    #[test]
    fn test_semantic_score_as_ratio() {
        let score = SemanticScore::new(500);
        assert!((score.as_ratio() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_prune_with_high_token_budget() {
        let pruner = ContextPruner::new(10000);
        let messages = create_test_messages();

        let decisions = pruner.prune_messages(&messages);
        // With generous budget, should keep most messages
        let keep_count = decisions
            .values()
            .filter(|&&d| d == RetentionDecision::Keep)
            .count();
        assert!(keep_count >= 3);
    }

    #[test]
    fn test_format_efficiency_report() {
        let pruner = ContextPruner::new(4096);
        let messages = create_test_messages();

        let report = pruner.format_efficiency_report(&messages);
        assert!(report.contains("Context Window Efficiency"));
        assert!(report.contains("Tokens Used"));
    }
}
