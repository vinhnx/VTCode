/// Centralized error messages for tool operations.
///
/// This module provides consistent, reusable error messages across tool
/// implementations to ensure uniformity in user-facing error reporting.
///
/// Only actively-used message groups are retained. Submodules that were
/// defined but never referenced by tool implementations have been removed.
pub mod agent_execution {
    /// Marker used when planning workflow blocks a mutating tool call.
    pub const PLANNING_DENIED_CONTEXT: &str = "tool denied by planning workflow";
    /// Prefix for loop detection failures.
    pub const LOOP_DETECTION_PREFIX: &str = "LOOP DETECTION";
    /// Canonical action-required line for loop detection blocks.
    pub const LOOP_RETRY_BLOCKED_LINE: &str =
        "ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.";

    /// Build the canonical Planning workflow denial message.
    pub fn planning_workflow_denial_message(tool_name: &str) -> String {
        format!(
            "Tool '{tool_name}' execution failed: tool denied by planning workflow\n\n\
             This tool is MUTATING and blocked during planning.\n\n\
             What you CAN do during planning:\n\
             - Read files: exec_command with readonly shell inspection commands such as sed, rg, ls, find, and git show\n\
             - Run readonly commands: cargo check, cargo test, git status, ls, grep, find, diff\n\
             - Search code: exec_command with rg or other readonly search commands\n\
             - Use task_tracker\n\n\
             To start implementation:\n\
             1. Call `finish_planning` tool to show the user your plan for approval\n\
             2. Wait for user to confirm (they will see the Implementation Blueprint)\n\
             3. After approval, use apply_patch for file edits\n\n\
             Fallback if automatic planning handoff keeps failing: call `finish_planning` to present the plan again."
        )
    }

    /// Build the canonical loop-detection block message.
    pub fn loop_detection_block_message(tool_name: &str, repeat_count: u64, original_error: Option<&str>) -> String {
        let mut message = format!(
            "{LOOP_DETECTION_PREFIX}: Tool '{tool_name}' has been called {repeat_count} times with identical parameters and is now blocked.\n\n\
             {LOOP_RETRY_BLOCKED_LINE}\n\n\
             If you need the result from this tool:\n\
             1. Check if you already have the result from a previous successful call in your conversation history\n\
             2. If not available, use a different approach or modify your request"
        );

        if let Some(error) = original_error {
            message.push_str("\n\nOriginal error: ");
            message.push_str(error);
        }

        message
    }

    /// Check whether an error string corresponds to planning workflow denial.
    pub fn is_planning_active_denial(error: &str) -> bool {
        error.contains(PLANNING_DENIED_CONTEXT)
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
        anyhow::anyhow!("Skill '{name}' not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn internal_unified_tool_name(suffix: &str) -> String {
        format!("unified_{suffix}")
    }

    #[test]
    fn test_agent_execution_message_helpers() {
        let planning_msg = agent_execution::planning_workflow_denial_message("write_file");
        assert!(agent_execution::is_planning_active_denial(&planning_msg));
        assert!(planning_msg.contains("finish_planning"));
        assert!(planning_msg.contains("MUTATING"));
        assert!(planning_msg.contains("cargo check"));
        assert!(planning_msg.contains("exec_command"));
        assert!(planning_msg.contains("apply_patch"));
        assert!(!planning_msg.contains(&internal_unified_tool_name("file")));
        assert!(!planning_msg.contains(&internal_unified_tool_name("exec")));
        assert!(!planning_msg.contains(&internal_unified_tool_name("search")));
        assert!(!planning_msg.contains(&format!("/{}", "mode")));
        assert!(!planning_msg.contains("DO NOT retry this tool or use /plan off"));

        let loop_msg = agent_execution::loop_detection_block_message("read_file", 3, Some("base error"));
        assert!(loop_msg.contains("LOOP DETECTION"));
        assert!(loop_msg.contains("DO NOT retry"));
        assert!(loop_msg.contains("Original error: base error"));
    }
}
