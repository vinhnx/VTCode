//! Parallel tool execution with safe batching and conflict detection

use crate::config::constants::tools;
use anyhow::Result;
use futures::future::join_all;
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
    conflict_map: HashMap<&'static str, &'static [&'static str]>,
}

impl ParallelExecutionPlanner {
    pub fn new() -> Self {
        let mut conflict_map = HashMap::new();

        // Define conflict relationships as static arrays for better performance
        conflict_map.insert(
            tools::READ_FILE,
            &[
                tools::WRITE_FILE,
                tools::EDIT_FILE,
                tools::DELETE_FILE,
                tools::APPLY_PATCH,
            ] as &[&str],
        );

        conflict_map.insert(
            tools::LIST_FILES,
            &[
                tools::WRITE_FILE,
                tools::EDIT_FILE,
                tools::DELETE_FILE,
                tools::APPLY_PATCH,
            ] as &[&str],
        );

        conflict_map.insert(
            tools::GREP_FILE,
            &[tools::WRITE_FILE, tools::EDIT_FILE, tools::APPLY_PATCH] as &[&str],
        );

        conflict_map.insert(
            tools::WRITE_FILE,
            &[
                tools::READ_FILE,
                tools::LIST_FILES,
                tools::WRITE_FILE,
                tools::EDIT_FILE,
                tools::APPLY_PATCH,
            ] as &[&str],
        );

        conflict_map.insert(
            tools::EDIT_FILE,
            &[
                tools::READ_FILE,
                tools::LIST_FILES,
                tools::WRITE_FILE,
                tools::EDIT_FILE,
                tools::APPLY_PATCH,
            ] as &[&str],
        );

        conflict_map.insert(
            tools::APPLY_PATCH,
            &[
                tools::READ_FILE,
                tools::LIST_FILES,
                tools::WRITE_FILE,
                tools::EDIT_FILE,
                tools::APPLY_PATCH,
            ] as &[&str],
        );

        Self { conflict_map }
    }

    /// Partition tool calls into non-conflicting groups
    pub fn plan(&self, calls: &[(String, Arc<Value>, String)]) -> Vec<ExecutionGroup> {
        let mut groups = Vec::new();
        let mut assigned = vec![false; calls.len()];
        let mut group_id = 0;

        for i in 0..calls.len() {
            if assigned[i] {
                continue;
            }

            let (tool_i, args_i, call_id_i) = &calls[i];
            let mut group = ExecutionGroup::new(group_id);
            group.add_call(tool_i.clone(), args_i.clone(), call_id_i.clone());
            assigned[i] = true;

            // Find compatible calls
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
            group_id += 1;
        }

        groups
    }

