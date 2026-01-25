use anyhow::Error;
use serde_json::Value;
use vtcode_core::tools::registry::ToolExecutionError;

/// Status of a tool execution with progress information
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ToolProgress {
    /// Current progress value (0-100)
    pub progress: u8,
    /// Status message
    pub message: String,
}

/// Result of a tool execution
#[derive(Debug)]
#[allow(dead_code)]
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
    /// Tool execution progress update
    Progress(ToolProgress),
}

/// Outcome produced by a tool pipeline run - returns a success/failure wrapper along with stdout and modified files
#[allow(dead_code)]
pub(crate) struct ToolPipelineOutcome {
    pub status: ToolExecutionStatus,
    pub stdout: Option<String>,
    pub modified_files: Vec<String>,
    pub command_success: bool,
    pub has_more: bool,
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
                // Clone for top-level fields, move originals into nested status
                // This avoids double-cloning the same data
                let stdout_copy = stdout.clone();
                let modified_files_copy = modified_files.clone();
                ToolPipelineOutcome {
                    status: ToolExecutionStatus::Success {
                        output,
                        stdout,
                        modified_files,
                        command_success,
                        has_more,
                    },
                    stdout: stdout_copy,
                    modified_files: modified_files_copy,
                    command_success,
                    has_more,
                }
            }
            other => ToolPipelineOutcome {
                status: other,
                stdout: None,
                modified_files: vec![],
                command_success: false,
                has_more: false,
            },
        }
    }
}
