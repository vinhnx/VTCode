//! Agent behavior optimization and learning system
//!
//! Analyzes metrics from Steps 1-8 to provide guidance on:
//! - Tool discovery optimization
//! - Code pattern effectiveness
//! - Skill recommendations
//! - Version compatibility predictions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;
use std::time::Duration;
use tracing::debug;

/// Statistics about skill usage patterns
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStatistics {
    /// Time from creation to first reuse
    pub creation_to_reuse_time: Option<Duration>,
    /// Average lifetime of a skill
    pub avg_lifecycle: Option<Duration>,
    /// Reuse ratio by tag (0.0-1.0)
    pub reuse_ratio_by_tag: HashMap<String, f64>,
    /// Most effective (frequently reused) skills
    pub most_effective_skills: Vec<String>,
    /// Rarely used skills
    pub rarely_used_skills: Vec<String>,
    /// Total skills tracked
    pub total_skills: usize,
    /// Skills reused more than once
    pub reused_skills: usize,
}

/// Statistics about tool usage patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatistics {
    /// Success rate of tool discovery (0.0-1.0)
    pub discovery_success_rate: f64,
    /// How many times each tool was used
    pub usage_frequency: HashMap<String, u64>,
    /// Tools often discovered together
    pub common_tool_chains: Vec<Vec<String>>,
    /// Typical queries used to find tools
    pub typical_discovery_queries: Vec<String>,
    /// Total tool discovery attempts
    pub total_discoveries: u64,
    /// Successful discoveries
    pub successful_discoveries: u64,
}

impl Default for ToolStatistics {
    fn default() -> Self {
        Self {
            discovery_success_rate: 0.0,
            usage_frequency: HashMap::new(),
            common_tool_chains: vec![],
            typical_discovery_queries: vec![],
            total_discoveries: 0,
            successful_discoveries: 0,
        }
    }
}

/// Pattern in code that is associated with failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodePattern {
    /// Programming language (python3, javascript)
    pub language: String,
    /// Pattern description or regex
    pub pattern: String,
    /// Failure rate when this pattern is used
    pub failure_rate: f64,
    /// Example code that failed with this pattern
    pub example_failures: Vec<String>,
    /// Times this pattern appeared
    pub occurrences: u64,
}

/// Pattern for recovering from specific errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPattern {
    /// Type of error (e.g., "timeout", "missing_tool")
    pub error_type: String,
    /// What action helps recover (e.g., "retry with increased timeout")
    pub recovery_action: String,
    /// Success rate of this recovery (0.0-1.0)
    pub success_rate: f64,
    /// How many times this recovery was tried
    pub attempts: u64,
}

/// Result of applying a recovery pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedRecovery {
    /// Type of error being recovered from
    pub error_type: String,
    /// The recovery action to take
    pub recovery_action: String,
    /// Historical success rate of this recovery
    pub success_rate: f64,
    /// Number of times this pattern has been attempted
    pub attempts: u64,
}

/// Patterns of failures and recovery
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailurePatterns {
    /// Tools with high failure rates
    pub high_failure_tools: Vec<(String, f64)>,
    /// Code patterns associated with failures
    pub high_failure_patterns: Vec<CodePattern>,
    /// Most common errors and their frequencies
    pub common_errors: Vec<(String, u64)>,
    /// Effective recovery patterns
    pub recovery_patterns: Vec<RecoveryPattern>,
}

/// Analyzes agent behavior from metrics history
#[derive(Default)]
pub struct AgentBehaviorAnalyzer {
    skill_stats: SkillStatistics,
    tool_stats: ToolStatistics,
    failure_patterns: FailurePatterns,
}

impl AgentBehaviorAnalyzer {
    /// Create a new behavior analyzer
    pub fn new() -> Self {
        Self::default()
    }

    /// Get skill statistics
    pub fn skill_stats(&self) -> &SkillStatistics {
        &self.skill_stats
    }

