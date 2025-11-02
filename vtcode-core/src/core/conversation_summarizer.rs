use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Represents a conversation summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub timestamp: u64,
    pub session_duration_seconds: u64,
    pub total_turns: usize,
    pub key_decisions: Vec<KeyDecision>,
    pub completed_tasks: Vec<TaskSummary>,
    pub error_patterns: Vec<ErrorPattern>,
    pub context_recommendations: Vec<String>,
    pub summary_text: String,
    pub compression_ratio: f64,
    pub confidence_score: f64,
}

/// A key decision made during the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDecision {
    pub turn_number: usize,
    pub decision_type: DecisionType,
    pub description: String,
    pub rationale: String,
    pub outcome: Option<String>,
    pub importance_score: f64,
}

/// Type of decision that was made
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionType {
    ToolExecution,
    ResponseGeneration,
    ContextCompression,
    ErrorRecovery,
    WorkflowChange,
}

impl std::fmt::Display for DecisionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self {
            DecisionType::ToolExecution => "Tool Execution",
            DecisionType::ResponseGeneration => "Response Generation",
            DecisionType::ContextCompression => "Context Compression",
            DecisionType::ErrorRecovery => "Error Recovery",
            DecisionType::WorkflowChange => "Workflow Change",
        };
        write!(f, "{}", description)
    }
}

/// Summary of a completed task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_type: String,
    pub description: String,
    pub success: bool,
    pub duration_seconds: Option<u64>,
    pub tools_used: Vec<String>,
}

/// Pattern of errors that occurred
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    pub error_type: String,
    pub frequency: usize,
    pub description: String,
    pub recommended_solution: String,
}

/// Conversation summarizer for long-running sessions
pub struct ConversationSummarizer {
    enabled: bool,
    summaries: Vec<ConversationSummary>,
    summarization_threshold: usize, // Minimum conversation length to trigger summarization
    max_summary_length: usize,
    compression_target_ratio: f64,
    context_trigger_percent: u8,
    error_trigger_percent: u8,
    min_turn_gap: usize,
    last_summary_turn: Option<usize>,
}

