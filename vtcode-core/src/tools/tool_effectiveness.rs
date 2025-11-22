//! Tool effectiveness tracking and adaptive selection
//!
//! Tracks which tools are effective for given contexts and helps the agent
//! select the best tool based on prior success rates and result quality.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::tools::result_metadata::ResultMetadata;

/// Tracks effectiveness of a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEffectiveness {
    pub tool_name: String,

    /// Success rate (0.0-1.0)
    pub success_rate: f32,

    /// Average result quality (0.0-1.0)
    pub avg_result_quality: f32,

    /// Number of times tool was used
    pub usage_count: usize,

    /// Number of successful executions
    pub success_count: usize,

    /// Last time tool was used
    pub last_used_timestamp: u64,

    /// Common failure modes
    #[serde(default)]
    pub failure_modes: Vec<ToolFailureMode>,

    /// Average execution time in milliseconds
    #[serde(default)]
    pub avg_execution_time_ms: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ToolFailureMode {
    Timeout,
    NoResults,
    InvalidArgs,
    ParseError,
    PermissionDenied,
    Unknown,
}

impl ToolEffectiveness {
    pub fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            success_rate: 0.0,
            avg_result_quality: 0.0,
            usage_count: 0,
            success_count: 0,
            last_used_timestamp: 0,
            failure_modes: vec![],
            avg_execution_time_ms: 0.0,
        }
    }

    /// Record a successful tool execution
    pub fn record_success(&mut self, quality: f32, execution_time_ms: f32) {
        self.usage_count += 1;
        self.success_count += 1;
        self.last_used_timestamp = current_timestamp();

        // Update rolling average of quality
        self.avg_result_quality =
            (self.avg_result_quality * (self.success_count - 1) as f32 + quality) / self.success_count as f32;

        // Update rolling average of execution time
        self.avg_execution_time_ms = (self.avg_execution_time_ms * (self.success_count - 1) as f32
            + execution_time_ms)
            / self.success_count as f32;

        self.update_success_rate();
    }

    /// Record a failed tool execution
    pub fn record_failure(&mut self, failure_mode: ToolFailureMode, execution_time_ms: f32) {
        self.usage_count += 1;
        self.last_used_timestamp = current_timestamp();

        // Track failure mode
        if !self.failure_modes.iter().any(|m| m == &failure_mode) {
            self.failure_modes.push(failure_mode);
        }

        // Update rolling average of execution time
        let success_count_f = (self.success_count + 1) as f32;
        self.avg_execution_time_ms =
            (self.avg_execution_time_ms * (self.success_count as f32 / success_count_f)) + (execution_time_ms / success_count_f);

        self.update_success_rate();
    }

    fn update_success_rate(&mut self) {
        if self.usage_count > 0 {
            self.success_rate = self.success_count as f32 / self.usage_count as f32;
        }
    }

    /// Get overall effectiveness score
    pub fn effectiveness_score(&self) -> f32 {
        if self.usage_count == 0 {
            return 0.5; // Unknown
        }

        // Weight success rate (60%) and result quality (40%)
        (self.success_rate * 0.6) + (self.avg_result_quality * 0.4)
    }

    /// Whether this tool is considered reliable
    pub fn is_reliable(&self) -> bool {
        self.usage_count >= 3 && self.success_rate > 0.7
    }

    /// Time since last use in seconds
    pub fn time_since_last_use_seconds(&self) -> u64 {
        if self.last_used_timestamp == 0 {
            u64::MAX
        } else {
            current_timestamp().saturating_sub(self.last_used_timestamp)
        }
    }
}

/// Tool selection context
#[derive(Debug, Clone)]
pub struct ToolSelectionContext {
    /// Description of current task
    pub task_description: String,

    /// Tools already used in current context
    pub prior_tools_used: Vec<String>,

    /// Quality scores of prior results
    pub prior_result_qualities: Vec<f32>,

    /// Current effectiveness snapshot
    pub tool_effectiveness: HashMap<String, ToolEffectiveness>,
}

/// Trait for selecting which tool to use
pub trait ToolSelector: Send + Sync {
    fn select_tool(
        &self,
        context: &ToolSelectionContext,
        candidates: &[&str],
    ) -> Option<String>;
}

/// Adaptive tool selector based on effectiveness
pub struct AdaptiveToolSelector;

impl ToolSelector for AdaptiveToolSelector {
    fn select_tool(
        &self,
        context: &ToolSelectionContext,
        candidates: &[&str],
    ) -> Option<String> {
        if candidates.is_empty() {
            return None;
        }

        // Score each candidate
        let mut scored: Vec<(String, f32)> = candidates
            .iter()
            .map(|tool| {
                let name = tool.to_string();
                let score = score_tool(&name, context);
                (name, score)
            })
            .collect();

        // Sort by score (highest first)
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.first().map(|(name, _)| name.clone())
    }
}

