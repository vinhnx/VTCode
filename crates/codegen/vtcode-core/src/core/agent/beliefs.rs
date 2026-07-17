//! Agent beliefs: a structured store of what the agent "knows" about the user,
//! system, task, and conversation.
//!
//! Following the "state as a first-class citizen" principle (Hitchhiker's Guide
//! to Agentic AI, Section 18.6.3), beliefs are part of the agent's explicit
//! state rather than an implicit property of the conversation history.
//!
//! Each belief carries a confidence score, source attribution, and reinforcement
//! count, enabling the agent to distinguish speculative inferences from
//! confirmed facts.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// All beliefs the agent currently holds, categorized by domain.
///
/// Beliefs are bounded by `max_beliefs` to prevent unbounded growth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBeliefs {
    /// Beliefs about the user (preferences, identity, working style).
    pub about_user: Vec<Belief>,
    /// Beliefs about the system (tool capabilities, project structure, limitations).
    pub about_system: Vec<Belief>,
    /// Beliefs about the current task (what has been done, what remains).
    pub about_task: Vec<Belief>,
    /// Beliefs about the conversation state (context usage, important decisions).
    pub about_conversation: Vec<Belief>,
    /// Maximum total beliefs across all categories.
    #[serde(skip)]
    pub max_beliefs: usize,
}

impl Default for AgentBeliefs {
    fn default() -> Self {
        Self {
            about_user: Vec::with_capacity(8),
            about_system: Vec::with_capacity(8),
            about_task: Vec::with_capacity(8),
            about_conversation: Vec::with_capacity(8),
            max_beliefs: 64,
        }
    }
}

impl AgentBeliefs {
    /// Add a belief, or reinforce an existing one with the same (or similar) statement.
    pub fn add_or_reinforce(&mut self, statement: &str, confidence: f64, source: BeliefSource) {
        let normalized = statement.trim().to_lowercase();
        let now = unix_timestamp();
        let category = source.default_category();

        // Check total capacity before taking the mutable borrow
        let needs_prune = {
            let total = self.about_user.len()
                + self.about_system.len()
                + self.about_task.len()
                + self.about_conversation.len();
            total >= self.max_beliefs
        };
        if needs_prune {
            self.prune_low_confidence(0.1);
        }

        // Select the target category, then try to reinforce or push new
        match category {
            BeliefCategory::User => {
                Self::reinforce_or_push(
                    &mut self.about_user,
                    &normalized,
                    statement,
                    confidence,
                    source,
                    now,
                );
            }
            BeliefCategory::System => {
                Self::reinforce_or_push(
                    &mut self.about_system,
                    &normalized,
                    statement,
                    confidence,
                    source,
                    now,
                );
            }
            BeliefCategory::Task => {
                Self::reinforce_or_push(
                    &mut self.about_task,
                    &normalized,
                    statement,
                    confidence,
                    source,
                    now,
                );
            }
            BeliefCategory::Conversation => {
                Self::reinforce_or_push(
                    &mut self.about_conversation,
                    &normalized,
                    statement,
                    confidence,
                    source,
                    now,
                );
            }
        }
    }

    /// Reinforce an existing matching belief in `list`, or push a new one.
    fn reinforce_or_push(
        list: &mut Vec<Belief>,
        normalized: &str,
        statement: &str,
        confidence: f64,
        source: BeliefSource,
        now: u64,
    ) {
        // Try to find an existing belief with a similar statement
        if let Some(existing) = list.iter_mut().find(|b| {
            let b_norm = b.statement.trim().to_lowercase();
            b_norm == *normalized || b_norm.contains(normalized) || normalized.contains(&b_norm)
        }) {
            existing.confidence = existing.confidence.max(confidence);
            existing.reinforcement_count = existing.reinforcement_count.saturating_add(1);
            existing.last_reinforced_at = now;
            return;
        }

        list.push(Belief {
            id: format!("belief_{now}_{}", list.len()),
            statement: statement.trim().to_string(),
            confidence: confidence.clamp(0.0, 1.0),
            source,
            created_at: now,
            last_reinforced_at: now,
            reinforcement_count: 1,
        });
    }

    /// Return beliefs with confidence at or above the threshold.
    pub fn above_confidence(&self, threshold: f64) -> Vec<&Belief> {
        let mut result = Vec::new();
        for belief in self
            .about_user
            .iter()
            .chain(self.about_system.iter())
            .chain(self.about_task.iter())
            .chain(self.about_conversation.iter())
        {
            if belief.confidence >= threshold {
                result.push(belief);
            }
        }
        result
    }

    /// Remove beliefs below the confidence threshold, unless recently reinforced.
    pub fn prune_low_confidence(&mut self, threshold: f64) {
        let now = unix_timestamp();
        let retain = |belief: &Belief| -> bool {
            belief.confidence >= threshold
                || belief.reinforcement_count >= 3
                || now.saturating_sub(belief.last_reinforced_at) < 3600 // reinforced within 1 hour
        };
        self.about_user.retain(retain);
        self.about_system.retain(retain);
        self.about_task.retain(retain);
        self.about_conversation.retain(retain);
    }

