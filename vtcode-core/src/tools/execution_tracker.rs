//! Tool execution tracking with optimized storage
//!
//! Tracks tool executions for observability and optimization while
//! minimizing memory overhead through strategic data retention.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

/// Lightweight execution record (optimized for memory)
#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    /// Tool name (stored once, referenced by ID)
    pub tool_id: u32,
    /// Execution status
    pub status: ExecutionStatus,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Timestamp of execution
    pub timestamp: Instant,
    /// Whether result was cached
    pub was_cached: bool,
}

/// Serializable version of ExecutionRecord
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecordSnapshot {
    pub tool_id: u32,
    pub status: ExecutionStatus,
    pub duration_ms: u64,
    pub was_cached: bool,
}

/// Tool execution status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    Success,
    Failed,
    TimedOut,
    Cancelled,
}

/// Tool execution tracker with bounded memory usage
pub struct ExecutionTracker {
    /// Mapping of tool names to IDs (deduplicate strings)
    tool_ids: Vec<String>,
    /// Bounded execution history
    history: VecDeque<ExecutionRecord>,
    /// Maximum history size
    max_history: usize,
    /// Execution stats
    stats: ExecutionStats,
}

/// Execution statistics
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    pub total_executions: u64,
    pub successful: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub cached_hits: u64,
    pub total_duration_ms: u64,
}

impl ExecutionTracker {
    pub fn new(max_history: usize) -> Self {
        Self {
            tool_ids: Vec::new(),
            history: VecDeque::with_capacity(max_history),
            max_history,
            stats: ExecutionStats::default(),
        }
    }

    /// Record a tool execution
    pub fn record(
        &mut self,
        tool_name: &str,
        status: ExecutionStatus,
        duration_ms: u64,
        was_cached: bool,
    ) {
        // Intern tool name to reduce memory
        let tool_id = self.get_or_intern_tool_id(tool_name);

        // Update stats
        self.stats.total_executions += 1;
        self.stats.total_duration_ms += duration_ms;
        match status {
            ExecutionStatus::Success => self.stats.successful += 1,
            ExecutionStatus::Failed => self.stats.failed += 1,
            ExecutionStatus::TimedOut => self.stats.timed_out += 1,
            ExecutionStatus::Cancelled => {}
        }
        if was_cached {
            self.stats.cached_hits += 1;
        }

        // Add record with bounded history
        let record = ExecutionRecord {
            tool_id,
            status,
            duration_ms,
            timestamp: Instant::now(),
            was_cached,
        };

        self.history.push_back(record);
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }

    /// Get or create a tool ID (interns string)
    fn get_or_intern_tool_id(&mut self, tool_name: &str) -> u32 {
        self.tool_ids
            .iter()
            .position(|t| t == tool_name)
            .unwrap_or_else(|| {
                self.tool_ids.push(tool_name.to_string());
                self.tool_ids.len() - 1
            }) as u32
    }

    /// Get tool name from ID
    pub fn get_tool_name(&self, id: u32) -> Option<&str> {
        self.tool_ids.get(id as usize).map(|s| s.as_str())
    }

    /// Get recent executions (bounded to prevent large allocations)
    pub fn recent_executions(&self, n: usize) -> Vec<(String, ExecutionStatus, u64)> {
        self.history
            .iter()
            .rev()
            .take(n)
            .filter_map(|rec| {
                self.get_tool_name(rec.tool_id)
                    .map(|name| (name.to_string(), rec.status, rec.duration_ms))
            })
            .collect()
    }

    /// Get execution statistics
    pub fn stats(&self) -> &ExecutionStats {
        &self.stats
    }

    /// Get average execution time for a tool
    pub fn avg_duration_for_tool(&self, tool_name: &str) -> Option<u64> {
        let executions: Vec<_> = self
            .history
            .iter()
            .filter(|rec| self.get_tool_name(rec.tool_id) == Some(tool_name))
            .collect();

        if executions.is_empty() {
            return None;
        }

        let total: u64 = executions.iter().map(|e| e.duration_ms).sum();
        Some(total / executions.len() as u64)
    }

    /// Clear history (useful for session boundaries)
    pub fn clear(&mut self) {
        self.history.clear();
        self.stats = ExecutionStats::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_tracker_bounds() {
        let mut tracker = ExecutionTracker::new(5);

        for i in 0..10 {
            tracker.record("read_file", ExecutionStatus::Success, 100, i % 2 == 0);
        }

        assert_eq!(tracker.history.len(), 5);
        assert_eq!(tracker.stats.total_executions, 10);
        assert_eq!(tracker.stats.cached_hits, 5);
    }

    #[test]
    fn test_tool_id_interning() {
        let mut tracker = ExecutionTracker::new(10);

        tracker.record("read_file", ExecutionStatus::Success, 100, false);
        tracker.record("read_file", ExecutionStatus::Success, 50, true);
        tracker.record("write_file", ExecutionStatus::Success, 200, false);

        assert_eq!(tracker.tool_ids.len(), 2);
    }

    #[test]
    fn test_avg_duration() {
        let mut tracker = ExecutionTracker::new(10);

        tracker.record("read_file", ExecutionStatus::Success, 100, false);
        tracker.record("read_file", ExecutionStatus::Success, 200, false);
        tracker.record("read_file", ExecutionStatus::Success, 300, false);

        assert_eq!(tracker.avg_duration_for_tool("read_file"), Some(200));
        assert_eq!(tracker.avg_duration_for_tool("write_file"), None);
    }
}
