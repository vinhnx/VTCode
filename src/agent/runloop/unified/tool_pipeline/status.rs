use anyhow::Error;
use serde_json::Value;
use vtcode_core::tools::registry::ToolExecutionError;

/// Result of a tool execution
#[derive(Debug)]
pub(crate) enum ToolExecutionStatus {
    /// Tool completed
    Success {
        /// Tool output
        output: Value,
        /// Standard output if available
        stdout: Option<String>,
        /// List of modified files
        modified_files: Vec<String>,
        /// Whether the command was successful
        command_success: bool,
        /// Whether there are more results available
        has_more: bool,
    },
    /// Tool execution failed
    Failure {
        /// Error that occurred
        error: Error,
    },
    /// Tool execution timed out
    Timeout {
        /// Timeout error
        error: ToolExecutionError,
    },
    /// Tool execution was cancelled
    Cancelled,
    // TODO: Progress variant planned for streaming tool progress updates
}

impl ToolExecutionStatus {
    /// Returns `true` if this status represents a successful execution.
    pub(crate) fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns `true` if this status represents any kind of failure
    /// (error, timeout, or cancellation).
    #[allow(dead_code)]
    pub(crate) fn is_failure(&self) -> bool {
        !self.is_success()
    }
}

/// Outcome produced by a tool pipeline run - returns a success/failure wrapper along with stdout and modified files
pub(crate) struct ToolPipelineOutcome {
    pub status: ToolExecutionStatus,
    pub modified_files: Vec<String>,
    pub command_success: bool,
}