/// Score a tool based on context
fn score_tool(tool_name: &str, context: &ToolSelectionContext) -> f32 {
    let mut score = 0.5; // Base score

    // Get effectiveness for this tool
    if let Some(eff) = context.tool_effectiveness.get(tool_name) {
        // Factor 1: Tool effectiveness history (weight: 40%)
        score += eff.effectiveness_score() * 0.4;

        // Factor 2: Execution time (prefer faster tools, weight: 10%)
        let time_score = 1.0 - (eff.avg_execution_time_ms / 10000.0).min(1.0);
        score += time_score * 0.1;

        // Factor 3: Reliability penalty for tools with recent failures
        if !eff.failure_modes.is_empty() {
            let failure_penalty = (eff.failure_modes.len() as f32) * 0.1;
            score -= failure_penalty;
        }
    }

    // Factor 4: Tool diversity - penalize recently used tools
    if context.prior_tools_used.contains(&tool_name.to_string()) {
        score -= 0.15; // Avoid repeating same tool
    }

    // Normalize to 0.0-1.0 range
    score.max(0.0).min(1.0)
}

/// Tracker for tool effectiveness across a session
pub struct ToolEffectivenessTracker {
    effectiveness: HashMap<String, ToolEffectiveness>,
}

impl ToolEffectivenessTracker {
    pub fn new() -> Self {
        Self {
            effectiveness: HashMap::new(),
        }
    }

    /// Get or create effectiveness tracker for tool
    fn get_or_create(&mut self, tool_name: &str) -> &mut ToolEffectiveness {
        self.effectiveness
            .entry(tool_name.to_string())
            .or_insert_with(|| ToolEffectiveness::new(tool_name.to_string()))
    }

    /// Record successful tool execution
    pub fn record_success(&mut self, tool_name: &str, metadata: &ResultMetadata, execution_time_ms: f32) {
        let quality = metadata.quality_score();
        self.get_or_create(tool_name).record_success(quality, execution_time_ms);
    }

    /// Record failed tool execution
    pub fn record_failure(&mut self, tool_name: &str, mode: ToolFailureMode, execution_time_ms: f32) {
        self.get_or_create(tool_name).record_failure(mode, execution_time_ms);
    }

    /// Get effectiveness snapshot
    pub fn snapshot(&self) -> HashMap<String, ToolEffectiveness> {
        self.effectiveness.clone()
    }

    /// Get effectiveness for specific tool
    pub fn get(&self, tool_name: &str) -> Option<&ToolEffectiveness> {
        self.effectiveness.get(tool_name)
    }

    /// Get tools sorted by effectiveness
    pub fn sorted_by_effectiveness(&self) -> Vec<(String, f32)> {
        let mut tools: Vec<_> = self
            .effectiveness
            .iter()
            .map(|(name, eff)| (name.clone(), eff.effectiveness_score()))
            .collect();

        tools.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        tools
    }
}

impl Default for ToolEffectivenessTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_effectiveness_success() {
        let mut eff = ToolEffectiveness::new("grep".to_string());
        eff.record_success(0.9, 100.0);
        eff.record_success(0.85, 110.0);

        assert_eq!(eff.success_count, 2);
        assert_eq!(eff.usage_count, 2);
        assert_eq!(eff.success_rate, 1.0);
        assert!(eff.avg_result_quality > 0.8);
    }

    #[test]
    fn test_tool_effectiveness_failure() {
        let mut eff = ToolEffectiveness::new("find".to_string());
        eff.record_failure(ToolFailureMode::Timeout, 5000.0);

        assert_eq!(eff.success_count, 0);
        assert_eq!(eff.usage_count, 1);
        assert_eq!(eff.success_rate, 0.0);
        assert!(eff.failure_modes.contains(&ToolFailureMode::Timeout));
    }

    #[test]
    fn test_adaptive_selector() {
        let selector = AdaptiveToolSelector;
        let mut effectiveness = HashMap::new();

        let mut grep_eff = ToolEffectiveness::new("grep".to_string());
        grep_eff.record_success(0.9, 100.0);
        effectiveness.insert("grep".to_string(), grep_eff);

        let mut find_eff = ToolEffectiveness::new("find".to_string());
        find_eff.record_failure(ToolFailureMode::Timeout, 5000.0);
        effectiveness.insert("find".to_string(), find_eff);

        let context = ToolSelectionContext {
            task_description: "find error patterns".to_string(),
            prior_tools_used: vec![],
            prior_result_qualities: vec![],
            tool_effectiveness: effectiveness,
        };

        let selected = selector.select_tool(&context, &["grep", "find"]);
        assert_eq!(selected, Some("grep".to_string()));
    }

    #[test]
    fn test_tool_diversity_penalty() {
        let selector = AdaptiveToolSelector;
        let effectiveness = HashMap::new();

        let context = ToolSelectionContext {
            task_description: "find files".to_string(),
            prior_tools_used: vec!["grep".to_string()],
            prior_result_qualities: vec![],
            tool_effectiveness: effectiveness,
        };

        let selected = selector.select_tool(&context, &["grep", "find"]);
        // Should prefer find over grep since grep was recently used
        assert_eq!(selected, Some("find".to_string()));
    }

    #[test]
    fn test_effectiveness_tracker() {
        let mut tracker = ToolEffectivenessTracker::new();
        let meta = ResultMetadata::success(0.8, 0.8);

        tracker.record_success("grep", &meta, 100.0);
        tracker.record_success("grep", &meta, 110.0);

        let sorted = tracker.sorted_by_effectiveness();
        assert_eq!(sorted[0].0, "grep");
        assert!(sorted[0].1 > 0.7);
    }
}