    /// Get tool statistics
    pub fn tool_stats(&self) -> &ToolStatistics {
        &self.tool_stats
    }

    /// Get failure patterns
    pub fn failure_patterns(&self) -> &FailurePatterns {
        &self.failure_patterns
    }

    /// Recommend tools based on usage patterns
    pub fn recommend_tools(&self, query: &str, limit: usize) -> Vec<String> {
        let mut recommendations = vec![];
        let query_lower = query.to_lowercase();

        // Find tools that match the query
        for (tool, _count) in self.tool_stats.usage_frequency.iter().take(limit) {
            if tool.to_lowercase().contains(&query_lower) {
                recommendations.push(tool.clone());
            }
        }

        // If no exact matches, return most used tools
        if recommendations.is_empty() {
            let mut by_usage: Vec<_> = self.tool_stats.usage_frequency.iter().collect();
            by_usage.sort_by(|a, b| b.1.cmp(a.1));
            recommendations = by_usage
                .iter()
                .take(limit)
                .map(|pair| pair.0.clone())
                .collect();
        }

        recommendations
    }

    /// Recommend skills based on effectiveness
    pub fn recommend_skills(&self, limit: usize) -> Vec<String> {
        self.skill_stats
            .most_effective_skills
            .iter()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Warn about tools with high failure rates
    pub fn identify_risky_tools(&self, failure_threshold: f64) -> Vec<(String, f64)> {
        self.failure_patterns
            .high_failure_tools
            .iter()
            .filter(|(_tool, rate)| *rate >= failure_threshold)
            .cloned()
            .collect()
    }

    /// Get recovery strategy for an error type
    pub fn get_recovery_strategy(&self, error_type: &str) -> Option<RecoveryPattern> {
        self.failure_patterns
            .recovery_patterns
            .iter()
            .find(|p| p.error_type == error_type)
            .cloned()
    }

    /// Record tool usage
    pub fn record_tool_usage(&mut self, tool_name: &str) {
        *self
            .tool_stats
            .usage_frequency
            .entry(tool_name.to_owned())
            .or_insert(0) += 1;
    }

    /// Record skill reuse
    pub fn record_skill_reuse(&mut self, skill_name: &str) {
        if let Some(pos) = self
            .skill_stats
            .most_effective_skills
            .iter()
            .position(|s| s == skill_name)
        {
            // Move to front
            let skill = self.skill_stats.most_effective_skills.remove(pos);
            self.skill_stats.most_effective_skills.insert(0, skill);
        } else {
            self.skill_stats
                .most_effective_skills
                .insert(0, skill_name.to_owned());
        }
        self.skill_stats.reused_skills += 1;
    }

    /// Record tool failure
    pub fn record_tool_failure(&mut self, tool_name: &str, error_msg: &str) {
        // Add to common errors
        if let Some(pos) = self
            .failure_patterns
            .common_errors
            .iter()
            .position(|(msg, _)| msg == error_msg)
        {
            self.failure_patterns.common_errors[pos].1 += 1;
        } else {
            self.failure_patterns
                .common_errors
                .push((error_msg.to_owned(), 1));
        }

        // Update high failure tools - find count
        let count = self
            .failure_patterns
            .common_errors
            .iter()
            .find(|(msg, _)| msg == error_msg)
            .map(|(_, c)| *c)
            .unwrap_or(1);
        let failure_rate = count as f64 / (count + 1) as f64; // Approximation

        if let Some(pos) = self
            .failure_patterns
            .high_failure_tools
            .iter()
            .position(|t| t.0 == tool_name)
        {
            self.failure_patterns.high_failure_tools[pos].1 = failure_rate;
        } else {
            self.failure_patterns
                .high_failure_tools
                .push((tool_name.to_owned(), failure_rate));
        }

        debug!(
            "Recorded failure for {}: {} (failure_rate: {})",
            tool_name, error_msg, failure_rate
        );
    }

    /// Check if a tool should trigger a warning due to high failure rate
    pub fn should_warn(&self, tool_name: &str) -> Option<String> {
        for (tool, rate) in &self.failure_patterns.high_failure_tools {
            if tool == tool_name && *rate >= 0.5 {
                return Some(format!(
                    "Tool '{}' has a high failure rate ({:.1}%). Consider alternative approaches.",
                    tool_name,
                    rate * 100.0
                ));
            }
        }
        None
    }

    /// Get recovery action for a known error type
    pub fn get_recovery_action(&self, error_type: &str) -> Option<String> {
        self.failure_patterns
            .recovery_patterns
            .iter()
            .find(|p| p.error_type == error_type)
            .map(|p| {
                format!(
                    "{} (success rate: {:.1}%)",
                    p.recovery_action,
                    p.success_rate * 100.0
                )
            })
    }

    /// Export metrics for monitoring integration
    pub fn export_metrics(&self) -> HashMap<String, serde_json::Value> {
        let mut metrics = HashMap::new();

        // Skill metrics
        metrics.insert(
            "total_skills".to_string(),
            serde_json::json!(self.skill_stats.total_skills),
        );
        metrics.insert(
            "reused_skills".to_string(),
            serde_json::json!(self.skill_stats.reused_skills),
        );

        // Tool metrics
        metrics.insert(
            "discovery_success_rate".to_string(),
            serde_json::json!(self.tool_stats.discovery_success_rate),
        );
        metrics.insert(
            "total_tools_used".to_string(),
            serde_json::json!(self.tool_stats.usage_frequency.len()),
        );

        // Failure metrics
        metrics.insert(
            "high_failure_tools_count".to_string(),
            serde_json::json!(self.failure_patterns.high_failure_tools.len()),
        );
        metrics.insert(
            "common_errors_count".to_string(),
            serde_json::json!(self.failure_patterns.common_errors.len()),
        );
        metrics.insert(
            "recovery_patterns_count".to_string(),
            serde_json::json!(self.failure_patterns.recovery_patterns.len()),
        );

        // Tool usage frequency
        let mut tool_usage: Vec<_> = self.tool_stats.usage_frequency.iter().collect();
        tool_usage.sort_by(|a, b| b.1.cmp(a.1));
        let top_tools: HashMap<String, u64> = tool_usage
            .into_iter()
            .take(10)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        metrics.insert("top_tools".to_string(), serde_json::json!(top_tools));

        metrics
    }

    /// Get usage count for a tool
    pub fn tool_usage_count(&self, tool_name: &str) -> u64 {
        *self.tool_stats.usage_frequency.get(tool_name).unwrap_or(&0)
    }

    /// Get failure rate for a tool (0.0-1.0), defaults to 0.0 when unknown
    pub fn tool_failure_rate(&self, tool_name: &str) -> f64 {
        self.failure_patterns
            .high_failure_tools
            .iter()
            .find(|(tool, _)| tool == tool_name)
            .map(|(_, rate)| *rate)
            .unwrap_or(0.0)
    }

    /// Estimate success rate for a tool using usage counts and observed failure rate
    pub fn tool_success_rate(&self, tool_name: &str) -> f64 {
        let usage = self.tool_usage_count(tool_name);
        if usage == 0 {
            return 1.0;
        }

        let failure_rate = self.tool_failure_rate(tool_name).clamp(0.0, 1.0);
        (1.0 - failure_rate).max(0.0)
    }

    /// Apply recovery pattern automatically for a known error type
    /// Returns the recovery action to take, or None if no pattern exists
    pub fn apply_recovery_pattern(&mut self, error_type: &str) -> Option<AppliedRecovery> {
        // Find the best recovery pattern for this error type
        let pattern = self
            .failure_patterns
            .recovery_patterns
            .iter()
            .find(|p| p.error_type == error_type)?;

        // Record that we're applying this pattern
        let applied = AppliedRecovery {
            error_type: error_type.to_owned(),
            recovery_action: pattern.recovery_action.clone(),
            success_rate: pattern.success_rate,
            attempts: pattern.attempts,
        };

        debug!(
            "Applying recovery pattern for '{}': {} (success rate: {:.1}%)",
            error_type,
            pattern.recovery_action,
            pattern.success_rate * 100.0
        );

        Some(applied)
    }

    /// Record the outcome of an applied recovery pattern
    pub fn record_recovery_outcome(&mut self, error_type: &str, success: bool) {
        if let Some(pattern) = self
            .failure_patterns
            .recovery_patterns
            .iter_mut()
            .find(|p| p.error_type == error_type)
        {
            pattern.attempts += 1;
            if success {
                // Update success rate using exponential moving average
                let alpha = 0.3; // Weight for new observation
                pattern.success_rate = alpha + (1.0 - alpha) * pattern.success_rate;
            } else {
                // Decrease success rate
                let alpha = 0.3;
                pattern.success_rate = (1.0 - alpha) * pattern.success_rate;
            }

            debug!(
                "Updated recovery pattern '{}': success_rate={:.1}%, attempts={}",
                error_type,
                pattern.success_rate * 100.0,
                pattern.attempts
            );
        }
    }

    /// Add or update a recovery pattern
    pub fn add_recovery_pattern(
        &mut self,
        error_type: String,
        recovery_action: String,
        initial_success_rate: f64,
    ) {
        // Check if pattern already exists
        if let Some(pattern) = self
            .failure_patterns
            .recovery_patterns
            .iter_mut()
            .find(|p| p.error_type == error_type)
        {
            // Update existing pattern
            pattern.recovery_action = recovery_action;
            pattern.success_rate = initial_success_rate;
        } else {
            // Add new pattern
            self.failure_patterns
                .recovery_patterns
                .push(RecoveryPattern {
                    error_type,
                    recovery_action,
                    success_rate: initial_success_rate,
                    attempts: 0,
                });
        }
    }

    /// Get analysis summary as string
    pub fn summary(&self) -> String {
        let mut output = String::new();
        output.push_str("=== Agent Behavior Analysis ===\n\n");

        output.push_str("## Skill Statistics\n");
        let _ = writeln!(output, "Total skills: {}", self.skill_stats.total_skills);
        let _ = writeln!(output, "Reused skills: {}", self.skill_stats.reused_skills);
        if !self.skill_stats.most_effective_skills.is_empty() {
            let _ = writeln!(
                output,
                "Top skill: {}",
                self.skill_stats.most_effective_skills.first().unwrap()
            );
        }

        output.push_str("\n## Tool Statistics\n");
        let _ = writeln!(
            output,
            "Tool discovery success rate: {:.1}%",
            self.tool_stats.discovery_success_rate * 100.0
        );
        let _ = writeln!(
            output,
            "Total tools used: {}",
            self.tool_stats.usage_frequency.len()
        );

        if !self.failure_patterns.high_failure_tools.is_empty() {
            output.push_str("\n## High-Risk Tools\n");
            for (tool, rate) in self.failure_patterns.high_failure_tools.iter().take(5) {
                let _ = writeln!(output, "- {} (failure rate: {:.1}%)", tool, rate * 100.0);
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = AgentBehaviorAnalyzer::new();
        assert_eq!(analyzer.skill_stats.total_skills, 0);
        assert_eq!(analyzer.tool_stats.total_discoveries, 0);
    }

    #[test]
    fn test_recommend_tools() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.record_tool_usage("read_file");
        analyzer.record_tool_usage("read_file");
        analyzer.record_tool_usage("write_file");
        analyzer.record_tool_usage("list_files");

        let recommendations = analyzer.recommend_tools("read", 1);
        assert!(recommendations.contains(&"read_file".to_owned()));
    }

    #[test]
    fn test_record_skill_reuse() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.record_skill_reuse("filter_skill");
        analyzer.record_skill_reuse("filter_skill");
        analyzer.record_skill_reuse("transform_skill");

        assert_eq!(analyzer.skill_stats.reused_skills, 3);
        assert!(
            analyzer
                .skill_stats
                .most_effective_skills
                .contains(&"filter_skill".to_owned())
        );
    }

    #[test]
    fn test_tool_failure_tracking() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.record_tool_failure("grep_file", "timeout");
        analyzer.record_tool_failure("grep_file", "timeout");
        analyzer.record_tool_failure("grep_file", "pattern_error");

        assert!(!analyzer.failure_patterns.high_failure_tools.is_empty());
        assert!(analyzer.failure_patterns.high_failure_tools[0].0 == "grep_file");
    }