impl ConversationSummarizer {
    pub fn new() -> Self {
        let enabled = std::env::var("VTCODE_AUTO_SUMMARY_ENABLED")
            .map(|value| {
                matches!(
                    value.trim().to_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let summarization_threshold = std::env::var("VTCODE_AUTO_SUMMARY_TURN_THRESHOLD")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(20);

        let context_trigger_percent = std::env::var("VTCODE_AUTO_SUMMARY_CONTEXT_PERCENT")
            .ok()
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|value| (1..=100).contains(value))
            .unwrap_or(85); // Trigger at 85% token usage to prevent blocking

        let error_trigger_percent = std::env::var("VTCODE_AUTO_SUMMARY_ERROR_PERCENT")
            .ok()
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|value| (1..=100).contains(value))
            .unwrap_or(60);

        let min_turn_gap = std::env::var("VTCODE_AUTO_SUMMARY_MIN_TURN_GAP")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(5);

        Self {
            enabled,
            summaries: Vec::new(),
            summarization_threshold,
            max_summary_length: 2000,      // Maximum characters in summary
            compression_target_ratio: 0.3, // Target 30% of original length
            context_trigger_percent,
            error_trigger_percent,
            min_turn_gap,
            last_summary_turn: None,
        }
    }

    /// Enable or disable automatic summarization.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether automatic summarization is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Builder-style helper for toggling auto summarization when constructing the struct.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Check if conversation should be summarized
    pub fn should_summarize(
        &self,
        conversation_length: usize,
        context_size: usize,
        context_limit: usize,
    ) -> bool {
        if !self.enabled {
            let context_percent = if context_limit > 0 {
                ((context_size as f64 / context_limit as f64) * 100.0).round() as usize
            } else {
                0
            };
            if conversation_length >= self.summarization_threshold
                || context_percent >= self.context_trigger_percent as usize
            {
                debug!(
                    conversation_length,
                    context_percent,
                    summarization_threshold = self.summarization_threshold,
                    context_trigger_percent = self.context_trigger_percent,
                    "Auto summarization disabled; thresholds reached",
                );
            }
            return false;
        }

        let context_percent = if context_limit > 0 {
            ((context_size as f64 / context_limit as f64) * 100.0).round() as usize
        } else {
            0
        };

        let approaching_limit = context_percent >= self.context_trigger_percent as usize;
        let has_errors = context_percent >= self.error_trigger_percent as usize;
        let long_conversation = conversation_length >= self.summarization_threshold;

        let since_last = self
            .last_summary_turn
            .map(|last| conversation_length.saturating_sub(last))
            .unwrap_or(usize::MAX);

        let turn_trigger =
            long_conversation && since_last >= self.summarization_threshold.max(self.min_turn_gap);

        let context_trigger = (approaching_limit || has_errors) && since_last >= self.min_turn_gap;

        if turn_trigger || context_trigger {
            let reason = if turn_trigger && context_trigger {
                "turn_and_context"
            } else if turn_trigger {
                "turn_threshold"
            } else {
                "context_pressure"
            };

            debug!(
                conversation_length,
                context_percent,
                since_last,
                summarization_threshold = self.summarization_threshold,
                context_trigger_percent = self.context_trigger_percent,
                error_trigger_percent = self.error_trigger_percent,
                min_turn_gap = self.min_turn_gap,
                reason,
                "Auto summarization triggered",
            );
            return true;
        }

        false
    }

    /// Generate a conversation summary
    pub fn generate_summary(
        &mut self,
        conversation_turns: &[ConversationTurn],
        decision_history: &[DecisionInfo],
        error_history: &[ErrorInfo],
        session_start_time: u64,
    ) -> Result<ConversationSummary, SummarizationError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let session_duration = now - session_start_time;
        let total_turns = conversation_turns.len();

        // Extract key decisions
        let key_decisions = self.extract_key_decisions(decision_history, conversation_turns);

        // Extract completed tasks
        let completed_tasks = self.extract_completed_tasks(conversation_turns);

        // Analyze error patterns
        let error_patterns = self.analyze_error_patterns(error_history);

        // Generate context recommendations
        let context_recommendations = self.generate_context_recommendations(
            conversation_turns.len(),
            error_history.len(),
            session_duration,
        );

        // Generate summary text
        let summary_text = self.generate_summary_text(
            &key_decisions,
            &completed_tasks,
            &error_patterns,
            session_duration,
            total_turns,
        );

        // Calculate compression ratio
        let original_length = self.calculate_conversation_length(conversation_turns);
        let compression_ratio = if original_length > 0 {
            summary_text.len() as f64 / original_length as f64
        } else {
            1.0
        };

        // Calculate confidence score
        let confidence_score = self.calculate_confidence_score(
            key_decisions.len(),
            completed_tasks.len(),
            error_patterns.len(),
            compression_ratio,
        );

        let summary_id = format!("summary_{}", now);

        let summary = ConversationSummary {
            id: summary_id,
            timestamp: now,
            session_duration_seconds: session_duration,
            total_turns,
            key_decisions,
            completed_tasks,
            error_patterns,
            context_recommendations,
            summary_text,
            compression_ratio,
            confidence_score,
        };

        self.last_summary_turn = Some(total_turns);
        self.summaries.push(summary.clone());
        Ok(summary)
    }

    /// Extract key decisions from conversation and decision history
    fn extract_key_decisions(
        &self,
        decision_history: &[DecisionInfo],
        conversation_turns: &[ConversationTurn],
    ) -> Vec<KeyDecision> {
        let mut key_decisions = Vec::new();

        for (_i, decision) in decision_history.iter().enumerate() {
            let decision_type = match decision.action_type.as_str() {
                "tool_call" => DecisionType::ToolExecution,
                "response" => DecisionType::ResponseGeneration,
                "context_compression" => DecisionType::ContextCompression,
                "error_recovery" => DecisionType::ErrorRecovery,
                _ => DecisionType::WorkflowChange,
            };

            let importance_score = self.calculate_decision_importance(decision, conversation_turns);

            if importance_score > 0.6 {
                // Only include important decisions
                key_decisions.push(KeyDecision {
                    turn_number: decision.turn_number,
                    decision_type,
                    description: decision.description.clone(),
                    rationale: decision.reasoning.clone(),
                    outcome: decision.outcome.clone(),
                    importance_score,
                });
            }
        }

        // Limit to top 10 most important decisions
        key_decisions.sort_by(|a, b| b.importance_score.partial_cmp(&a.importance_score).unwrap());
        key_decisions.truncate(10);
        key_decisions
    }

