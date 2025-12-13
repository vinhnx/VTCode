//! Context optimization for efficient token usage
//!
//! Implements context engineering principles from AGENTS.md:
//! - Per-tool output curation (max 5 grep results, summarize 50+ files)
//! - Token budget awareness (90%/95% thresholds)
//! - Semantic context over volume
//! - Progressive compaction

use crate::config::constants::tools;
use crate::core::token_budget::{TokenBudgetManager, TokenUsageStats};
use crate::core::token_constants::{THRESHOLD_CHECKPOINT, THRESHOLD_COMPACT};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

// Re-export unified constants for backward compatibility and clarity
/// Compact threshold: Begin compacting at 90% usage
pub const COMPACT_THRESHOLD: f64 = THRESHOLD_COMPACT;

/// Checkpoint threshold: Create checkpoint at 95% usage
pub const CHECKPOINT_THRESHOLD: f64 = THRESHOLD_CHECKPOINT;

/// Maximum results to show per tool
const MAX_GREP_RESULTS: usize = 5;
const MAX_LIST_FILES: usize = 50;
const MAX_FILE_LINES: usize = 2000;

/// Compact mode levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactMode {
    Normal,     // <90%
    Compact,    // 90-95%
    Checkpoint, // >95%
}

/// Checkpoint state for context reset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointState {
    pub task_description: String,
    pub completed_steps: Vec<String>,
    pub current_work: String,
    pub next_steps: Vec<String>,
    pub token_usage: TokenUsageStats,
    pub key_files: Vec<String>,
    pub timestamp: u64,
}

/// Context optimization manager
pub struct ContextOptimizer {
    token_budget: Option<Arc<TokenBudgetManager>>,
    history: VecDeque<ContextEntry>,
    compact_mode: CompactMode,
}

#[derive(Debug, Clone)]
struct ContextEntry {
    tool_name: String,
    result: Value,
    tokens: usize,
    compacted: bool,
}

impl ContextOptimizer {
    /// Create a new context optimizer without token budget integration
    pub fn new() -> Self {
        Self {
            token_budget: None,
            history: VecDeque::new(),
            compact_mode: CompactMode::Normal,
        }
    }

    /// Create a new context optimizer with token budget integration
    pub fn with_token_budget(token_budget: Arc<TokenBudgetManager>) -> Self {
        Self {
            token_budget: Some(token_budget),
            history: VecDeque::new(),
            compact_mode: CompactMode::Normal,
        }
    }

    /// Get current budget utilization (0.0 to 1.0)
    pub async fn utilization(&self) -> f64 {
        if let Some(ref budget) = self.token_budget {
            budget.usage_ratio().await
        } else {
            0.0
        }
    }

    /// Get a cloned token budget handle for external async use
    pub fn token_budget(&self) -> Option<Arc<TokenBudgetManager>> {
        self.token_budget.clone()
    }

    /// Check if checkpoint is needed
    pub async fn needs_checkpoint(&self) -> bool {
        self.utilization().await >= CHECKPOINT_THRESHOLD
    }

    /// Check if compaction is needed
    pub async fn needs_compaction(&self) -> bool {
        self.utilization().await >= COMPACT_THRESHOLD
    }

    /// Get current compact mode
    pub fn compact_mode(&self) -> CompactMode {
        self.compact_mode
    }

    /// Update compact mode based on current utilization
    pub async fn update_compact_mode(&mut self) {
        let util = self.utilization().await;
        self.compact_mode = if util >= CHECKPOINT_THRESHOLD {
            CompactMode::Checkpoint
        } else if util >= COMPACT_THRESHOLD {
            CompactMode::Compact
        } else {
            CompactMode::Normal
        };
    }

