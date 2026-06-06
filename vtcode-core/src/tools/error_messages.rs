/// Centralized error messages for tool operations
///
/// This module provides consistent, reusable error messages across tool implementations
/// to ensure uniformity in user-facing error reporting.
///
/// NOTE: Only actively-used message groups are retained. Submodules that were
/// defined but never referenced by tool implementations have been removed.

/// Shared execution guidance messages for agent/runloop/tool registry.
pub mod agent_execution {
    /// Marker used when plan mode blocks a mutating tool call.
    pub const PLAN_MODE_DENIED_CONTEXT: &str = "tool denied by plan mode";
    /// Prefix for loop detection failures.
    pub const LOOP_DETECTION_PREFIX: &str = "LOOP DETECTION";
    /// Canonical action-required line for loop detection blocks.
    pub const LOOP_RETRY_BLOCKED_LINE: &str = "ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.";

    /// Build the canonical plan-mode denial message.
    pub fn plan_mode_denial_message(tool_name: &str) -> String {
        format!(
            "Tool '{}' execution failed: tool denied by plan mode\n\n\
             ACTION REQUIRED: You are in Plan Mode (read-only). To start implementation:\n\
             1. Call `exit_plan_mode` tool to show the user your plan for approval\n\
             2. Wait for user to confirm (they will see the Implementation Blueprint)\n\
             3. After approval, mutating tools will be enabled\n\n\
             Fallback if automatic Plan->Edit switching keeps failing: manually switch using `/plan off` or `/mode` (or `Shift+Tab`/`Alt+M` in interactive mode).",
            tool_name
        )
    }

    /// Build the canonical loop-detection block message.
    pub fn loop_detection_block_message(
        tool_name: &str,
        repeat_count: u64,
        original_error: Option<&str>,
    ) -> String {
        let mut message = format!(
            "{}: Tool '{}' has been called {} times with identical parameters and is now blocked.\n\n\
             {}\n\n\
             If you need the result from this tool:\n\
             1. Check if you already have the result from a previous successful call in your conversation history\n\
             2. If not available, use a different approach or modify your request",
            LOOP_DETECTION_PREFIX, tool_name, repeat_count, LOOP_RETRY_BLOCKED_LINE
        );

        if let Some(error) = original_error {
            message.push_str("\n\nOriginal error: ");
            message.push_str(error);
        }

        message
    }

    /// Check whether an error string corresponds to plan mode denial.
    pub fn is_plan_mode_denial(error: &str) -> bool {
        error.contains(PLAN_MODE_DENIED_CONTEXT)
    }
}

/// Skill management error messages
pub mod skill_ops {
    pub const SKILL_NOT_FOUND: &str = "Skill not found";
    pub const SKILL_ALREADY_EXISTS: &str = "Skill already exists";
    pub const INVALID_SKILL_FORMAT: &str = "Invalid skill format";
    pub const SKILL_SAVE_FAILED: &str = "Failed to save skill";
    pub const SKILL_LOAD_FAILED: &str = "Failed to load skill";

    /// Build a formatted "skill not found" error message.
    pub fn skill_not_found_error(name: &str) -> anyhow::Error {
        anyhow::anyhow!("Skill '{}' not found", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages_are_not_empty() {
        assert!(!agent_execution::PLAN_MODE_DENIED_CONTEXT.is_empty());
        assert!(!agent_execution::LOOP_RETRY_BLOCKED_LINE.is_empty());
        assert!(!skill_ops::SKILL_NOT_FOUND.is_empty());
    }

    #[test]
    fn test_agent_execution_message_helpers() {
        let plan_mode_msg = agent_execution::plan_mode_denial_message("write_file");
        assert!(agent_execution::is_plan_mode_denial(&plan_mode_msg));
        assert!(plan_mode_msg.contains("exit_plan_mode"));
        assert!(plan_mode_msg.contains("/plan off"));
        assert!(plan_mode_msg.contains("/mode"));
        assert!(plan_mode_msg.contains("Shift+Tab"));
        assert!(!plan_mode_msg.contains("DO NOT retry this tool or use /plan off"));

        let loop_msg =
            agent_execution::loop_detection_block_message("read_file", 3, Some("base error"));
        assert!(loop_msg.contains("LOOP DETECTION"));
        assert!(loop_msg.contains("DO NOT retry"));
        assert!(loop_msg.contains("Original error: base error"));
    }
}