    /// Extract completed tasks from conversation
    fn extract_completed_tasks(&self, conversation_turns: &[ConversationTurn]) -> Vec<TaskSummary> {
        let mut tasks = Vec::new();

        for turn in conversation_turns {
            if let Some(task_info) = &turn.task_info {
                if task_info.completed {
                    tasks.push(TaskSummary {
                        task_type: task_info.task_type.clone(),
                        description: task_info.description.clone(),
                        success: task_info.success,
                        duration_seconds: task_info.duration_seconds,
                        tools_used: task_info.tools_used.clone(),
                    });
                }
            }
        }

        tasks
    }

    /// Analyze patterns in error history
    fn analyze_error_patterns(&self, error_history: &[ErrorInfo]) -> Vec<ErrorPattern> {
        let mut error_counts = HashMap::new();

        // Count errors by type
        for error in error_history {
            *error_counts.entry(error.error_type.clone()).or_insert(0) += 1;
        }

        let mut patterns = Vec::new();
        for (error_type, frequency) in error_counts {
            if frequency >= 2 {
                // Only include errors that occurred multiple times
                let description = format!("{} error occurred {} times", error_type, frequency);
                let recommended_solution = self.get_error_solution(&error_type);

                patterns.push(ErrorPattern {
                    error_type,
                    frequency,
                    description,
                    recommended_solution,
                });
            }
        }

        patterns.sort_by(|a, b| b.frequency.cmp(&a.frequency));
        patterns
    }

    /// Generate context recommendations
    fn generate_context_recommendations(
        &self,
        turn_count: usize,
        error_count: usize,
        session_duration: u64,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if turn_count > 50 {
            recommendations.push(
                "Consider summarizing the conversation to maintain context efficiency".to_string(),
            );
        }

        if error_count > 5 {
            recommendations.push(
                "High error rate detected - review error patterns and consider context compression"
                    .to_string(),
            );
        }

        if session_duration > 1800 {
            // 30 minutes
            recommendations.push(
                "Long-running session detected - consider breaking into smaller tasks".to_string(),
            );
        }

        if recommendations.is_empty() {
            recommendations.push("Conversation is proceeding normally".to_string());
        }

        recommendations
    }

    /// Generate human-readable summary text
    fn generate_summary_text(
        &self,
        key_decisions: &[KeyDecision],
        completed_tasks: &[TaskSummary],
        error_patterns: &[ErrorPattern],
        session_duration: u64,
        total_turns: usize,
    ) -> String {
        let mut summary = format!(
            "Conversation Summary ({} turns, {} seconds):\n\n",
            total_turns, session_duration
        );

        if !key_decisions.is_empty() {
            summary.push_str("Key Decisions Made:\n");
            for decision in key_decisions.iter().take(5) {
                summary.push_str(&format!(
                    "• Turn {}: {} - {}\n",
                    decision.turn_number, decision.decision_type, decision.description
                ));
            }
            summary.push('\n');
        }

        if !completed_tasks.is_empty() {
            summary.push_str("Completed Tasks:\n");
            for task in completed_tasks {
                let status = if task.success {
                    "[Success]"
                } else {
                    "[Failure]"
                };
                summary.push_str(&format!(
                    "{} {} ({})\n",
                    status, task.description, task.task_type
                ));
            }
            summary.push('\n');
        }

        if !error_patterns.is_empty() {
            summary.push_str("Error Patterns:\n");
            for pattern in error_patterns {
                summary.push_str(&format!(
                    "• {}: {} ({} occurrences)\n",
                    pattern.error_type, pattern.description, pattern.frequency
                ));
            }
            summary.push('\n');
        }

        // Truncate if too long
        if summary.len() > self.max_summary_length {
            summary.truncate(self.max_summary_length - 3);
            summary.push_str("...");
        }

        summary
    }

    /// Calculate importance score for a decision
    fn calculate_decision_importance(
        &self,
        decision: &DecisionInfo,
        conversation_turns: &[ConversationTurn],
    ) -> f64 {
        let mut score = 0.5; // Base score

        // Increase score based on decision type importance
        match decision.action_type.as_str() {
            "tool_call" => score += 0.3,
            "context_compression" => score += 0.4,
            "error_recovery" => score += 0.2,
            _ => {}
        }

        // Increase score if decision had significant outcome
        if let Some(outcome) = &decision.outcome {
            if outcome.contains("success") || outcome.contains("completed") {
                score += 0.2;
            }
        }

        // Increase score if decision was made in later turns (potentially more important)
        let progress_ratio = decision.turn_number as f64 / conversation_turns.len() as f64;
        score += progress_ratio * 0.1;

        score.min(1.0)
    }