    fn conflicts(&self, tool_a: &str, tool_b: &str) -> bool {
        // Check both directions due to symmetry
        if let Some(conflicts) = self.conflict_map.get(tool_a)
            && conflicts.contains(&tool_b)
        {
            return true;
        }

        if let Some(conflicts) = self.conflict_map.get(tool_b)
            && conflicts.contains(&tool_a)
        {
            return true;
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
}

impl Default for ExecutionResultCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionResultCollector {
    pub fn add_result(&mut self, call_id: String, result: Value) {
        self.results.insert(call_id, result);
    }

    pub fn add_error(&mut self, call_id: String, error: String) {
        self.errors.insert(call_id, error);
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn get_results(&self) -> &HashMap<String, Value> {
        &self.results
    }

    pub fn get_errors(&self) -> &HashMap<String, String> {
        &self.errors
    }

    pub fn into_parts(self) -> (HashMap<String, Value>, HashMap<String, String>) {
        (self.results, self.errors)
    }
}

/// Execute a group of tools in parallel
pub async fn execute_group_parallel(
    group: &ExecutionGroup,
    executor: Arc<dyn crate::tools::ToolExecutor>,
) -> Result<ExecutionResultCollector> {
    let mut collector = ExecutionResultCollector::new();

    if group.can_parallel && group.len() > 1 {
        // Execute in parallel
        let futures: Vec<_> = group
            .tool_calls
            .iter()
            .map(|(tool, args, call_id)| {
                let executor = Arc::clone(&executor);
                let tool = tool.clone();
                let call_id = call_id.clone();
                let args = Arc::clone(args);
                async move {
                    match executor.execute_tool_ref(&tool, &args).await {
                        Ok(result) => (call_id, Ok(result)),
                        Err(e) => (call_id, Err(e.to_string())),
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;

        for (call_id, result) in results {
            match result {
                Ok(value) => collector.add_result(call_id, value),
                Err(error) => collector.add_error(call_id, error),
            }
        }
    } else {
        // Execute sequentially
        for (tool, args, call_id) in &group.tool_calls {
            match executor.execute_tool_ref(tool, args).await {
                Ok(result) => collector.add_result(call_id.clone(), result),
                Err(e) => collector.add_error(call_id.clone(), e.to_string()),
            }
        }
    }

    Ok(collector)
}

/// Execute multiple tool groups sequentially
pub async fn execute_groups_sequential(
    groups: Vec<ExecutionGroup>,
    executor: Arc<dyn crate::tools::ToolExecutor>,
) -> Result<ExecutionResultCollector> {
    let mut final_collector = ExecutionResultCollector::new();

    for group in groups {
        let group_result = execute_group_parallel(&group, Arc::clone(&executor)).await?;

        // Merge results
        for (call_id, result) in group_result.get_results() {
            final_collector.add_result(call_id.clone(), result.clone());
        }

        for (call_id, error) in group_result.get_errors() {
            final_collector.add_error(call_id.clone(), error.clone());
        }

        // Stop on first error if needed
        if final_collector.has_errors() {
            break;
        }
    }

    Ok(final_collector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_detection() {
        let planner = ParallelExecutionPlanner::new();

        // Read operations should conflict with write operations
        assert!(planner.conflicts(tools::READ_FILE, tools::WRITE_FILE));
        assert!(planner.conflicts(tools::READ_FILE, tools::EDIT_FILE));
        assert!(planner.conflicts(tools::LIST_FILES, tools::WRITE_FILE));

        // Read operations should not conflict with each other
        assert!(!planner.conflicts(tools::READ_FILE, tools::LIST_FILES));
        assert!(!planner.conflicts(tools::READ_FILE, tools::GREP_FILE));

        // Write operations should conflict with each other
        assert!(planner.conflicts(tools::WRITE_FILE, tools::EDIT_FILE));
        assert!(planner.conflicts(tools::WRITE_FILE, tools::APPLY_PATCH));
    }

    #[test]
    fn test_execution_grouping() {
        let planner = ParallelExecutionPlanner::new();

        let calls = vec![
            (
                tools::READ_FILE.to_string(),
                Arc::new(serde_json::json!({"path": "file1.txt"})),
                "call1".to_string(),
            ),
            (
                tools::LIST_FILES.to_string(),
                Arc::new(serde_json::json!({"path": "."})),
                "call2".to_string(),
            ),
            (
                tools::WRITE_FILE.to_string(),
                Arc::new(serde_json::json!({"path": "file2.txt", "content": "test"})),
                "call3".to_string(),
            ),
            (
                tools::GREP_FILE.to_string(),
                Arc::new(serde_json::json!({"pattern": "test", "path": "."})),
                "call4".to_string(),
            ),
        ];

        let groups = planner.plan(&calls);

        // Should have at least 2 groups (reads in one, writes in another)
        assert!(groups.len() >= 2);

        // Verify that conflicting tools are in different groups
        let read_group = groups
            .iter()
            .find(|g| g.tool_calls.iter().any(|(t, _, _)| t == tools::READ_FILE));
        let write_group = groups
            .iter()
            .find(|g| g.tool_calls.iter().any(|(t, _, _)| t == tools::WRITE_FILE));

        assert!(read_group.is_some());
        assert!(write_group.is_some());
        assert_ne!(read_group.unwrap().group_id, write_group.unwrap().group_id);
    }
}
