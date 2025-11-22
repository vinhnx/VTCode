//! Tool execution context tracking
//!
//! Tracks the context of tool executions within a session to detect patterns,
//! prevent redundancy, and suggest better alternatives.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::SystemTime;

use crate::tools::result_metadata::EnhancedToolResult;
use crate::tools::tool_effectiveness::ToolEffectiveness;
use std::collections::HashMap;

/// A record of a single tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRecord {
    pub tool_name: String,
    pub args: Value,
    pub result: EnhancedToolResult,
    pub timestamp: u64,
    pub execution_time_ms: u64,
}

impl ToolExecutionRecord {
    pub fn new(
        tool_name: String,
        args: Value,
        result: EnhancedToolResult,
        execution_time_ms: u64,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            tool_name,
            args,
            result,
            timestamp,
            execution_time_ms,
        }
    }
}

/// Detected pattern across tool executions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolPattern {
    /// Same tool/pattern searched with multiple tools
    RedundantSearch {
        tools: Vec<String>,
        pattern: String,
    },

    /// Results build on each other sequentially
    SequentialRefinement {
        tools: Vec<String>,
        refinement_steps: usize,
    },

    /// Multiple tools converged on same finding
    ConvergentDiagnosis {
        tools: Vec<String>,
        common_finding: String,
    },

    /// Tool produced low quality despite multiple attempts
    LowQualityLoop {
        tool: String,
        attempts: usize,
    },
}

/// Context for cross-tool awareness
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// Session identifier
    pub session_id: String,

    /// Description of current task
    pub current_task: String,

    /// Recent execution history (limited to prevent memory bloat)
    execution_history: VecDeque<ToolExecutionRecord>,

    /// Maximum history size
    max_history_size: usize,

    /// Detected patterns
    patterns: Vec<ToolPattern>,

    /// Tool effectiveness metrics
    effectiveness_snapshot: HashMap<String, ToolEffectiveness>,
}

impl ToolExecutionContext {
    pub fn new(session_id: String, task: String) -> Self {
        Self {
            session_id,
            current_task: task,
            execution_history: VecDeque::with_capacity(100),
            max_history_size: 100,
            patterns: vec![],
            effectiveness_snapshot: HashMap::new(),
        }
    }

    /// Add an execution record
    pub fn add_record(&mut self, record: ToolExecutionRecord) {
        // Detect patterns before adding
        self.detect_patterns(&record);

        self.execution_history.push_back(record);

        // Keep history size bounded
        while self.execution_history.len() > self.max_history_size {
            self.execution_history.pop_front();
        }
    }

    /// Check if current call is redundant with recent history
    pub fn is_redundant(&self, tool: &str, args: &Value) -> bool {
        let recent_limit = 5;

        self.execution_history
            .iter()
            .rev()
            .take(recent_limit)
            .any(|record| {
                record.tool_name == tool && are_args_equivalent(&record.args, args)
            })
    }

    /// Get recent tool names (up to N)
    pub fn recent_tools(&self, n: usize) -> Vec<String> {
        self.execution_history
            .iter()
            .rev()
            .take(n)
            .map(|r| r.tool_name.clone())
            .collect()
    }

    /// Get tools that produced good results recently
    pub fn high_performing_tools(&self, n: usize) -> Vec<String> {
        let mut tools: Vec<_> = self
            .execution_history
            .iter()
            .rev()
            .filter(|r| r.result.metadata.quality_score() > 0.7)
            .take(n)
            .map(|r| r.tool_name.clone())
            .collect();

        tools.sort();
        tools.dedup();
        tools
    }

