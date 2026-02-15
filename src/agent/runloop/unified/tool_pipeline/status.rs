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
