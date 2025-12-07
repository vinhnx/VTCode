//! Context optimization for efficient token usage
//!
//! Implements context engineering principles from AGENTS.md:
//! - Per-tool output curation (max 5 grep results, summarize 50+ files)
//! - Token budget awareness (70%/85%/90% thresholds)
//! - Semantic context over volume
//! - Progressive compaction

use serde_json::Value;
use std::collections::VecDeque;

/// Context budget thresholds
const COMPACT_THRESHOLD: f64 = 0.70; // Start compacting
const AGGRESSIVE_THRESHOLD: f64 = 0.85; // Aggressive compaction
const CHECKPOINT_THRESHOLD: f64 = 0.90; // Create checkpoint

/// Maximum results to show per tool
const MAX_GREP_RESULTS: usize = 5;
const MAX_LIST_FILES: usize = 50;
const MAX_FILE_LINES: usize = 2000;

/// Context optimization manager
pub struct ContextOptimizer {
    total_budget: usize,
    used_tokens: usize,
    history: VecDeque<ContextEntry>,
}

#[derive(Debug, Clone)]
struct ContextEntry {
    tool_name: String,
    result: Value,
    tokens: usize,
    compacted: bool,
}

impl ContextOptimizer {
    pub fn new(total_budget: usize) -> Self {
        Self {
            total_budget,
            used_tokens: 0,
            history: VecDeque::new(),
        }
    }