    /// Optimize tool result based on tool type and budget
    pub async fn optimize_result(&mut self, tool_name: &str, result: Value) -> Value {
        let optimized = match tool_name {
            tools::GREP_FILE => self.optimize_grep_result(result),
            tools::LIST_FILES => self.optimize_list_files_result(result),
            tools::READ_FILE => self.optimize_read_file_result(result),
            "shell" | tools::RUN_PTY_CMD => self.optimize_command_result(result),
            _ => result,
        };

        // Estimate tokens (rough: 1 token ≈ 4 chars)
        let tokens = optimized.to_string().len() / 4;

        self.history.push_back(ContextEntry {
            tool_name: tool_name.to_string(),
            result: optimized.clone(),
            tokens,
            compacted: false,
        });

        // Update compact mode based on current utilization
        self.update_compact_mode().await;

        // Auto-compact if needed
        if self.needs_compaction().await {
            self.compact_history();
        }

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

    /// Optimize read_file - truncate based on max_tokens parameter or default limits
    fn optimize_read_file_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object()
            && let Some(content) = obj.get("content").and_then(|v| v.as_str())
        {
            let max_file_lines = MAX_FILE_LINES;

            // Calculate exact token count using tokenizer
            let estimated_tokens = self.count_tokens(content);

            // Check if max_tokens was specified in the result
            let max_tokens = obj
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .or_else(|| {
                    obj.get("metadata")
                        .and_then(|m| m.get("applied_max_tokens"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize)
                });

            let should_truncate = if let Some(max_tok) = max_tokens {
                estimated_tokens > max_tok
            } else {
                let lines: Vec<&str> = content.lines().collect();
                lines.len() > max_file_lines
            };

            let (final_content, is_truncated) = if should_truncate {
                // If we have a max_tokens limit, use it for smarter truncation
                // Default to ~8000 tokens (2000 lines * 4) if not specified
                let token_limit = max_tokens.unwrap_or(MAX_FILE_LINES * 4);
                
                // Use token-based truncation if possible (more accurate)
                // Otherwise fall back to line-based
                let truncated = self.truncate_content(content, token_limit);
                (truncated, true)
            } else {
                (content.to_string(), false)
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
                standardized_obj.insert("original_tokens".to_string(), json!(estimated_tokens));
                
                if let Some(omitted) = obj.get("omitted_line_count") {
                    standardized_obj.insert("omitted_line_count".to_string(), omitted.clone());
                }
            }

            return Value::Object(standardized_obj);
        }

        result
    }

    /// Estimate tokens (rough approximation)
    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }

    /// Truncate content while preserving line boundaries if possible
    fn truncate_content(&self, content: &str, token_limit: usize) -> String {
        let char_limit = token_limit * 4;
        if content.len() <= char_limit {
            return content.to_string();
        }
        
        let truncated = &content[..char_limit];
        // Try to cut at last newline to avoid partial lines
        if let Some(last_newline) = truncated.rfind('\n') {
            truncated[..last_newline].to_string()
        } else {
            truncated.to_string()
        }
    }

