use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a single context pruning decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningDecision {
    pub id: String,
    pub timestamp: u64,
    pub turn_number: usize,
    pub message_index: usize,
    pub reason: String,
    pub semantic_score: u16, // 0-1000
    pub token_count: usize,
    pub age_in_turns: usize,
    pub decision: RetentionChoice,
    pub rationale: String,
}

/// The decision made about message retention
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionChoice {
    Keep,
    Remove,
}

/// Statistics about pruning decisions for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningStatistics {
    pub total_messages_evaluated: usize,
    pub messages_kept: usize,
    pub messages_removed: usize,
    pub total_tokens_removed: usize,
    pub total_semantic_value_preserved: u32, // Sum of scores of kept messages
    pub average_semantic_score_kept: f64,
    pub average_semantic_score_removed: f64,
    pub average_message_age_removed: f64,
    pub pruning_rounds: usize,
    pub most_recent_pruning_time: Option<u64>,
}

impl Default for PruningStatistics {
    fn default() -> Self {
        Self {
            total_messages_evaluated: 0,
            messages_kept: 0,
            messages_removed: 0,
            total_tokens_removed: 0,
            total_semantic_value_preserved: 0,
            average_semantic_score_kept: 0.0,
            average_semantic_score_removed: 0.0,
            average_message_age_removed: 0.0,
            pruning_rounds: 0,
            most_recent_pruning_time: None,
        }
    }
}

/// Tracks context pruning decisions throughout a session
pub struct PruningDecisionLedger {
    decisions: Vec<PruningDecision>,
    statistics: PruningStatistics,
    session_start: u64,
}

impl PruningDecisionLedger {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            decisions: Vec::new(),
            statistics: PruningStatistics::default(),
            session_start: now,
        }
    }

    /// Record a pruning decision for a message
    pub fn record_decision(
        &mut self,
        turn_number: usize,
        message_index: usize,
        semantic_score: u16,
        token_count: usize,
        age_in_turns: usize,
        decision: RetentionChoice,
        reason: &str,
    ) -> String {
        let decision_id = format!("prune_{}_{}", self.session_start, self.decisions.len());

        let decision_record = PruningDecision {
            id: decision_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            turn_number,
            message_index,
            reason: reason.to_string(),
            semantic_score,
            token_count,
            age_in_turns,
            decision: decision.clone(),
            rationale: format!(
                "score={}, tokens={}, age={}",
                semantic_score, token_count, age_in_turns
            ),
        };

        // Update statistics
        self.statistics.total_messages_evaluated += 1;
        if decision == RetentionChoice::Keep {
            self.statistics.messages_kept += 1;
            self.statistics.total_semantic_value_preserved += semantic_score as u32;
        } else {
            self.statistics.messages_removed += 1;
            self.statistics.total_tokens_removed += token_count;
        }

        self.decisions.push(decision_record);
        decision_id
    }

    /// Record completion of a pruning round and update aggregate statistics
    pub fn record_pruning_round(&mut self) {
        self.statistics.pruning_rounds += 1;
        self.statistics.most_recent_pruning_time = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        // Recompute averages
        if self.statistics.messages_kept > 0 {
            let kept_decisions: Vec<_> = self
                .decisions
                .iter()
                .filter(|d| d.decision == RetentionChoice::Keep)
                .collect();
            let avg_score: f64 = kept_decisions
                .iter()
                .map(|d| d.semantic_score as f64)
                .sum::<f64>()
                / kept_decisions.len() as f64;
            self.statistics.average_semantic_score_kept = avg_score;
        }

        if self.statistics.messages_removed > 0 {
            let removed_decisions: Vec<_> = self
                .decisions
                .iter()
                .filter(|d| d.decision == RetentionChoice::Remove)
                .collect();
            let avg_score: f64 = removed_decisions
                .iter()
                .map(|d| d.semantic_score as f64)
                .sum::<f64>()
                / removed_decisions.len() as f64;
            let avg_age: f64 = removed_decisions
                .iter()
                .map(|d| d.age_in_turns as f64)
                .sum::<f64>()
                / removed_decisions.len() as f64;
            self.statistics.average_semantic_score_removed = avg_score;
            self.statistics.average_message_age_removed = avg_age;
        }
    }

    /// Get all pruning decisions
    pub fn get_decisions(&self) -> &[PruningDecision] {
        &self.decisions
    }

    /// Get decisions for a specific turn
    pub fn get_decisions_for_turn(&self, turn_number: usize) -> Vec<&PruningDecision> {
        self.decisions
            .iter()
            .filter(|d| d.turn_number == turn_number)
            .collect()
    }

    /// Get the N most recent pruning decisions
    pub fn recent_decisions(&self, count: usize) -> Vec<&PruningDecision> {
        self.decisions.iter().rev().take(count).rev().collect()
    }

    /// Get current statistics
    pub fn statistics(&self) -> &PruningStatistics {
        &self.statistics
    }

    /// Generate a pruning report for transparency
    pub fn generate_report(&self) -> PruningReport {
        let message_retention_ratio = if self.statistics.total_messages_evaluated > 0 {
            self.statistics.messages_kept as f64 / self.statistics.total_messages_evaluated as f64
        } else {
            1.0
        };

        let semantic_efficiency = if self.statistics.total_messages_evaluated > 0 {
            self.statistics.total_semantic_value_preserved as f64
                / self.statistics.total_messages_evaluated as f64
        } else {
            0.0
        };

        PruningReport {
            statistics: self.statistics.clone(),
            message_retention_ratio,
            semantic_efficiency,
            session_duration: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - self.session_start,
            recent_decisions: self
                .decisions
                .iter()
                .rev()
                .take(10)
                .rev()
                .cloned()
                .collect(),
        }
    }

    /// Render a compact pruning ledger for transparency
    pub fn render_ledger_brief(&self, max_entries: usize) -> String {
        let mut out = String::new();
        out.push_str("Context Pruning Ledger (most recent first)\n");

        if self.decisions.is_empty() {
            out.push_str("(no pruning decisions yet)");
            return out;
        }

        let take_n = max_entries.max(1);
        for d in self.decisions.iter().rev().take(take_n) {
            let action = match d.decision {
                RetentionChoice::Keep => "KEEP",
                RetentionChoice::Remove => "REMOVE",
            };
            out.push_str(&format!(
                "- [turn {}] msg#{} {}: score={} tokens={} age={} ({})\n",
                d.turn_number,
                d.message_index,
                action,
                d.semantic_score,
                d.token_count,
                d.age_in_turns,
                d.reason
            ));
        }

        out
    }

    /// Analyze retention patterns
    pub fn analyze_patterns(&self) -> RetentionPatterns {
        let mut patterns = RetentionPatterns::default();

        for decision in &self.decisions {
            if decision.decision == RetentionChoice::Keep {
                patterns.score_ranges_kept.record(decision.semantic_score);
                patterns.age_ranges_kept.record(decision.age_in_turns);
            } else {
                patterns
                    .score_ranges_removed
                    .record(decision.semantic_score);
                patterns.age_ranges_removed.record(decision.age_in_turns);
            }
        }

        patterns
    }
}

