//! Context optimization for efficient context usage
//!
//! Implements context engineering principles from AGENTS.md:
//! - Per-tool output curation (max 5 grep results, summarize 50+ files)
//! - Semantic context over volume

use crate::config::constants::tools;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::Path;
use tokio::fs;

/// Maximum results to show per tool
const MAX_GREP_RESULTS: usize = 5;
const MAX_LIST_FILES: usize = 50;
const MAX_FILE_LINES: usize = 2000;

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
pub struct ContextOptimizer {
    history: VecDeque<ContextEntry>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ContextEntry {
    tool_name: String,
    result: Value,
}

impl ContextOptimizer {
    /// Create a new context optimizer
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
        }
    }

    /// Get current budget utilization (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        0.0
    }

    /// Check if checkpoint is needed
    pub fn needs_checkpoint(&self) -> bool {
        self.utilization() >= 0.85
    }

    /// Optimize tool result based on tool type and budget
    pub async fn optimize_result(&mut self, tool_name: &str, result: Value) -> Value {
        let optimized = match tool_name {
            tools::GREP_FILE => self.optimize_grep_result(result),
            tools::LIST_FILES => self.optimize_list_files_result(result),
            tools::READ_FILE => self.optimize_read_file_result(result),
            "shell" | tools::RUN_PTY_CMD | tools::UNIFIED_EXEC => {
                self.optimize_command_result(result)
            }
            _ => result,
        };

        // Estimate tokens (rough: 1 token ≈ 4 chars)
        self.history.push_back(ContextEntry {
            tool_name: tool_name.to_string(),
            result: optimized.clone(),
        });

        optimized
    }

    /// Optimize grep results - dedupe, cap to top matches, and mark overflow
    fn optimize_grep_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object()
            && let Some(matches) = obj.get("matches").and_then(|v| v.as_array())
        {
            // Deduplicate by path + line to reduce noisy repeats
            let mut seen = std::collections::HashSet::new();
            let mut deduped = Vec::with_capacity(matches.len());
            for m in matches {
                let path = m
                    .get("path")
                    .or_else(|| m.get("file"))
                    .and_then(|p| p.as_str().map(str::to_owned));
                let line = m
                    .get("line")
                    .or_else(|| m.get("line_number"))
                    .and_then(|l| l.as_i64());
                let key = (path.clone(), line);

                // Only dedupe when we have a stable anchor (path or line). Otherwise keep all.
                if path.is_some() || line.is_some() {
                    if seen.insert(key) {
                        deduped.push(m.clone());
                    }
                } else {
                    deduped.push(m.clone());
                }
            }

            let total = deduped.len();

            if total > MAX_GREP_RESULTS {
                let truncated: Vec<_> = deduped.iter().take(MAX_GREP_RESULTS).cloned().collect();
                let overflow = total - MAX_GREP_RESULTS;
                return serde_json::json!({
                    "matches": truncated,
                    "overflow": format!("[+{} more matches]", overflow),
                    "total": total,
                    "note": "Showing top 5 unique matches (by path/line)"
                });
            }

            // Keep deduped result even when under the limit to avoid repeats
            if total != matches.len() {
                return serde_json::json!({
                    "matches": deduped,
                    "total": total,
                    "note": "unique grep matches (collapsed by path/line)"
                });
            }

            return serde_json::json!({
                "matches": deduped,
                "total": total,
                "note": "grep results normalized"
            });
        }
        result
    }

    /// Optimize list_files - summarize if 50+ items
    fn optimize_list_files_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object()
            && let Some(files) = obj.get("files").and_then(|v| v.as_array())
            && files.len() > MAX_LIST_FILES
        {
            let sample: Vec<_> = files.iter().take(5).cloned().collect();
            return serde_json::json!({
                "total_files": files.len(),
                "sample": sample,
                "note": format!("Showing 5 of {} files. Use grep_file for specific patterns.", files.len())
            });
        }
        result
    }

    /// Optimize read_file - basic size limiting without token counting
    fn optimize_read_file_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object()
            && let Some(content) = obj.get("content").and_then(|v| v.as_str())
        {
            let lines: Vec<&str> = content.lines().collect();
            let is_truncated = lines.len() > MAX_FILE_LINES;

            let final_content = if is_truncated {
                // Simple line-based truncation
                lines[..MAX_FILE_LINES].join("\n")
            } else {
                content.to_string()
            };

            // Reconstruct the object to ensure consistent field ordering and presence
            let mut standardized_obj = serde_json::Map::new();
            standardized_obj.insert("success".to_string(), json!(true));

            if let Some(status) = obj.get("status") {
                standardized_obj.insert("status".to_string(), status.clone());
            } else {
                standardized_obj.insert("status".to_string(), json!("success"));
            }

            if let Some(message) = obj.get("message") {
                standardized_obj.insert("message".to_string(), message.clone());
            }

            // Always put content
            standardized_obj.insert("content".to_string(), json!(final_content));

            if let Some(path) = obj.get("path").or_else(|| obj.get("file")) {
                standardized_obj.insert("path".to_string(), path.clone());
            }

            if let Some(metadata) = obj.get("metadata") {
                standardized_obj.insert("metadata".to_string(), metadata.clone());
            }

            if is_truncated {
                standardized_obj.insert("is_truncated".to_string(), json!(true));
            }

            return Value::Object(standardized_obj);
        }

        result
    }

    /// Optimize command output - extract errors only
    /// Optimize command output - extract errors only
    fn optimize_command_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object() {
            let stream_key = if obj.get("stdout").and_then(|v| v.as_str()).is_some() {
                "stdout"
            } else {
                "output"
            };
            if let Some(stdout) = obj.get(stream_key).and_then(|v| v.as_str()) {
                let lines: Vec<&str> = stdout.lines().collect();
                let is_truncated = lines.len() > MAX_FILE_LINES;

                if is_truncated {
                    let truncated = lines[..MAX_FILE_LINES].join("\n");
                    let lines_count = stdout.lines().count();

                    // Clone the original object to preserve exit_code, stderr, etc.
                    let mut new_obj = obj.clone();
                    new_obj.insert(stream_key.to_string(), json!(truncated));
                    new_obj.insert("is_truncated".to_string(), json!(true));
                    new_obj.insert("original_lines".to_string(), json!(lines_count));
                    new_obj.insert(
                    "note".to_string(),
                    json!(
                        "Output truncated. Use 'grep_file' or specific commands to search content."
                    ),
                );

                    return Value::Object(new_obj);
                }
            }
        }
        result
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
    use serde_json::json;

    #[tokio::test]
    async fn test_grep_optimization() {
        let mut optimizer = ContextOptimizer::new();

        let matches: Vec<_> = (0..20)
            .map(|i| json!({"line": i, "path": "src/main.rs", "text": "match"}))
            .collect();
        let result = json!({"matches": matches});

        let optimized = optimizer.optimize_result(tools::GREP_FILE, result).await;

        let opt_matches = optimized["matches"].as_array().unwrap();
        assert_eq!(opt_matches.len(), MAX_GREP_RESULTS);
        assert!(optimized["overflow"].is_string());
    }

    #[tokio::test]
    async fn test_grep_deduplicates_by_path_and_line() {
        let mut optimizer = ContextOptimizer::new();
        let matches = vec![
            json!({"line": 10, "path": "src/lib.rs", "text": "hit A"}),
            json!({"line": 10, "path": "src/lib.rs", "text": "hit A duplicate"}),
            json!({"line": 20, "path": "src/lib.rs", "text": "hit B"}),
        ];
        let result = json!({"matches": matches});

        let optimized = optimizer.optimize_result(tools::GREP_FILE, result).await;
        let opt_matches = optimized["matches"].as_array().unwrap();
        assert_eq!(opt_matches.len(), 2);
        assert_eq!(optimized["total"], 2);
        assert!(optimized["note"].as_str().unwrap().contains("unique"));
    }

    #[tokio::test]
    async fn test_list_files_optimization() {
        let mut optimizer = ContextOptimizer::new();

        let files: Vec<_> = (0..100).map(|i| json!(format!("file{}.rs", i))).collect();
        let result = json!({"files": files});

        let optimized = optimizer.optimize_result(tools::LIST_FILES, result).await;

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

        // Cleanup
        let _ = std::fs::remove_file(&temp_path);
    }

    #[tokio::test]
    async fn test_unified_exec_output_optimization_with_output_field() {
        let mut optimizer = ContextOptimizer::new();
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

        let optimized = optimizer.optimize_result(tools::UNIFIED_EXEC, result).await;
        assert_eq!(optimized["is_truncated"], true);
        assert!(optimized["output"].as_str().unwrap().lines().count() <= MAX_FILE_LINES);
    }
}
