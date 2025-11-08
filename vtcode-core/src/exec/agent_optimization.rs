//! Agent behavior optimization and learning system
//!
//! Analyzes metrics from Steps 1-8 to provide guidance on:
//! - Tool discovery optimization
//! - Code pattern effectiveness
//! - Skill recommendations
//! - Version compatibility predictions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::debug;

/// Statistics about skill usage patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for SkillStatistics {
    fn default() -> Self {
        Self {
            creation_to_reuse_time: None,
            avg_lifecycle: None,
            reuse_ratio_by_tag: HashMap::new(),
            most_effective_skills: vec![],
            rarely_used_skills: vec![],
            total_skills: 0,
            reused_skills: 0,
        }
    }
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

/// Patterns of failures and recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for FailurePatterns {
    fn default() -> Self {
        Self {
            high_failure_tools: vec![],
            high_failure_patterns: vec![],
            common_errors: vec![],
            recovery_patterns: vec![],
        }
    }
}

/// Analyzes agent behavior from metrics history
pub struct AgentBehaviorAnalyzer {
    skill_stats: SkillStatistics,
    tool_stats: ToolStatistics,
    failure_patterns: FailurePatterns,
}

impl Default for AgentBehaviorAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBehaviorAnalyzer {
    /// Create a new behavior analyzer
    pub fn new() -> Self {
        Self {
            skill_stats: SkillStatistics::default(),
            tool_stats: ToolStatistics::default(),
            failure_patterns: FailurePatterns::default(),
        }
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

        // Find tools that match the query
        for (tool, _count) in self.tool_stats.usage_frequency.iter().take(limit) {
            if tool.to_lowercase().contains(&query.to_lowercase()) {
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
                .map(|(tool, _)| tool.to_string())
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
            .entry(tool_name.to_string())
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
                .insert(0, skill_name.to_string());
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
                .push((error_msg.to_string(), 1));
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
                .push((tool_name.to_string(), failure_rate));
        }

        debug!(
            "Recorded failure for {}: {} (failure_rate: {})",
            tool_name, error_msg, failure_rate
        );
    }

    /// Get analysis summary as string
    pub fn summary(&self) -> String {
        let mut output = String::new();
        output.push_str("=== Agent Behavior Analysis ===\n\n");

        output.push_str("## Skill Statistics\n");
        output.push_str(&format!(
            "Total skills: {}\n",
            self.skill_stats.total_skills
        ));
        output.push_str(&format!(
            "Reused skills: {}\n",
            self.skill_stats.reused_skills
        ));
        if !self.skill_stats.most_effective_skills.is_empty() {
            output.push_str(&format!(
                "Top skill: {}\n",
                self.skill_stats.most_effective_skills.first().unwrap()
            ));
        }

        output.push_str("\n## Tool Statistics\n");
        output.push_str(&format!(
            "Tool discovery success rate: {:.1}%\n",
            self.tool_stats.discovery_success_rate * 100.0
        ));
        output.push_str(&format!(
            "Total tools used: {}\n",
            self.tool_stats.usage_frequency.len()
        ));

        if !self.failure_patterns.high_failure_tools.is_empty() {
            output.push_str("\n## High-Risk Tools\n");
            for (tool, rate) in self.failure_patterns.high_failure_tools.iter().take(5) {
                output.push_str(&format!(
                    "- {} (failure rate: {:.1}%)\n",
                    tool,
                    rate * 100.0
                ));
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
        assert!(recommendations.contains(&"read_file".to_string()));
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
                .contains(&"filter_skill".to_string())
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
            .push(("risky_tool".to_string(), 0.8));
        analyzer
            .failure_patterns
            .high_failure_tools
            .push(("safe_tool".to_string(), 0.1));

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
                error_type: "timeout".to_string(),
                recovery_action: "retry with increased timeout".to_string(),
                success_rate: 0.85,
                attempts: 20,
            });

        let recovery = analyzer.get_recovery_strategy("timeout");
        assert!(recovery.is_some());
        assert_eq!(recovery.unwrap().success_rate, 0.85);
    }
}