impl Default for PruningDecisionLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Report on pruning effectiveness and patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningReport {
    pub statistics: PruningStatistics,
    pub message_retention_ratio: f64,
    pub semantic_efficiency: f64,
    pub session_duration: u64,
    pub recent_decisions: Vec<PruningDecision>,
}

/// Analysis of retention patterns
#[derive(Debug, Clone, Default)]
pub struct RetentionPatterns {
    pub score_ranges_kept: ScoreDistribution,
    pub score_ranges_removed: ScoreDistribution,
    pub age_ranges_kept: AgeDistribution,
    pub age_ranges_removed: AgeDistribution,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreDistribution {
    pub low: usize,      // 0-250
    pub medium: usize,   // 250-500
    pub high: usize,     // 500-750
    pub critical: usize, // 750-1000
}

impl ScoreDistribution {
    fn record(&mut self, score: u16) {
        match score {
            0..=250 => self.low += 1,
            251..=500 => self.medium += 1,
            501..=750 => self.high += 1,
            _ => self.critical += 1,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgeDistribution {
    pub recent: usize,   // 0-5 turns
    pub moderate: usize, // 6-20 turns
    pub old: usize,      // 21-50 turns
    pub very_old: usize, // 50+ turns
}

impl AgeDistribution {
    fn record(&mut self, age: usize) {
        match age {
            0..=5 => self.recent += 1,
            6..=20 => self.moderate += 1,
            21..=50 => self.old += 1,
            _ => self.very_old += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_pruning_decision() {
        let mut ledger = PruningDecisionLedger::new();

        let id = ledger.record_decision(
            1,
            0,
            500,
            100,
            5,
            RetentionChoice::Keep,
            "high semantic value",
        );

        assert!(!id.is_empty());
        assert_eq!(ledger.statistics().total_messages_evaluated, 1);
        assert_eq!(ledger.statistics().messages_kept, 1);
    }

    #[test]
    fn test_record_removal() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(
            1,
            1,
            100,
            50,
            25,
            RetentionChoice::Remove,
            "low semantic value and old",
        );

        assert_eq!(ledger.statistics().messages_removed, 1);
        assert_eq!(ledger.statistics().total_tokens_removed, 50);
    }

    #[test]
    fn test_pruning_round_updates_stats() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(1, 0, 700, 100, 2, RetentionChoice::Keep, "important");
        ledger.record_decision(1, 1, 150, 50, 15, RetentionChoice::Remove, "old");

        ledger.record_pruning_round();

        assert_eq!(ledger.statistics().pruning_rounds, 1);
        assert!(ledger.statistics().average_semantic_score_kept > 0.0);
        assert!(ledger.statistics().most_recent_pruning_time.is_some());
    }

    #[test]
    fn test_get_decisions_for_turn() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(1, 0, 500, 100, 2, RetentionChoice::Keep, "msg 0");
        ledger.record_decision(1, 1, 300, 50, 5, RetentionChoice::Remove, "msg 1");
        ledger.record_decision(2, 0, 600, 120, 1, RetentionChoice::Keep, "msg 0 turn 2");

        let turn1_decisions = ledger.get_decisions_for_turn(1);
        assert_eq!(turn1_decisions.len(), 2);

        let turn2_decisions = ledger.get_decisions_for_turn(2);
        assert_eq!(turn2_decisions.len(), 1);
    }

    #[test]
    fn test_generate_report() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(1, 0, 800, 100, 1, RetentionChoice::Keep, "critical");
        ledger.record_decision(1, 1, 100, 40, 30, RetentionChoice::Remove, "old");
        ledger.record_pruning_round();

        let report = ledger.generate_report();

        assert_eq!(report.statistics.total_messages_evaluated, 2);
        assert_eq!(report.statistics.messages_kept, 1);
        assert_eq!(report.statistics.messages_removed, 1);
        assert!(report.message_retention_ratio > 0.0 && report.message_retention_ratio < 1.0);
    }

    #[test]
    fn test_render_ledger_brief() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(1, 0, 500, 100, 2, RetentionChoice::Keep, "important");

        let brief = ledger.render_ledger_brief(10);
        assert!(brief.contains("KEEP"));
        assert!(brief.contains("important"));
    }