    /// Calculate conversation length in characters
    fn calculate_conversation_length(&self, conversation_turns: &[ConversationTurn]) -> usize {
        conversation_turns
            .iter()
            .map(|turn| turn.content.len())
            .sum()
    }

    /// Calculate confidence score for the summary
    fn calculate_confidence_score(
        &self,
        decision_count: usize,
        task_count: usize,
        error_count: usize,
        compression_ratio: f64,
    ) -> f64 {
        let mut confidence = 0.7; // Base confidence

        // Higher confidence with more decisions and tasks
        confidence += decision_count.min(10) as f64 * 0.02;
        confidence += task_count.min(10) as f64 * 0.03;

        // Lower confidence with many errors
        confidence -= error_count.min(10) as f64 * 0.05;

        // Adjust based on compression ratio (closer to target = higher confidence)
        let ratio_distance = (compression_ratio - self.compression_target_ratio).abs();
        confidence -= ratio_distance * 0.5;

        confidence.max(0.1).min(1.0)
    }

    /// Get recommended solution for error type
    fn get_error_solution(&self, error_type: &str) -> String {
        match error_type {
            "tool_execution" => "Review tool parameters and ensure correct file paths".to_string(),
            "api_call" => "Check API key and consider implementing retry logic".to_string(),
            "context_compression" => {
                "Monitor context size and implement proactive compression".to_string()
            }
            _ => "Investigate error details and consider context preservation".to_string(),
        }
    }

    /// Get all summaries
    pub fn get_summaries(&self) -> &[ConversationSummary] {
        &self.summaries
    }

    /// Get latest summary
    pub fn get_latest_summary(&self) -> Option<&ConversationSummary> {
        self.summaries.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summarizer_enabled() -> ConversationSummarizer {
        ConversationSummarizer::new().with_enabled(true)
    }

    #[test]
    fn disabled_summarizer_never_triggers() {
        let summarizer = ConversationSummarizer::new().with_enabled(false);
        assert!(!summarizer.should_summarize(25, 90_000, 100_000));
    }

    #[test]
    fn turn_based_trigger_respects_gap() {
        let mut summarizer = summarizer_enabled();
        let turns: Vec<ConversationTurn> = Vec::new();
        let decisions: Vec<DecisionInfo> = Vec::new();
        let errors: Vec<ErrorInfo> = Vec::new();

        assert!(summarizer.should_summarize(25, 10_000, 100_000));
        summarizer
            .generate_summary(&turns, &decisions, &errors, 0)
            .expect("generate summary succeeds");
        assert!(!summarizer.should_summarize(30, 10_000, 100_000));
        assert!(summarizer.should_summarize(50, 10_000, 100_000));
    }

    #[test]
    fn context_trigger_enforces_min_gap() {
        let mut summarizer = summarizer_enabled();
        let turns: Vec<ConversationTurn> = Vec::new();
        let decisions: Vec<DecisionInfo> = Vec::new();
        let errors: Vec<ErrorInfo> = Vec::new();

        assert!(summarizer.should_summarize(5, 90_000, 100_000));
        summarizer
            .generate_summary(&turns, &decisions, &errors, 0)
            .expect("generate summary succeeds");
        assert!(!summarizer.should_summarize(7, 90_000, 100_000));
        assert!(summarizer.should_summarize(15, 90_000, 100_000));
    }
}

/// Information about a conversation turn
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub turn_number: usize,
    pub content: String,
    pub role: String,
    pub task_info: Option<TaskInfo>,
}

/// Information about a task within a conversation turn
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub task_type: String,
    pub description: String,
    pub completed: bool,
    pub success: bool,
    pub duration_seconds: Option<u64>,
    pub tools_used: Vec<String>,
}

/// Information about a decision
#[derive(Debug, Clone)]
pub struct DecisionInfo {
    pub turn_number: usize,
    pub action_type: String,
    pub description: String,
    pub reasoning: String,
    pub outcome: Option<String>,
}

/// Information about an error
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub turn_number: usize,
    pub recoverable: bool,
}

/// Error that can occur during summarization
#[derive(Debug, Clone)]
pub enum SummarizationError {
    InsufficientData,
    ProcessingError(String),
}

impl std::fmt::Display for SummarizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SummarizationError::InsufficientData => {
                write!(f, "Insufficient data for summarization")
            }
            SummarizationError::ProcessingError(msg) => write!(f, "Processing error: {}", msg),
        }
    }
}

impl std::error::Error for SummarizationError {}

impl Default for ConversationSummarizer {
    fn default() -> Self {
        Self::new()
    }
}
