//! Context optimization for efficient context usage.
//!
//! Delegates tool-result reduction to the shared harness kernel used by both harnesses.

use crate::core::agent::harness_kernel::reduce_tool_result;

/// Context optimization manager
pub struct ContextOptimizer;

impl ContextOptimizer {
    /// Create a new context optimizer
    pub fn new() -> Self {
        Self
    }

    /// Optimize tool result with the shared reducer used by both harnesses.
    pub fn optimize_result(&self, tool_name: &str, result: serde_json::Value) -> serde_json::Value {
        reduce_tool_result(tool_name, result)
    }
}

impl Default for ContextOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use serde_json::json;

    #[tokio::test]
    async fn test_grep_optimization() {
        let optimizer = ContextOptimizer::new();

        let matches: Vec<_> = (0..20)
            .map(|i| json!({"line": i, "path": "src/main.rs", "text": "match"}))
            .collect();
        let result = json!({"matches": matches});

        let optimized = optimizer.optimize_result(tools::UNIFIED_SEARCH, result);

        let opt_matches = optimized["matches"].as_array().unwrap();
        assert_eq!(opt_matches.len(), 5);
        assert!(optimized["overflow"].is_string());
    }

    #[tokio::test]
    async fn test_grep_deduplicates_by_path_and_line() {
        let optimizer = ContextOptimizer::new();
        let matches = vec![
            json!({"line": 10, "path": "src/lib.rs", "text": "hit A"}),
            json!({"line": 10, "path": "src/lib.rs", "text": "hit A duplicate"}),
            json!({"line": 20, "path": "src/lib.rs", "text": "hit B"}),
        ];
        let result = json!({"matches": matches});

        let optimized = optimizer.optimize_result(tools::UNIFIED_SEARCH, result);
        let opt_matches = optimized["matches"].as_array().unwrap();
        assert_eq!(opt_matches.len(), 2);
        assert_eq!(optimized["total"], 2);
        assert!(optimized["note"].as_str().unwrap().contains("unique"));
    }

    #[tokio::test]
    async fn test_list_files_optimization() {
        let optimizer = ContextOptimizer::new();

        let files: Vec<_> = (0..100).map(|i| json!(format!("file{}.rs", i))).collect();
        let result = json!({"files": files});

        let optimized = optimizer.optimize_result(tools::UNIFIED_SEARCH, result);

        assert_eq!(optimized["total_files"], 100);
        assert!(optimized["sample"].is_array());
        assert!(optimized["note"].is_string());
    }

    #[tokio::test]
    async fn test_unified_exec_output_optimization_with_output_field() {
        let optimizer = ContextOptimizer::new();
        let long_output = (0..2505)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = json!({
            "command": "ls -la",
            "output": long_output,
            "exit_code": 0,
            "is_exited": true
        });

        let optimized = optimizer.optimize_result(tools::UNIFIED_EXEC, result);
        assert_eq!(optimized["is_truncated"], true);
        assert!(optimized["output"].as_str().unwrap().lines().count() <= 2000);
    }
}
