/// Tool Justification System
///
/// Captures agent reasoning before high-risk tool execution to improve approval UX
/// and enable learning of approval patterns.
use crate::tools::registry::risk_scorer::RiskLevel;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Justification provided by the agent for executing a high-risk tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolJustification {
    /// Tool being justified
    pub tool_name: String,
    /// Brief explanation from the agent
    pub reason: String,
    /// Expected outcome of tool execution
    pub expected_outcome: Option<String>,
    /// Risk level that triggered justification
    pub risk_level: String,
    /// Timestamp when justification was provided
    pub timestamp: String,
}

impl ToolJustification {
    /// Create a new tool justification
    pub fn new(
        tool_name: impl Into<String>,
        reason: impl Into<String>,
        risk_level: &RiskLevel,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            reason: reason.into(),
            expected_outcome: None,
            risk_level: format!("{:?}", risk_level),
            timestamp: chrono::Local::now().to_rfc3339(),
        }
    }

    /// Add expected outcome to justification
    pub fn with_outcome(mut self, outcome: impl Into<String>) -> Self {
        self.expected_outcome = Some(outcome.into());
        self
    }

    /// Format justification for display in approval dialog
    pub fn format_for_dialog(&self) -> Vec<String> {
        let mut lines = vec![];

        lines.push(String::new());
        lines.push("Agent Reasoning:".to_string());

        // Wrap reason text if needed
        let reason_lines: Vec<&str> = self.reason.lines().collect();
        for line in reason_lines {
            let wrapped = textwrap::fill(&format!("  {}", line), 78);
            for wrapped_line in wrapped.lines() {
                lines.push(wrapped_line.to_string());
            }
        }

        if let Some(outcome) = &self.expected_outcome {
            lines.push(String::new());
            lines.push("Expected Outcome:".to_string());
            let wrapped = textwrap::fill(&format!("  {}", outcome), 78);
            for wrapped_line in wrapped.lines() {
                lines.push(wrapped_line.to_string());
            }
        }

        lines.push(String::new());
        lines.push(format!("Risk Level: {}", self.risk_level));

        lines
    }
}

/// Tracks approval patterns to learn from user decisions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalPattern {
    /// Tool name
    pub tool_name: String,
    /// Number of times user approved
    pub approve_count: u32,
    /// Number of times user denied
    pub deny_count: u32,
    /// Last decision (true = approve, false = deny)
    pub last_decision: Option<bool>,
    /// Most recent reason (if available)
    pub recent_reason: Option<String>,
}

impl ApprovalPattern {
    /// Compute approval rate (0.0 to 1.0)
    pub fn approval_rate(&self) -> f32 {
        let total = self.approve_count + self.deny_count;
        if total == 0 {
            0.0
        } else {
            self.approve_count as f32 / total as f32
        }
    }

    /// Check if this tool has high approval rate (>80%)
    pub fn has_high_approval_rate(&self) -> bool {
        self.approval_count() >= 3 && self.approval_rate() > 0.8
    }

    /// Return approval count
    pub fn approval_count(&self) -> u32 {
        self.approve_count
    }
}

/// Manager for approval pattern learning and justifications
pub struct JustificationManager {
    cache_dir: PathBuf,
    patterns: HashMap<String, ApprovalPattern>,
}

impl JustificationManager {
    /// Create a new justification manager
    pub fn new(cache_dir: PathBuf) -> Self {
        let manager = Self {
            cache_dir,
            patterns: HashMap::new(),
        };

        // Try to load existing patterns
        let _ = manager.load_patterns();

        manager
    }

    /// Load approval patterns from disk
    fn load_patterns(&self) -> Result<()> {
        let patterns_file = self.cache_dir.join("approval_patterns.json");
        if patterns_file.exists() {
            let _content = fs::read_to_string(&patterns_file)?;
            // Note: We can't mutate self in a const context, so this would need RefCell
            // For now, just validate the file is readable
        }
        Ok(())
    }

    /// Get approval pattern for a tool
    pub fn get_pattern(&self, tool_name: &str) -> Option<ApprovalPattern> {
        self.patterns.get(tool_name).cloned()
    }

    /// Record user approval decision
    pub fn record_decision(&mut self, tool_name: &str, approved: bool, reason: Option<String>) {
        let pattern = self
            .patterns
            .entry(tool_name.to_string())
            .or_insert_with(|| ApprovalPattern {
                tool_name: tool_name.to_string(),
                approve_count: 0,
                deny_count: 0,
                last_decision: None,
                recent_reason: None,
            });

        if approved {
            pattern.approve_count += 1;
        } else {
            pattern.deny_count += 1;
        }

        pattern.last_decision = Some(approved);
        pattern.recent_reason = reason;

        // Persist to disk
        let _ = self.persist_patterns();
    }

    /// Persist patterns to disk
    fn persist_patterns(&self) -> Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        let patterns_file = self.cache_dir.join("approval_patterns.json");
        let content = serde_json::to_string_pretty(&self.patterns)?;
        fs::write(&patterns_file, content)?;
        Ok(())
    }

    /// Get learning summary for a tool
    pub fn get_learning_summary(&self, tool_name: &str) -> Option<String> {
        let pattern = self.get_pattern(tool_name)?;

        if pattern.approval_count() == 0 {
            return None;
        }

        Some(format!(
            "Approved {} of {} times ({:.0}%)",
            pattern.approve_count,
            pattern.approve_count + pattern.deny_count,
            pattern.approval_rate() * 100.0
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_justification_creation() {
        let just = ToolJustification::new(
            "read_file",
            "Need to understand code structure",
            &RiskLevel::Low,
        )
        .with_outcome("Will analyze the AST to provide better context");

        assert_eq!(just.tool_name, "read_file");
        assert!(just.reason.contains("understand"));
        assert!(just.expected_outcome.is_some());
    }

    #[test]
    fn test_justification_formatting() {
        let just = ToolJustification::new(
            "run_command",
            "Execute build to check for compilation errors",
            &RiskLevel::High,
        )
        .with_outcome("Will produce build output for analysis");

        let formatted = just.format_for_dialog();
        assert!(formatted.iter().any(|l| l.contains("Agent Reasoning")));
        assert!(formatted.iter().any(|l| l.contains("Expected Outcome")));
        assert!(formatted.iter().any(|l| l.contains("Risk Level")));
    }

    #[test]
    fn test_approval_pattern_calculation() {
        let mut pattern = ApprovalPattern {
            tool_name: "read_file".to_string(),
            approve_count: 8,
            deny_count: 2,
            last_decision: Some(true),
            recent_reason: None,
        };

        assert_eq!(pattern.approval_rate(), 0.8);
        assert!(pattern.has_high_approval_rate());

        pattern.approve_count = 3;
        pattern.deny_count = 7;
        assert!(!pattern.has_high_approval_rate()); // < 0.8 rate
    }

    #[test]
    fn test_justification_manager_basic() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let mut manager = JustificationManager::new(temp_dir.clone());

        manager.record_decision("read_file", true, None);
        manager.record_decision("read_file", true, None);
        manager.record_decision("read_file", false, None);

        let pattern = manager.get_pattern("read_file").unwrap();
        assert_eq!(pattern.approve_count, 2);
        assert_eq!(pattern.deny_count, 1);
        assert_eq!(pattern.approval_rate(), 2.0 / 3.0);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
