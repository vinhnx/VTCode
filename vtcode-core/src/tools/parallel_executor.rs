//! Parallel tool execution with safe batching and conflict detection

use anyhow::Result;
use futures::future::{BoxFuture, join_all};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Safe parallel execution group (no conflicts)
pub struct ExecutionGroup {
    pub group_id: usize,
    pub tool_calls: Vec<(String, Arc<Value>, String)>, // (tool, args, call_id)
    pub can_parallel: bool,
}

impl ExecutionGroup {
    pub fn new(group_id: usize) -> Self {
        Self {
            group_id,
            tool_calls: Vec::new(),
            can_parallel: true,
        }
    }

    pub fn add_call(&mut self, tool: String, args: Arc<Value>, call_id: String) {
        self.tool_calls.push((tool, args, call_id));
    }

    pub fn len(&self) -> usize {
        self.tool_calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tool_calls.is_empty()
    }
}

/// Partition tool calls into non-conflicting groups
pub struct ParallelExecutionPlanner {
    conflict_map: HashMap<&'static str, Vec<&'static str>>,
}

impl ParallelExecutionPlanner {
    pub fn new() -> Self {
        let mut conflict_map = HashMap::new();

        // Define conflict relationships (read-write, write-write)
        let conflicts: Vec<(&'static str, Vec<&'static str>)> = vec![
            ("read_file", vec!["write_file", "edit_file", "delete_file"]),
            ("list_files", vec!["write_file", "edit_file", "delete_file"]),
            ("grep", vec!["write_file", "edit_file"]),
            (
                "write_file",
                vec!["read_file", "list_files", "write_file", "edit_file"],
            ),
            (
                "edit_file",
                vec!["read_file", "list_files", "write_file", "edit_file"],
            ),
        ];

        for (tool, conflicting) in conflicts {
            conflict_map.insert(tool, conflicting);
        }

        Self { conflict_map }
    }

    /// Partition tool calls into non-conflicting groups
    pub fn plan(&self, calls: &[(String, Arc<Value>, String)]) -> Vec<ExecutionGroup> {
        if calls.is_empty() {
            return Vec::new();
        }

        let mut groups: Vec<ExecutionGroup> = Vec::new();
        let mut assigned = vec![false; calls.len()];

        for (i, (tool_i, args_i, call_id_i)) in calls.iter().enumerate() {
            if assigned[i] {
                continue;
            }

            let mut group = ExecutionGroup::new(groups.len());
            group.add_call(tool_i.clone(), args_i.clone(), call_id_i.clone());
            assigned[i] = true;

            // Find other compatible tools
            for (j, (tool_j, args_j, call_id_j)) in calls.iter().enumerate() {
                if assigned[j] || i == j {
                    continue;
                }

                // Check if compatible
                if !self.conflicts(tool_i.as_str(), tool_j.as_str()) {
                    group.add_call(tool_j.clone(), args_j.clone(), call_id_j.clone());
                    assigned[j] = true;
                }
            }

            groups.push(group);
        }

        groups
    }

    fn conflicts(&self, tool_a: &str, tool_b: &str) -> bool {
        // Check both directions due to symmetry
        if let Some(conflicts) = self.conflict_map.get(tool_a) {
            if conflicts.iter().any(|&c| c == tool_b) {
                return true;
            }
        }

        if let Some(conflicts) = self.conflict_map.get(tool_b) {
            if conflicts.iter().any(|&c| c == tool_a) {
                return true;
            }
        }

        false
    }
}

impl Default for ParallelExecutionPlanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch execution result collector
pub struct ExecutionResultCollector {
    results: HashMap<String, Value>,
    errors: HashMap<String, String>,
}

impl ExecutionResultCollector {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
            errors: HashMap::new(),
        }
    }

    pub fn add_success(&mut self, call_id: String, result: Value) {
        self.results.insert(call_id, result);
    }

    pub fn add_error(&mut self, call_id: String, error: String) {
        self.errors.insert(call_id, error);
    }

    pub fn get_result(&self, call_id: &str) -> Result<&Value> {
        self.results
            .get(call_id)
            .ok_or_else(|| anyhow::anyhow!("No result for call {}", call_id))
    }

    pub fn get_error(&self, call_id: &str) -> Option<&str> {
        self.errors.get(call_id).map(|s| s.as_str())
    }

    pub fn into_results(self) -> HashMap<String, Value> {
        self.results
    }

    pub fn into_errors(self) -> HashMap<String, String> {
        self.errors
    }

    pub fn len(&self) -> usize {
        self.results.len() + self.errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty() && self.errors.is_empty()
    }
}

impl Default for ExecutionResultCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Parallel executor trait for dependency injection
pub trait ParallelToolExecutor: Send + Sync {
    fn execute_tool<'a>(
        &'a self,
        tool: &'a str,
        args: &'a Value,
        call_id: &'a str,
    ) -> BoxFuture<'a, Result<Value>>;
}

/// Execute multiple groups sequentially, with groups executing in parallel
pub async fn execute_groups<E: ParallelToolExecutor>(
    groups: Vec<ExecutionGroup>,
    executor: &E,
) -> Result<ExecutionResultCollector> {
    let mut collector = ExecutionResultCollector::new();

    for group in groups {
        // Execute group in parallel
        let futures: Vec<_> = group
            .tool_calls
            .iter()
            .map(|(tool, args, call_id)| {
                executor.execute_tool(tool.as_str(), args.as_ref(), call_id.as_str())
            })
            .collect();

        let results = join_all(futures).await;

        // Collect results
        for ((_, _, call_id), result) in group.tool_calls.iter().zip(results) {
            match result {
                Ok(value) => collector.add_success(call_id.clone(), value),
                Err(e) => collector.add_error(call_id.clone(), e.to_string()),
            }
        }
    }

    Ok(collector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_planner_no_conflicts() {
        let planner = ParallelExecutionPlanner::new();

        let calls = vec![
            (
                "grep".to_string(),
                Arc::new(json!({"pattern": "test"})),
                "id1".to_string(),
            ),
            (
                "list_files".to_string(),
                Arc::new(json!({"path": "."})),
                "id2".to_string(),
            ),
        ];

        // These shouldn't conflict
        assert!(!planner.conflicts("grep", "list_files"));
    }

    #[test]
    fn test_planner_with_conflicts() {
        let planner = ParallelExecutionPlanner::new();

        assert!(planner.conflicts("read_file", "write_file"));
        assert!(planner.conflicts("write_file", "read_file"));
        assert!(planner.conflicts("write_file", "write_file"));
    }

    #[test]
    fn test_execution_planning() {
        let planner = ParallelExecutionPlanner::new();

        let calls = vec![
            (
                "read_file".to_string(),
                Arc::new(json!({})),
                "id1".to_string(),
            ),
            (
                "write_file".to_string(),
                Arc::new(json!({})),
                "id2".to_string(),
            ),
            ("grep".to_string(), Arc::new(json!({})), "id3".to_string()),
        ];

        let groups = planner.plan(&calls);
        // Should have at least 2 groups (read_file + grep can't run with write_file)
        assert!(groups.len() >= 2);
    }

    #[test]
    fn test_result_collector() {
        let mut collector = ExecutionResultCollector::new();

        collector.add_success("id1".to_string(), json!({"result": "ok"}));
        collector.add_error("id2".to_string(), "Failed".to_string());

        assert_eq!(collector.len(), 2);
        assert!(collector.get_result("id1").is_ok());
        assert_eq!(collector.get_error("id2"), Some("Failed"));
    }
}
