//! Context optimization for efficient context usage.
//!
//! This keeps the legacy checkpoint API while delegating tool-result reduction
//! to the shared harness kernel used by both harnesses.

use crate::core::agent::harness_kernel::reduce_tool_result;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Checkpoint state for context reset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointState {
    pub task_description: String,
    pub completed_steps: Vec<String>,
    pub current_work: String,
    pub next_steps: Vec<String>,
    pub key_files: Vec<String>,
    pub timestamp: u64,
}

/// Context optimization manager
pub struct ContextOptimizer;

impl ContextOptimizer {
    /// Create a new context optimizer
    pub fn new() -> Self {
        Self
    }

    /// Get current budget utilization (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        0.0
    }

    /// Check if checkpoint is needed
    pub fn needs_checkpoint(&self) -> bool {
        self.utilization() >= 0.85
    }

    /// Optimize tool result with the shared reducer used by both harnesses.
    pub fn optimize_result(&self, tool_name: &str, result: serde_json::Value) -> serde_json::Value {
        reduce_tool_result(tool_name, result)
    }

    /// Create checkpoint state for context reset
    pub async fn create_checkpoint(
        &self,
        task_description: String,
        completed_steps: Vec<String>,
        current_work: String,
        next_steps: Vec<String>,
        key_files: Vec<String>,
    ) -> CheckpointState {
        CheckpointState {
            task_description,
            completed_steps,
            current_work,
            next_steps,
            key_files,
            timestamp: crate::utils::current_timestamp(),
        }
    }

    /// Save checkpoint to file
    pub async fn save_checkpoint(&self, path: &Path, checkpoint: &CheckpointState) -> Result<()> {
        let json =
            serde_json::to_string_pretty(checkpoint).context("Failed to serialize checkpoint")?;
        fs::write(path, json)
            .await
            .context("Failed to write checkpoint file")?;
        Ok(())
    }

    /// Load checkpoint from file
    pub async fn load_checkpoint(path: &Path) -> Result<CheckpointState> {
        let json = fs::read_to_string(path)
            .await
            .context("Failed to read checkpoint file")?;
        let checkpoint: CheckpointState =
            serde_json::from_str(&json).context("Failed to deserialize checkpoint")?;
        Ok(checkpoint)
    }

    /// Get budget status message
    pub async fn budget_status(&self) -> String {
        "[INFO] Context optimization disabled".to_string()
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
    async fn test_checkpoint_save_load() {
        let optimizer = ContextOptimizer::new();

        let checkpoint = optimizer
            .create_checkpoint(
                "Test task".to_string(),
                vec!["Step 1".to_string()],
                "Current work".to_string(),
                vec!["Next step".to_string()],
                vec!["file1.rs".to_string()],
            )
            .await;

        let temp_path = std::env::temp_dir().join("test_checkpoint.json");
        optimizer
            .save_checkpoint(&temp_path, &checkpoint)
            .await
            .unwrap();

        let loaded = ContextOptimizer::load_checkpoint(&temp_path).await.unwrap();

        assert_eq!(loaded.task_description, checkpoint.task_description);
        assert_eq!(loaded.completed_steps, checkpoint.completed_steps);
        assert_eq!(loaded.current_work, checkpoint.current_work);

        let _ = std::fs::remove_file(&temp_path);
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