    #[test]
    fn test_summary_generation() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.skill_stats.total_skills = 5;
        analyzer.record_skill_reuse("test_skill");

        let summary = analyzer.summary();
        assert!(summary.contains("Skill Statistics"));
        assert!(summary.contains("Total skills: 5"));
        assert!(summary.contains("Reused skills: 1"));
    }

    #[test]
    fn test_identify_risky_tools() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .high_failure_tools
            .push(("risky_tool".to_owned(), 0.8));
        analyzer
            .failure_patterns
            .high_failure_tools
            .push(("safe_tool".to_owned(), 0.1));

        let risky = analyzer.identify_risky_tools(0.5);
        assert_eq!(risky.len(), 1);
        assert_eq!(risky[0].0, "risky_tool");
    }

    #[test]
    fn test_recovery_pattern_lookup() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "timeout".to_owned(),
                recovery_action: "retry with increased timeout".to_owned(),
                success_rate: 0.85,
                attempts: 20,
            });

        let recovery = analyzer.get_recovery_strategy("timeout");
        assert!(recovery.is_some());
        assert_eq!(recovery.unwrap().success_rate, 0.85);
    }

    #[test]
    fn test_should_warn() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .high_failure_tools
            .push(("risky_tool".to_owned(), 0.7));

        let warning = analyzer.should_warn("risky_tool");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("high failure rate"));

        let no_warning = analyzer.should_warn("safe_tool");
        assert!(no_warning.is_none());
    }

    #[test]
    fn test_get_recovery_action() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "network_error".to_owned(),
                recovery_action: "retry with exponential backoff".to_owned(),
                success_rate: 0.9,
                attempts: 15,
            });

        let action = analyzer.get_recovery_action("network_error");
        assert!(action.is_some());
        let action_str = action.unwrap();
        assert!(action_str.contains("retry with exponential backoff"));
        assert!(action_str.contains("90.0%"));
    }

    #[test]
    fn test_export_metrics() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.skill_stats.total_skills = 10;
        analyzer.skill_stats.reused_skills = 5;
        analyzer.record_tool_usage("test_tool");

        let metrics = analyzer.export_metrics();
        assert_eq!(metrics.get("total_skills").unwrap(), &serde_json::json!(10));
        assert_eq!(metrics.get("reused_skills").unwrap(), &serde_json::json!(5));
        assert_eq!(
            metrics.get("total_tools_used").unwrap(),
            &serde_json::json!(1)
        );
    }

    #[test]
    fn test_apply_recovery_pattern() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "timeout".to_owned(),
                recovery_action: "retry with increased timeout".to_owned(),
                success_rate: 0.85,
                attempts: 20,
            });

        let applied = analyzer.apply_recovery_pattern("timeout");
        assert!(applied.is_some());
        let applied = applied.unwrap();
        assert_eq!(applied.error_type, "timeout");
        assert_eq!(applied.recovery_action, "retry with increased timeout");
        assert_eq!(applied.success_rate, 0.85);
        assert_eq!(applied.attempts, 20);

        // Non-existent error type
        let no_pattern = analyzer.apply_recovery_pattern("unknown_error");
        assert!(no_pattern.is_none());
    }

    #[test]
    fn test_record_recovery_outcome_success() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "network_error".to_owned(),
                recovery_action: "retry".to_owned(),
                success_rate: 0.5,
                attempts: 10,
            });

        analyzer.record_recovery_outcome("network_error", true);

        let pattern = &analyzer.failure_patterns.recovery_patterns[0];
        assert_eq!(pattern.attempts, 11);
        // Success rate should increase (exponential moving average)
        assert!(pattern.success_rate > 0.5);
    }

    #[test]
    fn test_record_recovery_outcome_failure() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "parse_error".to_owned(),
                recovery_action: "simplify input".to_owned(),
                success_rate: 0.8,
                attempts: 5,
            });

        analyzer.record_recovery_outcome("parse_error", false);

        let pattern = &analyzer.failure_patterns.recovery_patterns[0];
        assert_eq!(pattern.attempts, 6);
        // Success rate should decrease
        assert!(pattern.success_rate < 0.8);
    }

    #[test]
    fn test_add_recovery_pattern_new() {
        let mut analyzer = AgentBehaviorAnalyzer::new();

        analyzer.add_recovery_pattern(
            "new_error".to_owned(),
            "new recovery action".to_owned(),
            0.75,
        );

        assert_eq!(analyzer.failure_patterns.recovery_patterns.len(), 1);
        let pattern = &analyzer.failure_patterns.recovery_patterns[0];
        assert_eq!(pattern.error_type, "new_error");
        assert_eq!(pattern.recovery_action, "new recovery action");
        assert_eq!(pattern.success_rate, 0.75);
        assert_eq!(pattern.attempts, 0);
    }

    #[test]
    fn test_add_recovery_pattern_update_existing() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer
            .failure_patterns
            .recovery_patterns
            .push(RecoveryPattern {
                error_type: "existing_error".to_owned(),
                recovery_action: "old action".to_owned(),
                success_rate: 0.5,
                attempts: 10,
            });

        analyzer.add_recovery_pattern(
            "existing_error".to_owned(),
            "updated action".to_owned(),
            0.9,
        );

        assert_eq!(analyzer.failure_patterns.recovery_patterns.len(), 1);
        let pattern = &analyzer.failure_patterns.recovery_patterns[0];
        assert_eq!(pattern.error_type, "existing_error");
        assert_eq!(pattern.recovery_action, "updated action");
        assert_eq!(pattern.success_rate, 0.9);
        // Attempts should be preserved from original
        assert_eq!(pattern.attempts, 10);
    }

    #[test]
    fn test_export_metrics_with_recovery_patterns() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.add_recovery_pattern("error1".to_owned(), "action1".to_owned(), 0.8);
        analyzer.add_recovery_pattern("error2".to_owned(), "action2".to_owned(), 0.9);

        let metrics = analyzer.export_metrics();
        assert_eq!(
            metrics.get("recovery_patterns_count").unwrap(),
            &serde_json::json!(2)
        );
    }

    #[test]
    fn test_export_metrics_with_top_tools() {
        let mut analyzer = AgentBehaviorAnalyzer::new();
        analyzer.record_tool_usage("tool_a");
        analyzer.record_tool_usage("tool_a");
        analyzer.record_tool_usage("tool_a");
        analyzer.record_tool_usage("tool_b");
        analyzer.record_tool_usage("tool_b");
        analyzer.record_tool_usage("tool_c");

        let metrics = analyzer.export_metrics();
        let top_tools = metrics.get("top_tools").unwrap();

        // Verify it's a JSON object
        assert!(top_tools.is_object());

        // Verify tool_a has highest count
        let top_tools_map = top_tools.as_object().unwrap();
        assert_eq!(top_tools_map.get("tool_a").unwrap(), &serde_json::json!(3));
        assert_eq!(top_tools_map.get("tool_b").unwrap(), &serde_json::json!(2));
        assert_eq!(top_tools_map.get("tool_c").unwrap(), &serde_json::json!(1));
    }
}