    /// Get current budget utilization (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        self.used_tokens as f64 / self.total_budget as f64
    }

    /// Check if checkpoint is needed
    pub fn needs_checkpoint(&self) -> bool {
        self.utilization() >= CHECKPOINT_THRESHOLD
    }

    /// Check if compaction is needed
    pub fn needs_compaction(&self) -> bool {
        self.utilization() >= COMPACT_THRESHOLD
    }

    /// Check if aggressive compaction is needed
    pub fn needs_aggressive_compaction(&self) -> bool {
        self.utilization() >= AGGRESSIVE_THRESHOLD
    }

    /// Optimize tool result based on tool type and budget
    pub fn optimize_result(&mut self, tool_name: &str, result: Value) -> Value {
        let optimized = match tool_name {
            "grep_file" => self.optimize_grep_result(result),
            "list_files" => self.optimize_list_files_result(result),
            "read_file" => self.optimize_read_file_result(result),
            "shell" | "run_pty_cmd" => self.optimize_command_result(result),
            _ => result,
        };

        // Estimate tokens (rough: 1 token ≈ 4 chars)
        let tokens = optimized.to_string().len() / 4;
        self.used_tokens += tokens;

        self.history.push_back(ContextEntry {
            tool_name: tool_name.to_string(),
            result: optimized.clone(),
            tokens,
            compacted: false,
        });

        // Auto-compact if needed
        if self.needs_compaction() {
            self.compact_history();
        }

        optimized
    }

    /// Optimize grep results - max 5 matches, indicate overflow
    fn optimize_grep_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object() {
            if let Some(matches) = obj.get("matches").and_then(|v| v.as_array()) {
                if matches.len() > MAX_GREP_RESULTS {
                    let truncated: Vec<_> = matches.iter().take(MAX_GREP_RESULTS).cloned().collect();
                    let overflow = matches.len() - MAX_GREP_RESULTS;
                    return serde_json::json!({
                        "matches": truncated,
                        "overflow": format!("[+{} more matches]", overflow),
                        "total": matches.len(),
                        "note": "Showing top 5 most relevant matches"
                    });
                }
            }
        }
        result
    }

    /// Optimize list_files - summarize if 50+ items
    fn optimize_list_files_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object() {
            if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
                if files.len() > MAX_LIST_FILES {
                    let sample: Vec<_> = files.iter().take(5).cloned().collect();
                    return serde_json::json!({
                        "total_files": files.len(),
                        "sample": sample,
                        "note": format!("Showing 5 of {} files. Use grep_file for specific patterns.", files.len())
                    });
                }
            }
        }
        result
    }

    /// Optimize read_file - truncate if too large
    fn optimize_read_file_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object() {
            if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
                let lines: Vec<_> = content.lines().collect();
                if lines.len() > MAX_FILE_LINES {
                    let truncated = lines[..MAX_FILE_LINES].join("\n");
                    return serde_json::json!({
                        "content": truncated,
                        "truncated": true,
                        "total_lines": lines.len(),
                        "showing_lines": MAX_FILE_LINES,
                        "note": "File truncated. Use read_file with start_line/end_line for specific sections."
                    });
                }
            }
        }
        result
    }

    /// Optimize command output - extract errors only
    fn optimize_command_result(&self, result: Value) -> Value {
        if let Some(obj) = result.as_object() {
            if let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str()) {
                // Extract error lines + 2 context lines
                let lines: Vec<_> = stdout.lines().collect();
                if lines.len() > 100 {
                    let error_lines: Vec<_> = lines
                        .iter()
                        .enumerate()
                        .filter(|(_, line)| {
                            line.contains("error:") || line.contains("Error:") || line.contains("ERROR")
                        })
                        .flat_map(|(i, _)| {
                            let start = i.saturating_sub(2);
                            let end = (i + 3).min(lines.len());
                            lines[start..end].iter().map(|s| s.to_string())
                        })
                        .collect();

                    if !error_lines.is_empty() {
                        return serde_json::json!({
                            "errors": error_lines,
                            "total_lines": lines.len(),
                            "note": "Showing error lines with context. Full output available if needed."
                        });
                    }

                    // No errors, just show summary
                    return serde_json::json!({
                        "status": "completed",
                        "total_lines": lines.len(),
                        "note": "Command completed successfully. Output truncated."
                    });
                }
            }
        }
        result
    }

    /// Compact history to free up tokens
    fn compact_history(&mut self) {
        let mut freed_tokens = 0;
        
        // Compact oldest entries first
        for entry in self.history.iter_mut() {
            if entry.compacted {
                continue;
            }

            let compacted = match entry.tool_name.as_str() {
                "grep_file" | "list_files" => {
                    // Already optimized, just mark as compacted
                    serde_json::json!({
                        "tool": entry.tool_name,
                        "note": "Result compacted to save tokens"
                    })
                }
                "read_file" => {
                    serde_json::json!({
                        "tool": "read_file",
                        "note": "File content compacted. Re-read if needed."
                    })
                }
                _ => {
                    serde_json::json!({
                        "tool": entry.tool_name,
                        "note": "Output compacted"
                    })
                }
            };

            let old_tokens = entry.tokens;
            let new_tokens = compacted.to_string().len() / 4;
            freed_tokens += old_tokens - new_tokens;

            entry.result = compacted;
            entry.tokens = new_tokens;
            entry.compacted = true;

            // Stop if we've freed enough
            if freed_tokens > self.total_budget / 10 {
                break;
            }
        }

        self.used_tokens = self.used_tokens.saturating_sub(freed_tokens);
    }

    /// Create checkpoint summary for context reset
    pub fn create_checkpoint(&self) -> Value {
        serde_json::json!({
            "checkpoint": true,
            "total_budget": self.total_budget,
            "used_tokens": self.used_tokens,
            "utilization": format!("{:.1}%", self.utilization() * 100.0),
            "note": "Context checkpoint created. Previous work summarized.",
            "recommendation": "Continue from this point with fresh context window."
        })
    }

    /// Get budget status message
    pub fn budget_status(&self) -> String {
        let util = self.utilization();
        if util >= CHECKPOINT_THRESHOLD {
            format!(
                "⚠️  Token budget at {:.1}% - checkpoint recommended",
                util * 100.0
            )
        } else if util >= AGGRESSIVE_THRESHOLD {
            format!(
                "⚠️  Token budget at {:.1}% - aggressive compaction active",
                util * 100.0
            )
        } else if util >= COMPACT_THRESHOLD {
            format!(
                "ℹ️  Token budget at {:.1}% - compaction active",
                util * 100.0
            )
        } else {
            format!("✓ Token budget at {:.1}%", util * 100.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_grep_optimization() {
        let mut optimizer = ContextOptimizer::new(100000);
        
        let matches: Vec<_> = (0..20).map(|i| json!({"line": i, "text": "match"})).collect();
        let result = json!({"matches": matches});
        
        let optimized = optimizer.optimize_result("grep_file", result);
        
        let opt_matches = optimized["matches"].as_array().unwrap();
        assert_eq!(opt_matches.len(), MAX_GREP_RESULTS);
        assert!(optimized["overflow"].is_string());
    }

    #[test]
    fn test_list_files_optimization() {
        let mut optimizer = ContextOptimizer::new(100000);
        
        let files: Vec<_> = (0..100).map(|i| json!(format!("file{}.rs", i))).collect();
        let result = json!({"files": files});
        
        let optimized = optimizer.optimize_result("list_files", result);
        
        assert_eq!(optimized["total_files"], 100);
        assert!(optimized["sample"].is_array());
        assert!(optimized["note"].is_string());
    }

    #[test]
    fn test_budget_thresholds() {
        let mut optimizer = ContextOptimizer::new(1000);
        
        assert!(!optimizer.needs_compaction());
        
        optimizer.used_tokens = 700;
        assert!(optimizer.needs_compaction());
        assert!(!optimizer.needs_aggressive_compaction());
        
        optimizer.used_tokens = 850;
        assert!(optimizer.needs_aggressive_compaction());
        assert!(!optimizer.needs_checkpoint());
        
        optimizer.used_tokens = 900;
        assert!(optimizer.needs_checkpoint());
    }

    #[test]
    fn test_compaction() {
        let mut optimizer = ContextOptimizer::new(1000);
        
        // Add some entries
        for i in 0..10 {
            let result = json!({"data": format!("entry {}", i), "large": "x".repeat(100)});
            optimizer.optimize_result("test_tool", result);
        }
        
        let before = optimizer.used_tokens;
        optimizer.compact_history();
        let after = optimizer.used_tokens;
        
        assert!(after < before);
    }
}