    #[test]
    fn test_analyze_patterns() {
        let mut ledger = PruningDecisionLedger::new();

        // High-score kept messages
        ledger.record_decision(1, 0, 800, 100, 1, RetentionChoice::Keep, "kept");
        ledger.record_decision(1, 1, 900, 120, 2, RetentionChoice::Keep, "kept");

        // Low-score removed messages
        ledger.record_decision(1, 2, 100, 50, 30, RetentionChoice::Remove, "removed");
        ledger.record_decision(1, 3, 200, 40, 40, RetentionChoice::Remove, "removed");

        let patterns = ledger.analyze_patterns();

        assert_eq!(patterns.score_ranges_kept.critical, 2);
        assert_eq!(patterns.score_ranges_removed.low, 2);
        assert!(patterns.age_ranges_removed.very_old > 0);
    }

    #[test]
    fn test_retention_ratio_calculation() {
        let mut ledger = PruningDecisionLedger::new();

        // 3 kept, 1 removed = 75% retention
        ledger.record_decision(1, 0, 700, 100, 1, RetentionChoice::Keep, "");
        ledger.record_decision(1, 1, 600, 100, 1, RetentionChoice::Keep, "");
        ledger.record_decision(1, 2, 500, 100, 1, RetentionChoice::Keep, "");
        ledger.record_decision(1, 3, 100, 50, 30, RetentionChoice::Remove, "");

        let report = ledger.generate_report();
        assert!((report.message_retention_ratio - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_semantic_efficiency() {
        let mut ledger = PruningDecisionLedger::new();

        ledger.record_decision(1, 0, 1000, 100, 1, RetentionChoice::Keep, "high value");
        ledger.record_decision(1, 1, 0, 50, 50, RetentionChoice::Remove, "no value");

        let report = ledger.generate_report();
        // Efficiency = total_preserved / total_evaluated = 1000 / 2 = 500
        assert!(report.semantic_efficiency > 0.0);
    }
}