    /// Suggest a fallback tool based on prior effectiveness
    pub fn suggest_fallback(&self, failed_tool: &str) -> Option<String> {
        // Find most effective tool that hasn't been tried recently
        let recent = self.recent_tools(3);

        self.effectiveness_snapshot
            .values()
            .filter(|eff| {
                eff.success_rate > 0.7 && !recent.contains(&eff.tool_name)
                    && eff.tool_name != failed_tool
            })
            .max_by(|a, b| {
                a.effectiveness_score()
                    .partial_cmp(&b.effectiveness_score())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|eff| eff.tool_name.clone())
    }

    /// Get all detected patterns
    pub fn patterns(&self) -> &[ToolPattern] {
        &self.patterns
    }

    /// Update effectiveness snapshot
    pub fn set_effectiveness(&mut self, snapshot: HashMap<String, ToolEffectiveness>) {
        self.effectiveness_snapshot = snapshot;
    }

    /// Get effectiveness snapshot
    pub fn effectiveness(&self) -> &HashMap<String, ToolEffectiveness> {
        &self.effectiveness_snapshot
    }

    /// Get full execution history
    pub fn history(&self) -> Vec<&ToolExecutionRecord> {
        self.execution_history.iter().collect()
    }

    /// Detect patterns in execution
    fn detect_patterns(&mut self, new_record: &ToolExecutionRecord) {
        // Pattern 1: Redundant searches
        let recent_records: Vec<_> = self
            .execution_history
            .iter()
            .rev()
            .take(10)
            .collect();

        let mut same_pattern_tools = vec![new_record.tool_name.clone()];
        for record in &recent_records {
            if are_args_equivalent(&record.args, &new_record.args) {
                same_pattern_tools.push(record.tool_name.clone());
            }
        }

        if same_pattern_tools.len() > 2 {
            same_pattern_tools.sort();
            same_pattern_tools.dedup();
            self.patterns.push(ToolPattern::RedundantSearch {
                tools: same_pattern_tools,
                pattern: format!("{:?}", new_record.args),
            });
        }

        // Pattern 2: Low quality loop
        let recent_same_tool: Vec<_> = recent_records
            .iter()
            .filter(|r| r.tool_name == new_record.tool_name)
            .collect();

        if recent_same_tool.len() > 3 {
            let avg_quality = recent_same_tool
                .iter()
                .map(|r| r.result.metadata.quality_score())
                .sum::<f32>()
                / recent_same_tool.len() as f32;

            if avg_quality < 0.4 {
                self.patterns.push(ToolPattern::LowQualityLoop {
                    tool: new_record.tool_name.clone(),
                    attempts: recent_same_tool.len() + 1,
                });
            }
        }
    }
}

/// Check if two sets of arguments are equivalent (for deduplication)
pub fn are_args_equivalent(a: &Value, b: &Value) -> bool {
    // Normalize and compare
    match (a, b) {
        (Value::Object(a_map), Value::Object(b_map)) => {
            // Exact match of all keys and values
            a_map.len() == b_map.len()
                && a_map
                    .iter()
                    .all(|(k, v)| b_map.get(k).map_or(false, |bv| bv == v))
        }
        (Value::Array(a_arr), Value::Array(b_arr)) => {
            a_arr.len() == b_arr.len()
                && a_arr
                    .iter()
                    .zip(b_arr.iter())
                    .all(|(av, bv)| av == bv)
        }
        (Value::String(a_str), Value::String(b_str)) => a_str == b_str,
        (Value::Number(a_num), Value::Number(b_num)) => a_num == b_num,
        (Value::Bool(a_bool), Value::Bool(b_bool)) => a_bool == b_bool,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::result_metadata::ResultMetadata;

    fn make_record(tool: &str, arg_val: i32) -> ToolExecutionRecord {
        ToolExecutionRecord::new(
            tool.to_string(),
            Value::Number(arg_val.into()),
            EnhancedToolResult::new(
                Value::Null,
                ResultMetadata::success(0.8, 0.8),
                tool.to_string(),
            ),
            100,
        )
    }

    #[test]
    fn test_execution_context_creation() {
        let ctx = ToolExecutionContext::new(
            "session-1".to_string(),
            "find errors".to_string(),
        );

        assert_eq!(ctx.session_id, "session-1");
        assert_eq!(ctx.current_task, "find errors");
    }

    #[test]
    fn test_add_record() {
        let mut ctx = ToolExecutionContext::new(
            "session-1".to_string(),
            "test".to_string(),
        );

        let record = make_record("grep", 1);
        ctx.add_record(record);

        assert_eq!(ctx.history().len(), 1);
    }

    #[test]
    fn test_is_redundant() {
        let mut ctx = ToolExecutionContext::new(
            "session-1".to_string(),
            "test".to_string(),
        );

        let args = Value::String("pattern".to_string());

        ctx.add_record(ToolExecutionRecord::new(
            "grep".to_string(),
            args.clone(),
            EnhancedToolResult::new(Value::Null, ResultMetadata::default(), "grep".to_string()),
            100,
        ));

        assert!(ctx.is_redundant("grep", &args));
    }

    #[test]
    fn test_recent_tools() {
        let mut ctx = ToolExecutionContext::new(
            "session-1".to_string(),
            "test".to_string(),
        );

        ctx.add_record(make_record("grep", 1));
        ctx.add_record(make_record("find", 2));
        ctx.add_record(make_record("grep", 3));

        let recent = ctx.recent_tools(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0], "grep"); // Most recent first
    }

    #[test]
    fn test_args_equivalent() {
        let a = Value::String("pattern".to_string());
        let b = Value::String("pattern".to_string());
        assert!(are_args_equivalent(&a, &b));

        let c = Value::String("different".to_string());
        assert!(!are_args_equivalent(&a, &c));
    }
}