    /// Format all beliefs above a confidence threshold as a markdown string for
    /// injection into the system prompt.
    pub fn format_for_prompt(&self, threshold: f64) -> String {
        let beliefs = self.above_confidence(threshold);
        if beliefs.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Agent Beliefs".to_string()];
        for belief in &beliefs {
            // Skip tool result beliefs in prompt — they're transient
            if matches!(&belief.source, BeliefSource::ToolResult { .. }) {
                continue;
            }
            let source_str = match &belief.source {
                BeliefSource::UserStatement => "user said",
                BeliefSource::ToolResult { .. } => "tool result",
                BeliefSource::Inference { .. } => "inferred",
                BeliefSource::SystemPrompt => "configured",
                BeliefSource::PersistentMemory => "previous session",
            };
            lines.push(format!(
                "- [{}] {} (confidence: {:.1}, reinforced: {})",
                source_str, belief.statement, belief.confidence, belief.reinforcement_count
            ));
        }
        if lines.len() == 1 {
            return String::new();
        }
        lines.join("\n")
    }
}

/// A single belief held by the agent, with provenance and confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    /// Unique identifier for this belief instance.
    pub id: String,
    /// The belief statement (e.g., "the user prefers Python for scripting").
    pub statement: String,
    /// Confidence in [0.0, 1.0]: 0.0 = speculative, 1.0 = confirmed.
    pub confidence: f64,
    /// Origin of this belief.
    pub source: BeliefSource,
    /// Unix timestamp (seconds) when the belief was first created.
    pub created_at: u64,
    /// Unix timestamp (seconds) of the last reinforcement.
    pub last_reinforced_at: u64,
    /// How many times this belief has been corroborated.
    pub reinforcement_count: u64,
}

/// Origin category for a belief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefSource {
    /// Directly stated by the user.
    UserStatement,
    /// Derived from a tool execution result.
    ToolResult {
        /// Name of the tool that produced the result.
        tool: String,
    },
    /// Inferred by the agent from evidence.
    Inference {
        /// Description of the reasoning that produced this belief.
        by: String,
    },
    /// Established by the system prompt or configuration.
    SystemPrompt,
    /// Loaded from persistent memory (cross-session).
    PersistentMemory,
}

impl BeliefSource {
    /// Map a belief source to its default category.
    pub fn default_category(&self) -> BeliefCategory {
        match self {
            BeliefSource::UserStatement => BeliefCategory::User,
            BeliefSource::ToolResult { .. } => BeliefCategory::System,
            BeliefSource::Inference { .. } => BeliefCategory::Task,
            BeliefSource::SystemPrompt => BeliefCategory::System,
            BeliefSource::PersistentMemory => BeliefCategory::User,
        }
    }
}

/// High-level category for a belief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeliefCategory {
    /// Beliefs about the user (preferences, identity, working style).
    User,
    /// Beliefs about the system (tools, project structure, limitations).
    System,
    /// Beliefs about the current task (what has been done, what remains).
    Task,
    /// Beliefs about the conversation state (context usage, important decisions).
    Conversation,
}

fn unix_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_belief() {
        let mut beliefs = AgentBeliefs::default();
        beliefs.add_or_reinforce("user prefers Python", 0.8, BeliefSource::UserStatement);
        assert_eq!(beliefs.about_user.len(), 1);
        assert_eq!(beliefs.above_confidence(0.5).len(), 1);
    }

    #[test]
    fn test_reinforce_belief() {
        let mut beliefs = AgentBeliefs::default();
        beliefs.add_or_reinforce("user prefers Python", 0.6, BeliefSource::UserStatement);
        beliefs.add_or_reinforce("user prefers Python", 0.9, BeliefSource::UserStatement);
        assert_eq!(beliefs.about_user.len(), 1);
        assert!((beliefs.about_user[0].confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(beliefs.about_user[0].reinforcement_count, 2);
    }

    #[test]
    fn test_prune_low_confidence() {
        let mut beliefs = AgentBeliefs::default();
        beliefs.add_or_reinforce("high confidence", 0.9, BeliefSource::UserStatement);
        // Add a low-confidence belief and manually set it as old so prune removes it
        beliefs.add_or_reinforce(
            "low confidence",
            0.05,
            BeliefSource::ToolResult { tool: "ls".to_string() },
        );
        if let Some(low) = beliefs.about_system.last_mut() {
            low.last_reinforced_at = 1; // Unix epoch — very old
        }
        assert_eq!(beliefs.above_confidence(0.0).len(), 2);
        beliefs.prune_low_confidence(0.5);
        let remaining = beliefs.above_confidence(0.0);
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].statement.contains("high"));
    }

    #[test]
    fn test_format_for_prompt() {
        let mut beliefs = AgentBeliefs::default();
        beliefs.add_or_reinforce("user prefers Python", 0.8, BeliefSource::UserStatement);
        let formatted = beliefs.format_for_prompt(0.5);
        assert!(formatted.contains("user prefers Python"));
        assert!(formatted.contains("[user said]"));
    }

    #[test]
    fn test_belief_serde_roundtrip() {
        let belief = Belief {
            id: "test_1".to_string(),
            statement: "the answer is 42".to_string(),
            confidence: 0.95,
            source: BeliefSource::Inference { by: "test".to_string() },
            created_at: 1000,
            last_reinforced_at: 1000,
            reinforcement_count: 1,
        };
        let json = serde_json::to_string(&belief).unwrap();
        let deserialized: Belief = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.statement, "the answer is 42");
        assert!((deserialized.confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_belief_source_default_category() {
        assert_eq!(BeliefSource::UserStatement.default_category(), BeliefCategory::User);
        assert_eq!(
            BeliefSource::ToolResult { tool: "read_file".to_string() }.default_category(),
            BeliefCategory::System
        );
        assert_eq!(BeliefSource::SystemPrompt.default_category(), BeliefCategory::System);
        assert_eq!(BeliefSource::PersistentMemory.default_category(), BeliefCategory::User);
    }
}