impl ToolPipelineOutcome {
    pub(crate) fn from_status(status: ToolExecutionStatus) -> Self {
        match status {
            ToolExecutionStatus::Success {
                output,
                stdout,
                modified_files,
                command_success,
                has_more,
            } => {
                let modified_files_copy = modified_files.clone();
                ToolPipelineOutcome {
                    status: ToolExecutionStatus::Success {
                        output,
                        stdout,
                        modified_files,
                        command_success,
                        has_more,
                    },
                    modified_files: modified_files_copy,
                    command_success,
                }
            }
            other => ToolPipelineOutcome {
                status: other,
                modified_files: vec![],
                command_success: false,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Batch outcome tracking
// ---------------------------------------------------------------------------

/// Summary of a batch of tool executions (one LLM turn may request multiple
/// tool calls). Tracks per-tool outcomes so the agent loop can detect partial
/// failures and inject recovery context.
#[derive(Debug)]
pub(crate) struct ToolBatchOutcome {
    /// Per-tool results, ordered to match the original batch request order.
    pub entries: Vec<ToolBatchEntry>,
}

/// A single entry inside a [`ToolBatchOutcome`].
#[derive(Debug)]
pub(crate) struct ToolBatchEntry {
    /// Tool name (e.g. `"read_file"`, `"mcp_github_create_issue"`)
    pub tool_name: String,
    /// The provider-assigned call id.
    pub call_id: String,
    /// High-level result category.
    pub result: ToolBatchResult,
}

/// Simplified result for aggregation — avoids carrying the full
/// [`ToolExecutionStatus`] payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolBatchResult {
    Success,
    Failure,
    Timeout,
    Cancelled,
}

impl From<&ToolExecutionStatus> for ToolBatchResult {
    fn from(status: &ToolExecutionStatus) -> Self {
        match status {
            ToolExecutionStatus::Success { .. } => Self::Success,
            ToolExecutionStatus::Failure { .. } => Self::Failure,
            ToolExecutionStatus::Timeout { .. } => Self::Timeout,
            ToolExecutionStatus::Cancelled => Self::Cancelled,
        }
    }
}

/// Aggregate statistics for a [`ToolBatchOutcome`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolBatchStats {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub cancelled: usize,
}

impl ToolBatchOutcome {
    /// Create a new empty batch.
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Record the result of one tool call.
    pub(crate) fn record(
        &mut self,
        tool_name: impl Into<String>,
        call_id: impl Into<String>,
        status: &ToolExecutionStatus,
    ) {
        self.entries.push(ToolBatchEntry {
            tool_name: tool_name.into(),
            call_id: call_id.into(),
            result: ToolBatchResult::from(status),
        });
    }

    /// Compute aggregate statistics.
    pub(crate) fn stats(&self) -> ToolBatchStats {
        let mut s = ToolBatchStats {
            total: self.entries.len(),
            succeeded: 0,
            failed: 0,
            timed_out: 0,
            cancelled: 0,
        };
        for entry in &self.entries {
            match entry.result {
                ToolBatchResult::Success => s.succeeded += 1,
                ToolBatchResult::Failure => s.failed += 1,
                ToolBatchResult::Timeout => s.timed_out += 1,
                ToolBatchResult::Cancelled => s.cancelled += 1,
            }
        }
        s
    }

    /// Returns `true` when **some** tools succeeded but **some** failed.
    pub(crate) fn is_partial_success(&self) -> bool {
        let s = self.stats();
        s.succeeded > 0 && (s.failed > 0 || s.timed_out > 0)
    }

    /// Build a compact one-line summary suitable for structured logging.
    pub(crate) fn summary_line(&self) -> String {
        let s = self.stats();
        if s.total == 0 {
            return "no tools executed".to_string();
        }
        if s.succeeded == s.total {
            return format!("all {} tools succeeded", s.total);
        }
        let mut parts = Vec::new();
        if s.succeeded > 0 {
            parts.push(format!("{} succeeded", s.succeeded));
        }
        if s.failed > 0 {
            parts.push(format!("{} failed", s.failed));
        }
        if s.timed_out > 0 {
            parts.push(format!("{} timed out", s.timed_out));
        }
        if s.cancelled > 0 {
            parts.push(format!("{} cancelled", s.cancelled));
        }
        format!("{}/{} tools: {}", s.total, s.total, parts.join(", "))
    }

    /// Names of tools that failed or timed out.
    #[allow(dead_code)]
    pub(crate) fn failed_tool_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| {
                matches!(
                    e.result,
                    ToolBatchResult::Failure | ToolBatchResult::Timeout
                )
            })
            .map(|e| e.tool_name.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_batch_stats() {
        let batch = ToolBatchOutcome::new();
        let s = batch.stats();
        assert_eq!(s.total, 0);
        assert!(!batch.is_partial_success());
        assert_eq!(batch.summary_line(), "no tools executed");
    }

    #[test]
    fn all_success_batch() {
        let mut batch = ToolBatchOutcome::new();
        let success = ToolExecutionStatus::Success {
            output: serde_json::json!("ok"),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
        };
        batch.record("read_file", "c1", &success);
        batch.record("list_files", "c2", &success);

        let s = batch.stats();
        assert_eq!(s.total, 2);
        assert_eq!(s.succeeded, 2);
        assert!(!batch.is_partial_success());
        assert!(batch.summary_line().contains("all 2 tools succeeded"));
    }

    #[test]
    fn partial_failure_batch() {
        let mut batch = ToolBatchOutcome::new();
        let success = ToolExecutionStatus::Success {
            output: serde_json::json!("ok"),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
        };
        let failure = ToolExecutionStatus::Failure {
            error: anyhow::anyhow!("permission denied"),
        };
        batch.record("read_file", "c1", &success);
        batch.record("write_file", "c2", &failure);

        assert!(batch.is_partial_success());
        let s = batch.stats();
        assert_eq!(s.succeeded, 1);
        assert_eq!(s.failed, 1);
        assert!(batch.summary_line().contains("1 succeeded"));
        assert!(batch.summary_line().contains("1 failed"));
    }

    #[test]
    fn all_failure_not_partial_success() {
        let mut batch = ToolBatchOutcome::new();
        let failure = ToolExecutionStatus::Failure {
            error: anyhow::anyhow!("boom"),
        };
        batch.record("write_file", "c1", &failure);
        assert!(!batch.is_partial_success());
    }

    #[test]
    fn timeout_entry_tracked() {
        let mut batch = ToolBatchOutcome::new();
        let timeout = ToolExecutionStatus::Timeout {
            error: ToolExecutionError::new(
                "slow_tool".to_string(),
                vtcode_core::tools::registry::ToolErrorType::Timeout,
                "timed out after 30s".to_string(),
            ),
        };
        batch.record("slow_tool", "c1", &timeout);
        let s = batch.stats();
        assert_eq!(s.timed_out, 1);
    }

    #[test]
    fn failed_tool_names_returns_correct_names() {
        let mut batch = ToolBatchOutcome::new();
        let success = ToolExecutionStatus::Success {
            output: serde_json::json!("ok"),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
        };
        let failure = ToolExecutionStatus::Failure {
            error: anyhow::anyhow!("nope"),
        };
        batch.record("good_tool", "c1", &success);
        batch.record("bad_tool", "c2", &failure);

        let failed = batch.failed_tool_names();
        assert_eq!(failed, vec!["bad_tool"]);
    }
}
