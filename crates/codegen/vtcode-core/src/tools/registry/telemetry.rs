//! Telemetry events for tool operations
//!
//! This module defines telemetry events emitted by the tool registry to track
//! operational metrics, fallback sequences, and potential issues during tool
//! execution.

use std::time::Duration;

/// Telemetry events emitted during tool execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTelemetryEvent {
    /// A tool execution was attempted
    ToolExecutionStarted { tool_name: String, has_args: bool },

    /// A tool execution completed successfully
    ToolExecutionCompleted {
        tool_name: String,
        duration: Duration,
        output_size_bytes: usize,
    },

    /// A tool execution failed
    ToolExecutionFailed {
        tool_name: String,
        error_type: String,
        duration: Duration,
    },

    /// A tool fell back to a different implementation or strategy
    ///
    /// This is particularly important for detecting cascading delete/recreate
    /// sequences as identified in the Codex issue review.
    ToolFallbackDetected {
        /// The original tool that was attempted
        from_tool: String,
        /// The fallback tool or strategy used
        to_tool: String,
        /// Reason for the fallback
        reason: String,
        /// Optional file path affected by the fallback
        affected_file: Option<String>,
    },

    /// A tool operation was blocked by policy
    ToolBlocked {
        tool_name: String,
        policy_reason: String,
    },

    /// A tool operation exceeded its timeout threshold
    ToolTimeoutWarning {
        tool_name: String,
        elapsed: Duration,
        ceiling: Duration,
        percentage: u8,
    },

    /// A destructive operation is about to be performed
    DestructiveOperationWarning {
        tool_name: String,
        operation_type: String,
        affected_files: Vec<String>,
        has_backup: bool,
    },
}

impl ToolTelemetryEvent {
    /// Create a tool fallback event for edit_file -> apply_patch transition
    pub fn edit_to_patch_fallback(file_path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ToolFallbackDetected {
            from_tool: "edit_file".to_string(),
            to_tool: "apply_patch".to_string(),
            reason: reason.into(),
            affected_file: Some(file_path.into()),
        }
    }

    /// Create a destructive operation warning for delete-and-recreate patterns
    pub fn delete_and_recreate_warning(
        tool_name: impl Into<String>,
        files: Vec<String>,
        has_backup: bool,
    ) -> Self {
        Self::DestructiveOperationWarning {
            tool_name: tool_name.into(),
            operation_type: "delete_and_recreate".to_string(),
            affected_files: files,
            has_backup,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_fallback_event() {
        let event = ToolTelemetryEvent::edit_to_patch_fallback("test.rs", "pattern_not_found");

        match event {
            ToolTelemetryEvent::ToolFallbackDetected {
                from_tool,
                to_tool,
                reason,
                affected_file,
            } => {
                assert_eq!(from_tool, "edit_file");
                assert_eq!(to_tool, "apply_patch");
                assert_eq!(reason, "pattern_not_found");
                assert_eq!(affected_file, Some("test.rs".to_string()));
            }
            _ => panic!("Expected ToolFallbackDetected event"),
        }
    }

    #[test]
    fn test_create_destructive_warning() {
        let event = ToolTelemetryEvent::delete_and_recreate_warning(
            "apply_patch",
            vec!["file1.rs".to_string(), "file2.rs".to_string()],
            false,
        );

        match event {
            ToolTelemetryEvent::DestructiveOperationWarning {
                tool_name,
                operation_type,
                affected_files,
                has_backup,
            } => {
                assert_eq!(tool_name, "apply_patch");
                assert_eq!(operation_type, "delete_and_recreate");
                assert_eq!(affected_files.len(), 2);
                assert!(!has_backup);
            }
            _ => panic!("Expected DestructiveOperationWarning event"),
        }
    }
}