    /// Optimize command output - extract errors only
    /// Optimize command output - extract errors only
    fn optimize_command_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object()
            && let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str())
        {
            // Use same limit as files (approx 2000 lines / 8000 tokens)
            let max_tokens = MAX_FILE_LINES * 4;
            let current_tokens = self.count_tokens(stdout);

            if current_tokens > max_tokens {
                let truncated = self.truncate_content(stdout, max_tokens);
                let lines_count = stdout.lines().count();

                // Clone the original object to preserve exit_code, stderr, etc.
                let mut new_obj = obj.clone();
                new_obj.insert("stdout".to_string(), json!(truncated));
                new_obj.insert("is_truncated".to_string(), json!(true));
                new_obj.insert("original_lines".to_string(), json!(lines_count));
                new_obj.insert("original_tokens".to_string(), json!(current_tokens));
                new_obj.insert("note".to_string(), json!("Output truncated. Use 'grep_file' or specific commands to search content."));

                return Value::Object(new_obj);
            }
        }
        result
    }

    /// Compact history to free up tokens while preserving critical information
    /// Preserves: file paths, line numbers, error messages
    fn compact_history(&mut self) {
        // Compact oldest entries first
        for entry in self.history.iter_mut() {
            if entry.compacted {
                continue;
            }

            let compacted = match entry.tool_name.as_str() {
                tools::GREP_FILE => {
                    // Preserve file paths and line numbers from grep results
                    Self::compact_grep_entry(&entry.result)
                }
                tools::LIST_FILES => {
                    // Preserve file paths and counts
                    Self::compact_list_files_entry(&entry.result)
                }
                tools::READ_FILE => {
                    // Preserve file path and line range
                    Self::compact_read_file_entry(&entry.result)
                }
                "shell" | tools::RUN_PTY_CMD => {
                    // Preserve error messages and exit codes
                    Self::compact_command_entry(&entry.result)
                }
                _ => {
                    // Generic compaction - preserve any error fields
                    Self::compact_generic_entry(&entry.result, &entry.tool_name)
                }
            };

            let new_tokens = compacted.to_string().len() / 4;

            entry.result = compacted;
            entry.tokens = new_tokens;
            entry.compacted = true;
        }
    }

    /// Compact grep entry while preserving paths and line numbers
    fn compact_grep_entry(result: &Value) -> Value {
        if let Some(obj) = result.as_object() {
            let mut preserved = serde_json::Map::new();
            preserved.insert("tool".to_string(), json!(tools::GREP_FILE));

            // Preserve file paths and line numbers
            if let Some(matches) = obj.get("matches").and_then(|v| v.as_array()) {
                let paths_and_lines: Vec<_> = matches
                    .iter()
                    .filter_map(|m| {
                        let path = m.get("path").or_else(|| m.get("file"))?;
                        let line = m.get("line").or_else(|| m.get("line_number"));
                        Some(json!({
                            "path": path,
                            "line": line
                        }))
                    })
                    .collect();
                preserved.insert("matches".to_string(), json!(paths_and_lines));
            }

            if let Some(total) = obj.get("total") {
                preserved.insert("total".to_string(), total.clone());
            }

            preserved.insert(
                "note".to_string(),
                json!("Grep results compacted - paths and line numbers preserved"),
            );
            return Value::Object(preserved);
        }
        json!({"tool": tools::GREP_FILE, "note": "Compacted"})
    }

    /// Compact list_files entry while preserving paths
    fn compact_list_files_entry(result: &Value) -> Value {
        if let Some(obj) = result.as_object() {
            let mut preserved = serde_json::Map::new();
            preserved.insert("tool".to_string(), json!(tools::LIST_FILES));

            // Preserve total count and sample of paths
            if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
                preserved.insert("total_files".to_string(), json!(files.len()));
                let sample: Vec<_> = files.iter().take(3).cloned().collect();
                preserved.insert("sample_paths".to_string(), json!(sample));
            } else if let Some(total) = obj.get("total_files") {
                preserved.insert("total_files".to_string(), total.clone());
                if let Some(sample) = obj.get("sample") {
                    preserved.insert("sample_paths".to_string(), sample.clone());
                }
            }

            preserved.insert(
                "note".to_string(),
                json!("File list compacted - count and sample preserved"),
            );
            return Value::Object(preserved);
        }
        json!({"tool": tools::LIST_FILES, "note": "Compacted"})
    }

    /// Compact read_file entry while preserving path and line range
    fn compact_read_file_entry(result: &Value) -> Value {
        if let Some(obj) = result.as_object() {
            let mut preserved = serde_json::Map::new();
            preserved.insert("tool".to_string(), json!(tools::READ_FILE));

            // Preserve file path
            if let Some(path) = obj.get("path").or_else(|| obj.get("file")) {
                preserved.insert("path".to_string(), path.clone());
            }

            // Preserve line range information
            if let Some(start) = obj.get("start_line") {
                preserved.insert("start_line".to_string(), start.clone());
            }
            if let Some(end) = obj.get("end_line") {
                preserved.insert("end_line".to_string(), end.clone());
            }
            if let Some(total) = obj.get("total_lines") {
                preserved.insert("total_lines".to_string(), total.clone());
            }

            preserved.insert(
                "note".to_string(),
                json!("File content compacted - path and line range preserved"),
            );
            return Value::Object(preserved);
        }
        json!({"tool": tools::READ_FILE, "note": "Compacted"})
    }

    /// Compact command entry while preserving errors
    fn compact_command_entry(result: &Value) -> Value {
        if let Some(obj) = result.as_object() {
            let mut preserved = serde_json::Map::new();
            preserved.insert("tool".to_string(), json!("command"));

            // Preserve exit code
            if let Some(exit_code) = obj.get("exit_code").or_else(|| obj.get("code")) {
                preserved.insert("exit_code".to_string(), exit_code.clone());
            }

            // Preserve error messages
            if let Some(stderr) = obj.get("stderr").and_then(|v| v.as_str())
                && !stderr.is_empty()
            {
                // Keep first 200 chars of stderr
                let truncated = if stderr.len() > 200 {
                    format!("{}...", &stderr[..200])
                } else {
                    stderr.to_string()
                };
                preserved.insert("stderr".to_string(), json!(truncated));
            }

            // Preserve error lines from stdout
            if let Some(errors) = obj.get("errors") {
                preserved.insert("errors".to_string(), errors.clone());
            }

            preserved.insert(
                "note".to_string(),
                json!("Command output compacted - errors preserved"),
            );
            return Value::Object(preserved);
        }
        json!({"tool": "command", "note": "Compacted"})
    }

    /// Compact generic entry while preserving error fields
    fn compact_generic_entry(result: &Value, tool_name: &str) -> Value {
        if let Some(obj) = result.as_object() {
            let mut preserved = serde_json::Map::new();
            preserved.insert("tool".to_string(), json!(tool_name));

            // Preserve any error-related fields
            for key in [
                "error",
                "errors",
                "error_message",
                "stderr",
                "exit_code",
                "status",
            ] {
                if let Some(value) = obj.get(key) {
                    preserved.insert(key.to_string(), value.clone());
                }
            }

            // Preserve any path-related fields
            for key in ["path", "file", "files", "directory"] {
                if let Some(value) = obj.get(key) {
                    preserved.insert(key.to_string(), value.clone());
                }
            }

            preserved.insert(
                "note".to_string(),
                json!("Output compacted - errors and paths preserved"),
            );
            return Value::Object(preserved);
        }
        json!({"tool": tool_name, "note": "Compacted"})
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
        let token_usage = if let Some(ref budget) = self.token_budget {
            budget.get_stats().await
        } else {
            crate::core::token_budget::TokenUsageStats::new()
        };

        CheckpointState {
            task_description,
            completed_steps,
            current_work,
            next_steps,
            token_usage,
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
        let util = self.utilization().await;
        if util >= CHECKPOINT_THRESHOLD {
            format!(
                "[WARN] Token budget at {:.1}% - checkpoint recommended",
                util * 100.0
            )
        } else if util >= COMPACT_THRESHOLD {
            format!(
                "[WARN] Token budget at {:.1}% - compaction active",
                util * 100.0
            )
        } else {
            format!("[OK] Token budget at {:.1}%", util * 100.0)
        }
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
    use crate::core::token_budget::TokenBudgetConfig;
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
    async fn test_budget_thresholds_with_token_budget() {
        let mut config = TokenBudgetConfig::default();
        config.max_context_tokens = 1000;
        let budget = Arc::new(TokenBudgetManager::new(config));

        let optimizer = ContextOptimizer::with_token_budget(budget.clone());

        assert!(!optimizer.needs_compaction().await);

        // Simulate 90% usage
        budget
            .record_tokens_for_component(
                crate::core::token_budget::ContextComponent::UserMessage,
                900,
                None,
            )
            .await;
        assert!(optimizer.needs_compaction().await);
        assert!(!optimizer.needs_checkpoint().await);

        // Simulate 95% usage
        budget
            .record_tokens_for_component(
                crate::core::token_budget::ContextComponent::UserMessage,
                50,
                None,
            )
            .await;
        assert!(optimizer.needs_checkpoint().await);
    }

    #[tokio::test]
    async fn test_compact_mode_updates() {
        let mut config = TokenBudgetConfig::default();
        config.max_context_tokens = 1000;
        let budget = Arc::new(TokenBudgetManager::new(config));

        let mut optimizer = ContextOptimizer::with_token_budget(budget.clone());

        assert_eq!(optimizer.compact_mode(), CompactMode::Normal);

        // Simulate 90% usage
        budget
            .record_tokens_for_component(
                crate::core::token_budget::ContextComponent::UserMessage,
                900,
                None,
            )
            .await;
        optimizer.update_compact_mode().await;
        assert_eq!(optimizer.compact_mode(), CompactMode::Compact);

        // Simulate 95% usage
        budget
            .record_tokens_for_component(
                crate::core::token_budget::ContextComponent::UserMessage,
                50,
                None,
            )
            .await;
        optimizer.update_compact_mode().await;
        assert_eq!(optimizer.compact_mode(), CompactMode::Checkpoint);
    }

    #[tokio::test]
    async fn test_checkpoint_creation() {
        let config = TokenBudgetConfig::default();
        let budget = Arc::new(TokenBudgetManager::new(config));
        let optimizer = ContextOptimizer::with_token_budget(budget);

        let checkpoint = optimizer
            .create_checkpoint(
                "Test task".to_string(),
                vec!["Step 1".to_string(), "Step 2".to_string()],
                "Current work".to_string(),
                vec!["Next step".to_string()],
                vec!["file1.rs".to_string()],
            )
            .await;

        assert_eq!(checkpoint.task_description, "Test task");
        assert_eq!(checkpoint.completed_steps.len(), 2);
        assert_eq!(checkpoint.current_work, "Current work");
        assert_eq!(checkpoint.next_steps.len(), 1);
        assert_eq!(checkpoint.key_files.len(), 1);
    }

    #[tokio::test]
    async fn test_checkpoint_save_load() {
        let config = TokenBudgetConfig::default();
        let budget = Arc::new(TokenBudgetManager::new(config));
        let optimizer = ContextOptimizer::with_token_budget(budget);

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
    async fn test_read_file_token_based_truncation() {
        let mut optimizer = ContextOptimizer::new();

        // Create a large file content
        let large_content = "line\n".repeat(5000);
        let result = json!({
            "content": large_content,
            "max_tokens": 1000
        });

        let optimized = optimizer.optimize_result(tools::READ_FILE, result).await;

        assert!(optimized["truncated"].as_bool().unwrap_or(false));
        assert!(optimized["estimated_tokens"].is_number());
        assert_eq!(optimized["max_tokens"], 1000);
    }

    #[tokio::test]
    async fn test_read_file_truncation_preserves_status() {
        let mut optimizer = ContextOptimizer::new();

        // Create a large file content to force truncation
        let large_content = "line\n".repeat(5000);
        let result = json!({
            "content": large_content,
            "max_tokens": 100, // Force truncation
            "status": "success",
            "message": "Successfully read file"
        });

        let optimized = optimizer.optimize_result(tools::READ_FILE, result).await;

        // Verify truncation happened
        assert!(optimized["is_truncated"].as_bool().unwrap());
        
        // Verify status/message preserved
        assert_eq!(optimized["status"], "success");
        assert_eq!(optimized["message"], "Successfully read file");
    }

    #[tokio::test]
    async fn test_history_compaction_preserves_paths() {
        let mut optimizer = ContextOptimizer::new();

        // Add grep result with paths and line numbers
        let grep_result = json!({
            "matches": [
                {"path": "src/main.rs", "line": 42, "text": "error"},
                {"path": "src/lib.rs", "line": 100, "text": "error"}
            ],
            "total": 2
        });

        optimizer
            .optimize_result(tools::GREP_FILE, grep_result)
            .await;

        // Trigger compaction
        optimizer.compact_history();

        // Check that paths and line numbers are preserved
        let compacted = &optimizer.history[0].result;
        assert_eq!(compacted["tool"], tools::GREP_FILE);
        assert!(compacted["matches"].is_array());
        let matches = compacted["matches"].as_array().unwrap();
        assert_eq!(matches[0]["path"], "src/main.rs");
        assert_eq!(matches[0]["line"], 42);
    }

    #[tokio::test]
    async fn test_history_compaction_preserves_errors() {
        let mut optimizer = ContextOptimizer::new();

        // Add command result with errors
        let cmd_result = json!({
            "exit_code": 1,
            "stderr": "Error: file not found at line 42",
            "stdout": "some output"
        });

        optimizer.optimize_result("shell", cmd_result).await;

        // Trigger compaction
        optimizer.compact_history();

        // Check that errors are preserved
        let compacted = &optimizer.history[0].result;
        assert_eq!(compacted["tool"], "command");
        assert_eq!(compacted["exit_code"], 1);
        assert!(compacted["stderr"].as_str().unwrap().contains("Error"));
    }

    #[tokio::test]
    async fn test_list_files_compaction_preserves_paths() {
        let mut optimizer = ContextOptimizer::new();

        // Add list_files result
        let files: Vec<_> = (0..10).map(|i| json!(format!("file{}.rs", i))).collect();
        let result = json!({"files": files});

        optimizer.optimize_result(tools::LIST_FILES, result).await;

        // Trigger compaction
        optimizer.compact_history();

        // Check that file count and sample are preserved
        let compacted = &optimizer.history[0].result;
        assert_eq!(compacted["tool"], tools::LIST_FILES);
        assert_eq!(compacted["total_files"], 10);
        assert!(compacted["sample_paths"].is_array());
    }

    #[tokio::test]
    async fn test_read_file_compaction_preserves_line_range() {
        let mut optimizer = ContextOptimizer::new();

        // Add read_file result with line range
        let result = json!({
            "path": "src/main.rs",
            "content": "some content",
            "start_line": 10,
            "end_line": 50,
            "total_lines": 100
        });

        optimizer.optimize_result(tools::READ_FILE, result).await;

        // Trigger compaction
        optimizer.compact_history();

        // Check that path and line range are preserved
        let compacted = &optimizer.history[0].result;
        assert_eq!(compacted["tool"], tools::READ_FILE);
        assert_eq!(compacted["path"], "src/main.rs");
        assert_eq!(compacted["start_line"], 10);
        assert_eq!(compacted["end_line"], 50);
        assert_eq!(compacted["total_lines"], 100);
    }
}
